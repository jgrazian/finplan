//! Rows lowered from nested specs, not yet written anywhere.
//!
//! `specs` flattens a trigger, amount or effect tree into the rows of the
//! self-referential tables. It does that into a [`RowBatch`] rather than
//! straight into SQL, so the same rows can land in one of two places:
//!
//!   * `db::batch::insert` in the server writes them to the database, which is
//!     what every route that saves an event does, and
//!   * [`RowBatch::merge_into`] adds them to an in-memory [`ScenarioGraph`],
//!     which is how an edit is previewed without touching the database.
//!
//! Both read the same rows, so what a preview simulates is what a save writes.
//!
//! Inside a batch every row has a *local* id: its 1-based position in the
//! batch. Links between rows of the same tree (`left_id`, `parent_id`,
//! `amount_id`, a withdrawal source's `effect_id`, …) hold local ids, and each
//! sink maps them to real ids as it places the rows. Links out of the tree —
//! accounts, assets, events, parameters — already hold real ids. A row only ever
//! links to rows pushed before it, so one forward pass places everything.

use std::collections::{HashMap, HashSet};

use crate::error::{PlanError, PlanResult};
use crate::graph::{
    EffectRow, ScenarioGraph, Table, TransferAmountRow, TriggerRow, WithdrawalItemRow,
    WithdrawalSourceRow,
};

/// One row of a batch, in the order it will be written.
#[derive(Debug, Clone)]
pub enum BatchRow {
    Amount(TransferAmountRow),
    Trigger(TriggerRow),
    Effect(EffectRow),
    WithdrawalSource(WithdrawalSourceRow),
    WithdrawalItem(WithdrawalItemRow),
}

/// Rows lowered from one or more spec trees, ready for a sink.
#[derive(Debug, Clone)]
pub struct RowBatch {
    rows: Vec<BatchRow>,
    /// The scenario's named parameters by id, as their `kind`. Parameter
    /// triggers check the kind while lowering, so the batch needs it up front.
    parameter_kinds: HashMap<i64, String>,
}

/// Where each row of a batch landed: the real id for each local id.
#[derive(Debug, Clone, Default)]
pub struct Placed {
    ids: Vec<Option<i64>>,
}

impl Placed {
    /// The real id of the row pushed as `local`; `None` for withdrawal rows,
    /// which are keyed by their effect rather than an id of their own.
    pub fn id(&self, local: i64) -> Option<i64> {
        usize::try_from(local - 1)
            .ok()
            .and_then(|i| self.ids.get(i).copied().flatten())
    }

    /// An empty record with room for `rows` ids, for a sink to fill as it
    /// places the batch row by row.
    pub fn with_capacity(rows: usize) -> Self {
        Self {
            ids: Vec::with_capacity(rows),
        }
    }

    /// Record where the next row of the batch landed (`None` for the
    /// withdrawal rows, which have no id of their own).
    pub fn record(&mut self, id: Option<i64>) {
        self.ids.push(id);
    }

    /// The real id of the row pushed as `local`, or an internal error if it
    /// has not been placed yet.
    pub fn resolve(&self, local: i64) -> PlanResult<i64> {
        self.id(local)
            .ok_or_else(|| PlanError::internal(format!("row batch links to unplaced row {local}")))
    }

    /// [`Self::resolve`] for an optional link.
    pub fn resolve_opt(&self, local: Option<i64>) -> PlanResult<Option<i64>> {
        local.map(|l| self.resolve(l)).transpose()
    }
}

impl RowBatch {
    pub fn new(parameter_kinds: HashMap<i64, String>) -> Self {
        Self {
            rows: Vec::new(),
            parameter_kinds,
        }
    }

    /// A batch that checks parameter triggers against `graph`'s parameters.
    pub fn for_graph(graph: &ScenarioGraph) -> Self {
        Self::new(
            graph
                .parameters
                .iter()
                .map(|p| (p.id, p.kind.clone()))
                .collect(),
        )
    }

    pub fn parameter_kind(&self, id: i64) -> Option<&str> {
        self.parameter_kinds.get(&id).map(String::as_str)
    }

    pub fn rows(&self) -> &[BatchRow] {
        &self.rows
    }

    fn push(&mut self, row: BatchRow) -> i64 {
        self.rows.push(row);
        self.rows.len() as i64
    }

    /// Queue an amount row; its `id` is ignored. Returns its local id.
    pub fn push_amount(&mut self, row: TransferAmountRow) -> i64 {
        self.push(BatchRow::Amount(row))
    }

    /// Queue a trigger row; its `id` is ignored. Returns its local id.
    pub fn push_trigger(&mut self, row: TriggerRow) -> i64 {
        self.push(BatchRow::Trigger(row))
    }

    /// Queue an effect row; its `id` is ignored. Returns its local id.
    pub fn push_effect(&mut self, row: EffectRow) -> i64 {
        self.push(BatchRow::Effect(row))
    }

    pub fn push_withdrawal_source(&mut self, row: WithdrawalSourceRow) {
        self.push(BatchRow::WithdrawalSource(row));
    }

    pub fn push_withdrawal_item(&mut self, row: WithdrawalItemRow) {
        self.push(BatchRow::WithdrawalItem(row));
    }

    /// Add every row to `graph` with fresh ids, and re-index it.
    ///
    /// Fresh ids start above the largest id each table already holds, so they
    /// never collide with a stored row. References out of the batch are checked
    /// against `graph` first — the in-memory stand-in for the foreign keys the
    /// database would enforce — and nothing is added unless all of them hold.
    /// A failure after that point is a bug in the lowering, and may leave
    /// `graph` half-merged; callers that need atomicity merge into a copy.
    /// Unlike the database, which accepts any existing row, the check is scoped
    /// to this plan.
    pub fn merge_into(&self, graph: &mut ScenarioGraph) -> PlanResult<Placed> {
        self.check_references(graph)?;

        let mut next_amount = graph.next_id(Table::Amounts);
        let mut next_trigger = graph.next_id(Table::Triggers);
        let mut next_effect = graph.next_id(Table::Effects);

        let mut placed = Placed::with_capacity(self.rows.len());
        for row in &self.rows {
            let id =
                match row {
                    BatchRow::Amount(r) => {
                        let id = next_amount;
                        next_amount += 1;
                        let row = TransferAmountRow {
                            id,
                            left_id: placed.resolve_opt(r.left_id)?,
                            right_id: placed.resolve_opt(r.right_id)?,
                            ..r.clone()
                        };
                        graph.amounts.insert(id, row);
                        Some(id)
                    }
                    BatchRow::Trigger(r) => {
                        let id = next_trigger;
                        next_trigger += 1;
                        let row = TriggerRow {
                            id,
                            start_trigger_id: placed.resolve_opt(r.start_trigger_id)?,
                            end_trigger_id: placed.resolve_opt(r.end_trigger_id)?,
                            parent_id: placed.resolve_opt(r.parent_id)?,
                            ..r.clone()
                        };
                        graph.triggers.insert(id, row);
                        Some(id)
                    }
                    BatchRow::Effect(r) => {
                        let id = next_effect;
                        next_effect += 1;
                        let row = EffectRow {
                            id,
                            parent_id: placed.resolve_opt(r.parent_id)?,
                            amount_id: placed.resolve_opt(r.amount_id)?,
                            down_payment_amount_id: placed.resolve_opt(r.down_payment_amount_id)?,
                            ..r.clone()
                        };
                        graph.effects.insert(id, row);
                        Some(id)
                    }
                    BatchRow::WithdrawalSource(r) => {
                        let effect_id = placed.resolve(r.effect_id)?;
                        graph.withdrawal_sources.insert(
                            effect_id,
                            WithdrawalSourceRow {
                                effect_id,
                                ..r.clone()
                            },
                        );
                        None
                    }
                    BatchRow::WithdrawalItem(r) => {
                        let effect_id = placed.resolve(r.effect_id)?;
                        graph.withdrawal_items.entry(effect_id).or_default().push(
                            WithdrawalItemRow {
                                effect_id,
                                ..r.clone()
                            },
                        );
                        None
                    }
                };
            placed.record(id);
        }
        graph.reindex();
        Ok(placed)
    }

    /// Every reference out of the batch names a row `graph` holds.
    fn check_references(&self, graph: &ScenarioGraph) -> PlanResult<()> {
        let accounts: HashSet<i64> = graph.accounts.iter().map(|a| a.id).collect();
        let assets: HashSet<i64> = graph.assets.iter().map(|a| a.id).collect();
        let events: HashSet<i64> = graph.events.iter().map(|e| e.id).collect();
        let parameters: HashSet<i64> = graph.parameters.iter().map(|p| p.id).collect();

        let check = |set: &HashSet<i64>, id: Option<i64>, what: &str| -> PlanResult<()> {
            match id {
                Some(id) if !set.contains(&id) => Err(PlanError::invalid(format!(
                    "{what} {id} does not exist in this plan"
                ))),
                _ => Ok(()),
            }
        };

        let mut roots = HashSet::new();
        for row in &self.rows {
            match row {
                BatchRow::Amount(r) => {
                    check(&accounts, r.account_id, "account")?;
                    check(&assets, r.asset_id, "asset")?;
                }
                BatchRow::Trigger(r) => {
                    check(&events, r.event_id, "event")?;
                    // `triggers.event_id` is UNIQUE: an event has one root.
                    if let Some(event_id) = r.event_id
                        && (graph.event_trigger.contains_key(&event_id) || !roots.insert(event_id))
                    {
                        return Err(PlanError::internal(format!(
                            "event {event_id} already has a trigger"
                        )));
                    }
                    check(&events, r.ref_event_id, "event")?;
                    check(&accounts, r.account_id, "account")?;
                    check(&assets, r.asset_id, "asset")?;
                    check(&parameters, r.parameter_id, "parameter")?;
                }
                BatchRow::Effect(r) => {
                    check(&events, r.event_id, "event")?;
                    check(&events, r.target_event_id, "event")?;
                    check(&accounts, r.from_account_id, "account")?;
                    check(&accounts, r.to_account_id, "account")?;
                    check(&accounts, r.loan_account_id, "account")?;
                    check(&accounts, r.pay_tax_from_account_id, "account")?;
                    check(&assets, r.asset_id, "asset")?;
                }
                BatchRow::WithdrawalSource(r) => {
                    check(&accounts, r.account_id, "account")?;
                    check(&assets, r.asset_id, "asset")?;
                }
                BatchRow::WithdrawalItem(r) => {
                    check(&accounts, Some(r.account_id), "account")?;
                    check(&assets, r.asset_id, "asset")?;
                }
            }
        }
        Ok(())
    }
}
