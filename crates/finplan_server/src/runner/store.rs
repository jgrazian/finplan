//! Persist a `MonteCarloSummary` into the normalized `run_*` tables.
//!
//! Engine output is keyed by dense simulation ids, so every account reference is
//! translated back through `IdMap` before it is written. Percentile paths are
//! stored with their percentile; the mean path is stored with `percentile IS
//! NULL`, so bands and mean share one table and one query shape.

use finplan_core::model::{MonteCarloSummary, SimulationResult, WarningKind, final_net_worth};

use crate::compile::CompiledScenario;
use crate::db::Db;
use crate::runner::ledger;

pub async fn mark_failed(db: &Db, run_id: i64, message: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE runs SET status = 'failed', error_message = ?2, finished_at = datetime('now')
          WHERE id = ?1 AND status IN ('queued','running')",
    )
    .bind(run_id)
    .bind(message)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn mark_canceled(db: &Db, run_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE runs SET status = 'canceled', finished_at = datetime('now')
          WHERE id = ?1 AND status IN ('queued','running')",
    )
    .bind(run_id)
    .execute(db)
    .await?;
    Ok(())
}

fn warning_kind_name(kind: WarningKind) -> &'static str {
    match kind {
        WarningKind::EffectSkipped => "EffectSkipped",
        WarningKind::EvaluationFailed => "EvaluationFailed",
        WarningKind::IterationLimitHit => "IterationLimitHit",
        WarningKind::CashShortfall => "CashShortfall",
    }
}

/// Total tax paid across the whole horizon of a single path.
fn lifetime_taxes(result: &SimulationResult) -> f64 {
    result
        .yearly_taxes
        .iter()
        .map(|t| t.total_tax + t.early_withdrawal_penalties)
        .sum()
}

pub async fn persist(
    db: &Db,
    run_id: i64,
    compiled: &CompiledScenario,
    summary: &MonteCarloSummary,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;

    // A re-run of the same id should replace, not append.
    for table in [
        "run_real_stats",
        "run_stats",
        "run_percentile_values",
        "run_net_worth_points",
        "run_account_points",
        "run_cash_flows",
        "run_taxes",
        "run_warnings",
        "run_inflation",
        "run_ledger",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE run_id = ?1"))
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
    }

    let stats = &summary.stats;

    // Lifetime taxes are reported from the median path when it is available,
    // since a mean-of-paths tax figure is not attributable to any real run.
    let median_taxes = summary
        .percentile_runs
        .iter()
        .min_by(|a, b| {
            (a.0 - 0.5)
                .abs()
                .partial_cmp(&(b.0 - 0.5).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(_, result)| lifetime_taxes(result))
        .unwrap_or(0.0);

    sqlx::query(
        "INSERT INTO run_stats (run_id, num_iterations, success_rate, mean_final_net_worth,
                                std_dev_final_net_worth, min_final_net_worth, max_final_net_worth,
                                lifetime_taxes, converged, convergence_metric, convergence_value,
                                funding_success_rate)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )
    .bind(run_id)
    .bind(stats.num_iterations as i64)
    .bind(stats.success_rate)
    .bind(stats.mean_final_net_worth)
    .bind(stats.std_dev_final_net_worth)
    .bind(stats.min_final_net_worth)
    .bind(stats.max_final_net_worth)
    .bind(median_taxes)
    .bind(stats.converged.map(i64::from))
    .bind(stats.convergence_metric.as_ref().map(|m| format!("{m:?}")))
    .bind(stats.convergence_value)
    .bind(stats.funding_success_rate)
    .execute(&mut *tx)
    .await?;

    for (percentile, value) in &stats.percentile_values {
        sqlx::query(
            "INSERT INTO run_percentile_values (run_id, percentile, final_net_worth)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(run_id, percentile) DO UPDATE SET final_net_worth = excluded.final_net_worth",
        )
        .bind(run_id)
        .bind(*percentile)
        .bind(*value)
        .execute(&mut *tx)
        .await?;
    }

    if let Some(real) = &summary.real_net_worth {
        sqlx::query(
            "INSERT INTO run_real_stats (run_id, base_date, num_iterations, mean, std_dev, min, max)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(run_id)
        .bind(real.base_date.to_string())
        .bind(real.num_iterations as i64)
        .bind(real.terminal.mean)
        .bind(real.terminal.std_dev)
        .bind(real.terminal.min)
        .bind(real.terminal.max)
        .execute(&mut *tx).await?;
        for point in &real.points {
            sqlx::query(
                "INSERT INTO run_real_quantiles (run_id, as_of_date, p5, p50, p95)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(run_id)
            .bind(point.date.to_string())
            .bind(point.p5)
            .bind(point.p50)
            .bind(point.p95)
            .execute(&mut *tx)
            .await?;
        }
    }

    for (percentile, result) in &summary.percentile_runs {
        write_path(&mut tx, run_id, Some(*percentile), compiled, result).await?;
    }

    if let Some(mean) = summary.get_mean_result() {
        write_path(&mut tx, run_id, None, compiled, &mean).await?;
    }

    sqlx::query(
        "UPDATE runs SET status = 'succeeded', finished_at = datetime('now'),
                         completed_iterations = ?2, error_message = NULL
          WHERE id = ?1",
    )
    .bind(run_id)
    .bind(stats.num_iterations as i64)
    .execute(&mut *tx)
    .await?;

    tx.commit().await
}

/// Write one path (a percentile run, or the mean) to every per-path table.
async fn write_path(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: i64,
    percentile: Option<f64>,
    compiled: &CompiledScenario,
    result: &SimulationResult,
) -> Result<(), sqlx::Error> {
    for (step, snapshot) in result.wealth_snapshots.iter().enumerate() {
        let date = snapshot.date.to_string();
        let net_worth: f64 = snapshot.accounts.iter().map(|a| a.total_value()).sum();

        sqlx::query(
            "INSERT INTO run_net_worth_points (run_id, percentile, step, as_of_date, net_worth)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(step as i64)
        .bind(&date)
        .bind(net_worth)
        .execute(&mut **tx)
        .await?;

        for account in &snapshot.accounts {
            // An account created mid-simulation by a `CreateAccount` effect has
            // no database row to attribute to; its value still counts toward net
            // worth above, it just gets no per-account series.
            let Some(db_id) = compiled.id_map.account_db_id(account.account_id) else {
                continue;
            };

            sqlx::query(
                "INSERT INTO run_account_points (run_id, percentile, account_id, step, value)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(run_id)
            .bind(percentile)
            .bind(db_id)
            .bind(step as i64)
            .bind(account.total_value())
            .execute(&mut **tx)
            .await?;
        }
    }

    for flow in &result.yearly_cash_flows {
        sqlx::query(
            "INSERT INTO run_cash_flows (run_id, percentile, year, income, expenses,
                                         contributions, withdrawals, appreciation, net_cash_flow)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(i64::from(flow.year))
        .bind(flow.income)
        .bind(flow.expenses)
        .bind(flow.contributions)
        .bind(flow.withdrawals)
        .bind(flow.appreciation)
        .bind(flow.net_cash_flow)
        .execute(&mut **tx)
        .await?;
    }

    for tax in &result.yearly_taxes {
        sqlx::query(
            "INSERT INTO run_taxes (run_id, percentile, year, ordinary_income, capital_gains,
                                    tax_free_withdrawals, federal_tax, state_tax, total_tax,
                                    early_withdrawal_penalties)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(i64::from(tax.year))
        .bind(tax.ordinary_income)
        .bind(tax.capital_gains)
        .bind(tax.tax_free_withdrawals)
        .bind(tax.federal_tax)
        .bind(tax.state_tax)
        .bind(tax.total_tax)
        .bind(tax.early_withdrawal_penalties)
        .execute(&mut **tx)
        .await?;
    }

    // The path's own realised inflation, keyed by calendar year so a client can
    // deflate any figure it holds without knowing the plan's step cadence.
    // Index 0 is the plan's first year, where the factor is 1.0 by definition.
    if let Some(first) = result.wealth_snapshots.first() {
        let start_year = i64::from(first.date.year());
        for (offset, factor) in result.cumulative_inflation.iter().enumerate() {
            sqlx::query(
                "INSERT INTO run_inflation (run_id, percentile, year, factor)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(run_id)
            .bind(percentile)
            .bind(start_year + offset as i64)
            .bind(*factor)
            .execute(&mut **tx)
            .await?;
        }
    }

    // The ledger, flattened. Empty when the scenario has `collect_ledger` off,
    // and for the synthetic mean path, which averages figures rather than
    // replaying any one sequence of events.
    let names = ledger::Names::new(compiled);
    let mut position = 0i64;
    for entry in &result.ledger {
        let Some(flat) = ledger::flatten(entry, &names) else {
            continue;
        };
        sqlx::query(
            "INSERT INTO run_ledger (run_id, percentile, position, as_of_date, year, category,
                                     kind, detail, amount, basis, basis_label, account_id, event_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(position)
        .bind(entry.date.to_string())
        .bind(i64::from(entry.date.year()))
        .bind(flat.category)
        .bind(&flat.kind)
        .bind(&flat.detail)
        .bind(flat.amount)
        .bind(flat.basis)
        .bind(flat.basis_label)
        .bind(flat.account_id)
        .bind(flat.event_id)
        .execute(&mut **tx)
        .await?;
        position += 1;
    }

    for (position, warning) in result.warnings.iter().enumerate() {
        let event_db_id = warning
            .event_id
            .and_then(|id| compiled.id_map.event_db_id(id));
        sqlx::query(
            "INSERT INTO run_warnings (run_id, percentile, position, kind, as_of_date, event_id, message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(position as i64)
        .bind(warning_kind_name(warning.kind))
        .bind(warning.date.to_string())
        .bind(event_db_id)
        .bind(&warning.message)
        .execute(&mut **tx)
        .await?;
    }

    // Keep the engine's own net-worth helper honest against what we stored.
    debug_assert!(
        result.wealth_snapshots.is_empty()
            || (final_net_worth(result)
                - result
                    .wealth_snapshots
                    .last()
                    .map(|s| s.accounts.iter().map(|a| a.total_value()).sum::<f64>())
                    .unwrap_or_default())
            .abs()
                < 1.0
    );

    Ok(())
}
