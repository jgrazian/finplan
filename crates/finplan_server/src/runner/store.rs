//! Persist a run's [`RunResults`] into the normalized `run_*` tables.
//!
//! The engine's output is shaped once, by `finplan_plan::results::project`,
//! which also translates every account and event reference from dense
//! simulation ids to database ids; this module only writes that value out.
//! Percentile paths are stored with their percentile; the mean path is stored
//! with `percentile IS NULL`, so bands and mean share one table and one query
//! shape.

use finplan_plan::results::{PathResults, RunResults};

use crate::db::{CURRENT_RUN_ONLY_TABLES, Db};

pub async fn mark_failed(db: &Db, run_id: i64, message: &str) -> Result<bool, sqlx::Error> {
    let changed = sqlx::query(
        "UPDATE runs SET status = 'failed', error_message = ?2, finished_at = datetime('now')
          WHERE id = ?1 AND status IN ('queued','running')",
    )
    .bind(run_id)
    .bind(message)
    .execute(db)
    .await?
    .rows_affected();
    Ok(changed == 1)
}

pub async fn mark_canceled(db: &Db, run_id: i64) -> Result<bool, sqlx::Error> {
    let changed = sqlx::query(
        "UPDATE runs SET status = 'canceled', finished_at = datetime('now')
          WHERE id = ?1 AND status IN ('queued','running')",
    )
    .bind(run_id)
    .execute(db)
    .await?
    .rows_affected();
    Ok(changed == 1)
}

pub async fn persist(db: &Db, run_id: i64, results: &RunResults) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    // Lock and claim the terminal transition in the same transaction as result
    // writes. Cascading deletion cannot produce a fictitious success.
    let claimed =
        sqlx::query("UPDATE runs SET status = 'succeeded' WHERE id = ? AND status = 'running'")
            .bind(run_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if claimed == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

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

    let stats = &results.stats;

    // Stored already translated to row ids, the shape the API serves.
    let funding_json = results
        .funding_diagnostics
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| sqlx::Error::Encode(Box::new(e)))?;

    sqlx::query(
        "INSERT INTO run_stats (run_id, num_iterations, success_rate, mean_final_net_worth,
                                std_dev_final_net_worth, min_final_net_worth, max_final_net_worth,
                                lifetime_taxes, converged, convergence_metric, convergence_value,
                                funding_success_rate, funding_diagnostics, after_tax_final)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )
    .bind(run_id)
    .bind(stats.num_iterations)
    .bind(stats.success_rate)
    .bind(stats.mean_final_net_worth)
    .bind(stats.std_dev_final_net_worth)
    .bind(stats.min_final_net_worth)
    .bind(stats.max_final_net_worth)
    .bind(stats.lifetime_taxes)
    .bind(stats.converged.map(i64::from))
    .bind(&stats.convergence_metric)
    .bind(stats.convergence_value)
    .bind(stats.funding_success_rate)
    .bind(funding_json)
    .bind(results.after_tax_final)
    .execute(&mut *tx)
    .await?;

    for value in &stats.percentile_values {
        sqlx::query(
            "INSERT INTO run_percentile_values (run_id, percentile, final_net_worth)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(run_id, percentile) DO UPDATE SET final_net_worth = excluded.final_net_worth",
        )
        .bind(run_id)
        .bind(value.percentile)
        .bind(value.final_net_worth)
        .execute(&mut *tx)
        .await?;
    }

    if let Some(real) = &results.real_net_worth {
        sqlx::query(
            "INSERT INTO run_real_stats (run_id, base_date, num_iterations, mean, std_dev, min, max)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(run_id)
        .bind(&real.terminal.base_date)
        .bind(real.terminal.num_iterations)
        .bind(real.terminal.mean)
        .bind(real.terminal.std_dev)
        .bind(real.terminal.min)
        .bind(real.terminal.max)
        .execute(&mut *tx).await?;
        for point in &real.points {
            sqlx::query(
                "INSERT INTO run_real_quantiles
                     (run_id, as_of_date, p5, p10, p25, p50, p75, p90, p95)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .bind(run_id)
            .bind(&point.date)
            .bind(point.p5)
            .bind(point.p10)
            .bind(point.p25)
            .bind(point.p50)
            .bind(point.p75)
            .bind(point.p90)
            .bind(point.p95)
            .execute(&mut *tx)
            .await?;
        }
    }

    for path in &results.paths {
        write_path(&mut tx, run_id, path).await?;
        // The percentile rows were made at enqueue; the seed that replays the
        // path is only known now. Kept for every run (not a current-run-only
        // table), so a later view can re-simulate the median.
        if let (Some(percentile), Some(seed)) = (path.percentile, &path.seed) {
            sqlx::query(
                "UPDATE run_percentiles SET seed = ?3 WHERE run_id = ?1 AND percentile = ?2",
            )
            .bind(run_id)
            .bind(percentile)
            .bind(seed)
            .execute(&mut *tx)
            .await?;
        }
    }

    sqlx::query(
        "UPDATE runs SET status = 'succeeded', finished_at = datetime('now'),
                         completed_iterations = ?2, error_message = NULL
          WHERE id = ?1",
    )
    .bind(run_id)
    .bind(stats.num_iterations)
    .execute(&mut *tx)
    .await?;

    // Path series and the itemised ledger are high-volume details that the UI
    // only needs for the current result. Keep them only for the newest
    // successful run in this scenario; older runs keep their summary stats.
    // Pruning at success rather than enqueue leaves the prior details intact if
    // a replacement is canceled or fails. Selecting by id also makes an older
    // concurrent run discard its own details if a newer run has already
    // succeeded.
    for table in CURRENT_RUN_ONLY_TABLES {
        sqlx::query(&format!(
            "DELETE FROM {table}
              WHERE run_id IN (
                    SELECT candidate.id
                      FROM runs AS candidate
                     WHERE candidate.scenario_id = (SELECT scenario_id FROM runs WHERE id = ?1)
                       AND candidate.id <> (
                            SELECT MAX(latest.id)
                              FROM runs AS latest
                             WHERE latest.scenario_id = candidate.scenario_id
                               AND latest.status = 'succeeded'
                       )
              )"
        ))
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await
}

/// Write one path (a percentile run, or the mean) to every per-path table.
async fn write_path(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: i64,
    path: &PathResults,
) -> Result<(), sqlx::Error> {
    let percentile = path.percentile;

    for (step, point) in path.net_worth.iter().enumerate() {
        sqlx::query(
            "INSERT INTO run_net_worth_points (run_id, percentile, step, as_of_date, net_worth)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(step as i64)
        .bind(&point.date)
        .bind(point.net_worth)
        .execute(&mut **tx)
        .await?;
    }

    for point in &path.account_points {
        sqlx::query(
            "INSERT INTO run_account_points (run_id, percentile, account_id, step, value, cash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(point.account_id)
        .bind(point.step as i64)
        .bind(point.value)
        .bind(point.cash)
        .execute(&mut **tx)
        .await?;
    }

    for flow in &path.cash_flows {
        sqlx::query(
            "INSERT INTO run_cash_flows (run_id, percentile, year, income, expenses,
                                         contributions, withdrawals, appreciation, net_cash_flow)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(flow.year)
        .bind(flow.income)
        .bind(flow.expenses)
        .bind(flow.contributions)
        .bind(flow.withdrawals)
        .bind(flow.appreciation)
        .bind(flow.net_cash_flow)
        .execute(&mut **tx)
        .await?;
    }

    for tax in &path.taxes {
        sqlx::query(
            "INSERT INTO run_taxes (run_id, percentile, year, ordinary_income, capital_gains,
                                    tax_free_withdrawals, federal_tax, state_tax, total_tax,
                                    early_withdrawal_penalties)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(tax.year)
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

    for point in &path.inflation {
        sqlx::query(
            "INSERT INTO run_inflation (run_id, percentile, year, factor)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(point.year)
        .bind(point.factor)
        .execute(&mut **tx)
        .await?;
    }

    for entry in &path.ledger {
        sqlx::query(
            "INSERT INTO run_ledger (run_id, percentile, position, as_of_date, year, category,
                                     kind, detail, amount, basis, basis_label, account_id, event_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(entry.position)
        .bind(&entry.date)
        .bind(entry.year)
        .bind(&entry.category)
        .bind(&entry.kind)
        .bind(&entry.detail)
        .bind(entry.amount)
        .bind(entry.basis)
        .bind(&entry.basis_label)
        .bind(entry.account_id)
        .bind(entry.event_id)
        .execute(&mut **tx)
        .await?;
    }

    for (position, warning) in path.warnings.iter().enumerate() {
        sqlx::query(
            "INSERT INTO run_warnings (run_id, percentile, position, kind, as_of_date, event_id,
                                       account_id, message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(run_id)
        .bind(percentile)
        .bind(position as i64)
        .bind(&warning.kind)
        .bind(&warning.date)
        .bind(warning.event_id)
        .bind(warning.account_id)
        .bind(&warning.message)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}
