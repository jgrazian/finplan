//! The SQL sink for a [`RowBatch`]: writing lowered rows to the database.
//!
//! The batch itself (and its in-memory sink, `merge_into`) lives in
//! `finplan_plan::batch`; only what needs a connection is here.

use std::collections::HashMap;

use finplan_plan::batch::{BatchRow, Placed, RowBatch};
use sqlx::SqliteConnection;

use crate::error::ApiResult;

/// A batch that checks parameter triggers against the stored scenario.
pub(crate) async fn batch_for_scenario(
    conn: &mut SqliteConnection,
    scenario_id: i64,
) -> ApiResult<RowBatch> {
    let kinds: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, kind FROM named_parameters WHERE scenario_id = ?1")
            .bind(scenario_id)
            .fetch_all(&mut *conn)
            .await?;
    Ok(RowBatch::new(kinds.into_iter().collect::<HashMap<_, _>>()))
}

/// Write every row of `batch` to `scenario_id`'s tables, in batch order.
///
/// Row ids come from the database, in the order the rows were lowered —
/// the same order the recursive inserts this replaced used to issue.
pub(crate) async fn insert(
    conn: &mut SqliteConnection,
    scenario_id: i64,
    batch: &RowBatch,
) -> ApiResult<Placed> {
    let mut placed = Placed::with_capacity(batch.rows().len());
    for row in batch.rows() {
        let id = match row {
            BatchRow::Amount(r) => Some(
                sqlx::query_scalar(
                    "INSERT INTO transfer_amounts
                        (scenario_id, kind, value, account_id, asset_id, left_id, right_id, expression_source)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id",
                )
                .bind(scenario_id)
                .bind(&r.kind)
                .bind(r.value)
                .bind(r.account_id)
                .bind(r.asset_id)
                .bind(placed.resolve_opt(r.left_id)?)
                .bind(placed.resolve_opt(r.right_id)?)
                .bind(&r.expression_source)
                .fetch_one(&mut *conn)
                .await?,
            ),
            BatchRow::Trigger(r) => Some(
                sqlx::query_scalar(
                    "INSERT INTO triggers
                        (scenario_id, event_id, kind, on_date, age_years, age_months, ref_event_id,
                         offset_unit, offset_value, account_id, asset_id, comparison, threshold,
                         interval, start_trigger_id, end_trigger_id, max_occurrences, parent_id, position, parameter_id)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)
                     RETURNING id",
                )
                .bind(scenario_id)
                .bind(r.event_id)
                .bind(&r.kind)
                .bind(&r.on_date)
                .bind(r.age_years)
                .bind(r.age_months)
                .bind(r.ref_event_id)
                .bind(&r.offset_unit)
                .bind(r.offset_value)
                .bind(r.account_id)
                .bind(r.asset_id)
                .bind(&r.comparison)
                .bind(r.threshold)
                .bind(&r.interval)
                .bind(placed.resolve_opt(r.start_trigger_id)?)
                .bind(placed.resolve_opt(r.end_trigger_id)?)
                .bind(r.max_occurrences)
                .bind(placed.resolve_opt(r.parent_id)?)
                .bind(r.position)
                .bind(r.parameter_id)
                .fetch_one(&mut *conn)
                .await?,
            ),
            BatchRow::Effect(r) => Some(
                sqlx::query_scalar(
                    "INSERT INTO effects
                        (scenario_id, event_id, parent_id, parent_slot, position, kind,
                         from_account_id, to_account_id, asset_id, amount_id, target_event_id,
                         amount_mode, income_type, lot_method, probability, units, sell_to_cover,
                         loan_account_id, down_payment_amount_id, term_months, selling_cost_rate,
                         gain_exclusion, shock_drop, pay_tax_from_account_id)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,
                             ?18,?19,?20,?21,?22,?23,?24)
                     RETURNING id",
                )
                .bind(scenario_id)
                .bind(r.event_id)
                .bind(placed.resolve_opt(r.parent_id)?)
                .bind(&r.parent_slot)
                .bind(r.position)
                .bind(&r.kind)
                .bind(r.from_account_id)
                .bind(r.to_account_id)
                .bind(r.asset_id)
                .bind(placed.resolve_opt(r.amount_id)?)
                .bind(r.target_event_id)
                .bind(&r.amount_mode)
                .bind(&r.income_type)
                .bind(&r.lot_method)
                .bind(r.probability)
                .bind(r.units)
                .bind(r.sell_to_cover)
                .bind(r.loan_account_id)
                .bind(placed.resolve_opt(r.down_payment_amount_id)?)
                .bind(r.term_months)
                .bind(r.selling_cost_rate)
                .bind(r.gain_exclusion)
                .bind(r.shock_drop)
                .bind(r.pay_tax_from_account_id)
                .fetch_one(&mut *conn)
                .await?,
            ),
            BatchRow::WithdrawalSource(r) => {
                sqlx::query(
                    "INSERT INTO effect_withdrawal_sources
                        (effect_id, mode, account_id, asset_id, strategy, bracket_ceiling)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .bind(placed.resolve(r.effect_id)?)
                .bind(&r.mode)
                .bind(r.account_id)
                .bind(r.asset_id)
                .bind(&r.strategy)
                .bind(r.bracket_ceiling)
                .execute(&mut *conn)
                .await?;
                None
            }
            BatchRow::WithdrawalItem(r) => {
                sqlx::query(
                    "INSERT INTO effect_withdrawal_source_items
                        (effect_id, role, position, account_id, asset_id)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .bind(placed.resolve(r.effect_id)?)
                .bind(&r.role)
                .bind(r.position)
                .bind(r.account_id)
                .bind(r.asset_id)
                .execute(&mut *conn)
                .await?;
                None
            }
        };
        placed.record(id);
    }
    Ok(placed)
}
