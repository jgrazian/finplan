//! Request/response shapes for the recursive parts of the model.
//!
//! Clients send triggers, transfer amounts and effects as nested JSON. The
//! lowering below flattens that tree into rows of the self-referential tables,
//! collected in a [`RowBatch`] that is then written to the database or merged
//! into an in-memory graph; the readers rebuild the tree. Keeping the wire
//! format nested keeps the API pleasant while the storage stays normalized.

use serde::{Deserialize, Serialize};

use crate::batch::RowBatch;
use crate::error::{PlanError, PlanResult};
use crate::graph::{
    EffectRow, TransferAmountRow, TriggerRow, WithdrawalItemRow, WithdrawalSourceRow,
};
use ts_rs::TS;

/// Matches `compile::MAX_DEPTH`, enforced on the way in so a pathological
/// payload is rejected at write time rather than at simulation time.
const MAX_SPEC_DEPTH: usize = 64;

fn check_depth(depth: usize) -> PlanResult<()> {
    if depth > MAX_SPEC_DEPTH {
        return Err(PlanError::invalid(format!(
            "expression nests deeper than {MAX_SPEC_DEPTH} levels"
        )));
    }
    Ok(())
}

// ── transfer amounts ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum AmountSpec {
    Expression {
        source: String,
    },
    Fixed {
        value: f64,
    },
    InflationAdjusted {
        inner: Box<AmountSpec>,
    },
    SourceBalance,
    ZeroTargetBalance,
    TargetToBalance {
        value: f64,
    },
    AssetBalance {
        account_id: i64,
        asset_id: i64,
    },
    AccountTotalBalance {
        account_id: i64,
    },
    AccountCashBalance {
        account_id: i64,
    },
    Min {
        left: Box<AmountSpec>,
        right: Box<AmountSpec>,
    },
    Max {
        left: Box<AmountSpec>,
        right: Box<AmountSpec>,
    },
    Sub {
        left: Box<AmountSpec>,
        right: Box<AmountSpec>,
    },
    Add {
        left: Box<AmountSpec>,
        right: Box<AmountSpec>,
    },
    Mul {
        left: Box<AmountSpec>,
        right: Box<AmountSpec>,
    },
    Scale {
        factor: f64,
        inner: Box<AmountSpec>,
    },
}

impl AmountSpec {
    /// Lower this expression tree into `batch`, returning the root row's local id.
    pub fn lower(&self, batch: &mut RowBatch, depth: usize) -> PlanResult<i64> {
        check_depth(depth)?;
        let nested_expression = match self {
            AmountSpec::InflationAdjusted { inner } | AmountSpec::Scale { inner, .. } => {
                matches!(inner.as_ref(), AmountSpec::Expression { .. })
            }
            AmountSpec::Min { left, right }
            | AmountSpec::Max { left, right }
            | AmountSpec::Sub { left, right }
            | AmountSpec::Add { left, right }
            | AmountSpec::Mul { left, right } => {
                matches!(left.as_ref(), AmountSpec::Expression { .. })
                    || matches!(right.as_ref(), AmountSpec::Expression { .. })
            }
            _ => false,
        };
        if nested_expression {
            return Err(PlanError::invalid(
                "an Expression must be the root amount; put the full calculation in its source",
            ));
        }

        // Children first, so the parent row can reference them.
        let (kind, value, account_id, asset_id, left_id, right_id) = match self {
            AmountSpec::Expression { .. } => ("Fixed", Some(0.0), None, None, None, None),
            AmountSpec::Fixed { value } => ("Fixed", Some(*value), None, None, None, None),
            AmountSpec::TargetToBalance { value } => {
                ("TargetToBalance", Some(*value), None, None, None, None)
            }
            AmountSpec::SourceBalance => ("SourceBalance", None, None, None, None, None),
            AmountSpec::ZeroTargetBalance => ("ZeroTargetBalance", None, None, None, None, None),
            AmountSpec::AssetBalance {
                account_id,
                asset_id,
            } => (
                "AssetBalance",
                None,
                Some(*account_id),
                Some(*asset_id),
                None,
                None,
            ),
            AmountSpec::AccountTotalBalance { account_id } => (
                "AccountTotalBalance",
                None,
                Some(*account_id),
                None,
                None,
                None,
            ),
            AmountSpec::AccountCashBalance { account_id } => (
                "AccountCashBalance",
                None,
                Some(*account_id),
                None,
                None,
                None,
            ),
            AmountSpec::InflationAdjusted { inner } => {
                let id = inner.lower(batch, depth + 1)?;
                ("InflationAdjusted", None, None, None, Some(id), None)
            }
            AmountSpec::Scale { factor, inner } => {
                let id = inner.lower(batch, depth + 1)?;
                ("Scale", Some(*factor), None, None, Some(id), None)
            }
            AmountSpec::Min { left, right }
            | AmountSpec::Max { left, right }
            | AmountSpec::Sub { left, right }
            | AmountSpec::Add { left, right }
            | AmountSpec::Mul { left, right } => {
                let l = left.lower(batch, depth + 1)?;
                let r = right.lower(batch, depth + 1)?;
                let kind = match self {
                    AmountSpec::Min { .. } => "Min",
                    AmountSpec::Max { .. } => "Max",
                    AmountSpec::Sub { .. } => "Sub",
                    AmountSpec::Add { .. } => "Add",
                    _ => "Mul",
                };
                (kind, None, None, None, Some(l), Some(r))
            }
        };

        Ok(batch.push_amount(TransferAmountRow {
            id: 0,
            kind: kind.to_string(),
            value,
            account_id,
            asset_id,
            left_id,
            right_id,
            expression_source: match self {
                AmountSpec::Expression { source } => Some(source.clone()),
                _ => None,
            },
        }))
    }
}

// ── triggers ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Comparison {
    GreaterThanOrEqual,
    LessThanOrEqual,
}

impl Comparison {
    fn as_str(self) -> &'static str {
        match self {
            Comparison::GreaterThanOrEqual => "GreaterThanOrEqual",
            Comparison::LessThanOrEqual => "LessThanOrEqual",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum OffsetUnit {
    Days,
    Months,
    Years,
}

impl OffsetUnit {
    fn as_str(self) -> &'static str {
        match self {
            OffsetUnit::Days => "Days",
            OffsetUnit::Months => "Months",
            OffsetUnit::Years => "Years",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Interval {
    Never,
    Weekly,
    BiWeekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl Interval {
    fn as_str(self) -> &'static str {
        match self {
            Interval::Never => "Never",
            Interval::Weekly => "Weekly",
            Interval::BiWeekly => "BiWeekly",
            Interval::Monthly => "Monthly",
            Interval::Quarterly => "Quarterly",
            Interval::Yearly => "Yearly",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum TriggerSpec {
    DateParameter {
        parameter_id: i64,
    },
    AgeParameter {
        parameter_id: i64,
    },
    Date {
        on_date: String,
    },
    Age {
        years: u8,
        #[serde(default)]
        months: Option<u8>,
    },
    RelativeToEvent {
        event_id: i64,
        unit: OffsetUnit,
        value: i32,
    },
    AccountBalance {
        account_id: i64,
        comparison: Comparison,
        threshold: f64,
    },
    AssetBalance {
        account_id: i64,
        asset_id: i64,
        comparison: Comparison,
        threshold: f64,
    },
    NetWorth {
        comparison: Comparison,
        threshold: f64,
    },
    And {
        children: Vec<TriggerSpec>,
    },
    Or {
        children: Vec<TriggerSpec>,
    },
    Repeating {
        interval: Interval,
        #[serde(default)]
        start_condition: Option<Box<TriggerSpec>>,
        #[serde(default)]
        end_condition: Option<Box<TriggerSpec>>,
        #[serde(default)]
        max_occurrences: Option<u32>,
    },
    Manual,
}

/// Where a trigger row hangs: at the root of an event, or inside a parent.
#[derive(Debug, Clone, Copy)]
pub enum TriggerParent {
    Event(i64),
    Node {
        /// The parent's local id in the same [`RowBatch`].
        parent_id: i64,
        position: i64,
    },
    /// A `Repeating` start/end condition: linked from the parent's own column,
    /// so it carries neither an event nor a parent link.
    Detached,
}

impl TriggerSpec {
    /// Lower this condition tree into `batch`, returning the root row's local id.
    ///
    /// `TriggerParent::Node`'s `parent_id` is a local id in the same batch.
    pub fn lower(
        &self,
        batch: &mut RowBatch,
        parent: TriggerParent,
        depth: usize,
    ) -> PlanResult<i64> {
        check_depth(depth)?;
        if let TriggerSpec::DateParameter { parameter_id }
        | TriggerSpec::AgeParameter { parameter_id } = self
        {
            let required = if matches!(self, TriggerSpec::DateParameter { .. }) {
                "Date"
            } else {
                "Age"
            };
            if batch.parameter_kind(*parameter_id) != Some(required) {
                return Err(PlanError::invalid(format!(
                    "{required} trigger requires a {required} parameter from this scenario"
                )));
            }
        }

        let (event_id, parent_id, position) = match parent {
            TriggerParent::Event(id) => (Some(id), None, 0),
            TriggerParent::Node {
                parent_id,
                position,
            } => (None, Some(parent_id), position),
            TriggerParent::Detached => (None, None, 0),
        };

        // Repeating sub-conditions are lowered first so their ids can go in
        // this row's columns.
        let (start_id, end_id) = match self {
            TriggerSpec::Repeating {
                start_condition,
                end_condition,
                ..
            } => {
                let start = start_condition
                    .as_ref()
                    .map(|spec| spec.lower(batch, TriggerParent::Detached, depth + 1))
                    .transpose()?;
                let end = end_condition
                    .as_ref()
                    .map(|spec| spec.lower(batch, TriggerParent::Detached, depth + 1))
                    .transpose()?;
                (start, end)
            }
            _ => (None, None),
        };

        let kind = match self {
            TriggerSpec::DateParameter { .. } => "Date",
            TriggerSpec::AgeParameter { .. } => "Age",
            TriggerSpec::Date { .. } => "Date",
            TriggerSpec::Age { .. } => "Age",
            TriggerSpec::RelativeToEvent { .. } => "RelativeToEvent",
            TriggerSpec::AccountBalance { .. } => "AccountBalance",
            TriggerSpec::AssetBalance { .. } => "AssetBalance",
            TriggerSpec::NetWorth { .. } => "NetWorth",
            TriggerSpec::And { .. } => "And",
            TriggerSpec::Or { .. } => "Or",
            TriggerSpec::Repeating { .. } => "Repeating",
            TriggerSpec::Manual => "Manual",
        };

        let on_date = match self {
            TriggerSpec::DateParameter { .. } => Some("2000-01-01".to_string()),
            TriggerSpec::Date { on_date } => Some(validate_date(on_date)?),
            _ => None,
        };
        let (age_years, age_months) = match self {
            TriggerSpec::AgeParameter { .. } => (Some(0), None),
            TriggerSpec::Age { years, months } => (Some(i64::from(*years)), months.map(i64::from)),
            _ => (None, None),
        };
        let (ref_event_id, offset_unit, offset_value) = match self {
            TriggerSpec::RelativeToEvent {
                event_id,
                unit,
                value,
            } => (
                Some(*event_id),
                Some(unit.as_str()),
                Some(i64::from(*value)),
            ),
            _ => (None, None, None),
        };
        let (account_id, asset_id) = match self {
            TriggerSpec::AccountBalance { account_id, .. } => (Some(*account_id), None),
            TriggerSpec::AssetBalance {
                account_id,
                asset_id,
                ..
            } => (Some(*account_id), Some(*asset_id)),
            _ => (None, None),
        };
        let (comparison, threshold) = match self {
            TriggerSpec::AccountBalance {
                comparison,
                threshold,
                ..
            }
            | TriggerSpec::AssetBalance {
                comparison,
                threshold,
                ..
            }
            | TriggerSpec::NetWorth {
                comparison,
                threshold,
            } => (Some(comparison.as_str()), Some(*threshold)),
            _ => (None, None),
        };
        let (interval, max_occurrences) = match self {
            TriggerSpec::Repeating {
                interval,
                max_occurrences,
                ..
            } => (Some(interval.as_str()), max_occurrences.map(i64::from)),
            _ => (None, None),
        };

        let id = batch.push_trigger(TriggerRow {
            id: 0,
            event_id,
            kind: kind.to_string(),
            on_date,
            age_years,
            age_months,
            ref_event_id,
            offset_unit: offset_unit.map(str::to_string),
            offset_value,
            account_id,
            asset_id,
            comparison: comparison.map(str::to_string),
            threshold,
            interval: interval.map(str::to_string),
            start_trigger_id: start_id,
            end_trigger_id: end_id,
            max_occurrences,
            parent_id,
            position,
            parameter_id: match self {
                TriggerSpec::DateParameter { parameter_id }
                | TriggerSpec::AgeParameter { parameter_id } => Some(*parameter_id),
                _ => None,
            },
        });

        if let TriggerSpec::And { children } | TriggerSpec::Or { children } = self {
            if children.is_empty() {
                return Err(PlanError::invalid(
                    "And/Or triggers need at least one child condition",
                ));
            }
            for (position, child) in children.iter().enumerate() {
                child.lower(
                    batch,
                    TriggerParent::Node {
                        parent_id: id,
                        position: position as i64,
                    },
                    depth + 1,
                )?;
            }
        }

        Ok(id)
    }
}

fn validate_date(text: &str) -> PlanResult<String> {
    text.parse::<jiff::civil::Date>()
        .map(|d| d.to_string())
        .map_err(|e| PlanError::invalid(format!("invalid date '{text}': {e}")))
}

// ── effects ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, TS)]
#[ts(export)]
pub enum AmountMode {
    Gross,
    #[default]
    Net,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum IncomeType {
    Taxable,
    TaxFree,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, TS)]
#[ts(export)]
pub enum LotMethod {
    #[default]
    Fifo,
    Lifo,
    HighestCost,
    LowestCost,
    AverageCost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum WithdrawalStrategy {
    TaxEfficientEarly,
    TaxDeferredFirst,
    TaxFreeFirst,
    ProRata,
    PenaltyAware,
    /// Tax-deferred first up to the top of `bracket_ceiling`'s bracket, from
    /// 59.5; otherwise as `PenaltyAware`.
    BracketFilling,
}

impl WithdrawalStrategy {
    /// The name stored in `withdrawal_sources.strategy` and the funding policy.
    pub fn as_str(self) -> &'static str {
        match self {
            WithdrawalStrategy::TaxEfficientEarly => "TaxEfficientEarly",
            WithdrawalStrategy::TaxDeferredFirst => "TaxDeferredFirst",
            WithdrawalStrategy::TaxFreeFirst => "TaxFreeFirst",
            WithdrawalStrategy::ProRata => "ProRata",
            WithdrawalStrategy::PenaltyAware => "PenaltyAware",
            WithdrawalStrategy::BracketFilling => "BracketFilling",
        }
    }

    /// The inverse of [`Self::as_str`].
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "TaxEfficientEarly" => WithdrawalStrategy::TaxEfficientEarly,
            "TaxDeferredFirst" => WithdrawalStrategy::TaxDeferredFirst,
            "TaxFreeFirst" => WithdrawalStrategy::TaxFreeFirst,
            "ProRata" => WithdrawalStrategy::ProRata,
            "PenaltyAware" => WithdrawalStrategy::PenaltyAware,
            "BracketFilling" => WithdrawalStrategy::BracketFilling,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "mode")]
#[ts(export)]
pub enum WithdrawalSourcesSpec {
    SingleAsset {
        account_id: i64,
        asset_id: i64,
    },
    SingleAccount {
        account_id: i64,
    },
    Strategy {
        strategy: WithdrawalStrategy,
        #[serde(default)]
        exclude_accounts: Vec<i64>,
        /// `BracketFilling` only: the highest marginal rate to fill to, as a
        /// fraction. Absent means 12%.
        #[serde(default)]
        #[ts(optional)]
        bracket_ceiling: Option<f64>,
    },
    Custom {
        /// Ordered (account, asset) pairs to draw from.
        entries: Vec<AssetRef>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssetRef {
    pub account_id: i64,
    pub asset_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum EffectSpec {
    Income {
        to_account_id: i64,
        amount: AmountSpec,
        #[serde(default)]
        amount_mode: AmountMode,
        income_type: IncomeType,
    },
    Expense {
        from_account_id: i64,
        amount: AmountSpec,
    },
    AssetPurchase {
        from_account_id: i64,
        to_account_id: i64,
        asset_id: i64,
        amount: AmountSpec,
    },
    AssetSale {
        from_account_id: i64,
        #[serde(default)]
        asset_id: Option<i64>,
        amount: AmountSpec,
        #[serde(default)]
        amount_mode: AmountMode,
        #[serde(default)]
        lot_method: LotMethod,
    },
    Sweep {
        to_account_id: i64,
        amount: AmountSpec,
        #[serde(default)]
        sources: Option<WithdrawalSourcesSpec>,
        #[serde(default)]
        amount_mode: AmountMode,
        #[serde(default)]
        lot_method: LotMethod,
        income_type: IncomeType,
    },
    AdjustBalance {
        account_id: i64,
        amount: AmountSpec,
    },
    CashTransfer {
        from_account_id: i64,
        to_account_id: i64,
        amount: AmountSpec,
    },
    TriggerEvent {
        target_event_id: i64,
    },
    PauseEvent {
        target_event_id: i64,
    },
    ResumeEvent {
        target_event_id: i64,
    },
    TerminateEvent {
        target_event_id: i64,
    },
    DeleteAccount {
        account_id: i64,
    },
    ApplyRmd {
        to_account_id: i64,
        #[serde(default)]
        lot_method: LotMethod,
    },
    RsuVesting {
        to_account_id: i64,
        asset_id: i64,
        units: f64,
        #[serde(default)]
        sell_to_cover: bool,
        #[serde(default)]
        lot_method: LotMethod,
    },
    Random {
        probability: f64,
        on_true: Box<EffectSpec>,
        #[serde(default)]
        on_false: Option<Box<EffectSpec>>,
    },
    /// Buy a home: the price lands on the Property account, the cash side
    /// leaves `from_account_id` as a transfer, and a financed purchase draws
    /// the loan for the rest and starts it amortizing.
    BuyProperty {
        property_account_id: i64,
        from_account_id: i64,
        price: AmountSpec,
        #[serde(default)]
        financing: Option<FinancingSpec>,
    },
    /// Sell a home: proceeds net of selling costs and gains tax land in
    /// `to_account_id`, after paying off `payoff_account_id` if named.
    SellProperty {
        property_account_id: i64,
        to_account_id: i64,
        /// Share of the sale price lost to fees and closing costs, 0–1.
        #[serde(default)]
        selling_cost_rate: f64,
        /// Gain excluded from tax: 250000 single, 500000 joint, 0 if not a
        /// primary residence.
        #[serde(default)]
        gain_exclusion: f64,
        #[serde(default)]
        payoff_account_id: Option<i64>,
    },
    /// A one-time market crash: every market asset's price (not cash,
    /// property or debt) drops by `drop`, a fraction in (0, 1).
    MarketShock {
        drop: f64,
    },
}

/// How a `BuyProperty` is financed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FinancingSpec {
    /// Liability account drawn for price less down payment, at its own rate.
    pub loan_account_id: i64,
    pub down_payment: AmountSpec,
    /// Months to amortize over — 360 for a 30-year mortgage.
    pub term_months: u32,
}

/// Where an effect row hangs: in an event's ordered list, or in a `Random` slot.
#[derive(Debug, Clone, Copy)]
pub enum EffectParent {
    Event {
        event_id: i64,
        position: i64,
    },
    Branch {
        /// The `Random` effect's local id in the same [`RowBatch`].
        parent_id: i64,
        on_true: bool,
    },
}

impl EffectSpec {
    /// Lower this effect (and any `Random` branches) into `batch`, returning
    /// its row's local id.
    ///
    /// `EffectParent::Branch`'s `parent_id` is a local id in the same batch.
    pub fn lower(
        &self,
        batch: &mut RowBatch,
        parent: EffectParent,
        depth: usize,
    ) -> PlanResult<i64> {
        check_depth(depth)?;

        let (event_id, parent_id, parent_slot, position) = match parent {
            EffectParent::Event { event_id, position } => (Some(event_id), None, None, position),
            EffectParent::Branch { parent_id, on_true } => (
                None,
                Some(parent_id),
                Some(if on_true { "on_true" } else { "on_false" }),
                0,
            ),
        };

        let amount_id = self
            .amount_spec()
            .map(|spec| spec.lower(batch, depth))
            .transpose()?;
        let down_payment_amount_id = match self {
            EffectSpec::BuyProperty {
                financing: Some(financing),
                ..
            } => Some(financing.down_payment.lower(batch, depth)?),
            _ => None,
        };
        if let EffectSpec::SellProperty {
            selling_cost_rate,
            gain_exclusion,
            ..
        } = self
        {
            if !(0.0..=1.0).contains(selling_cost_rate) {
                return Err(PlanError::invalid(
                    "selling costs are a share of the price, between 0 and 1",
                ));
            }
            if *gain_exclusion < 0.0 {
                return Err(PlanError::invalid("the gain exclusion cannot be negative"));
            }
        }
        if let EffectSpec::MarketShock { drop } = self
            && !(*drop > 0.0 && *drop < 1.0)
        {
            return Err(PlanError::invalid(
                "a market shock's drop is a fraction between 0 and 1",
            ));
        }
        if let EffectSpec::BuyProperty {
            financing: Some(financing),
            ..
        } = self
            && financing.term_months == 0
        {
            return Err(PlanError::invalid(
                "a mortgage term must be at least one month",
            ));
        }

        let f = EffectFields::from(self);
        let id = batch.push_effect(EffectRow {
            id: 0,
            event_id,
            parent_id,
            parent_slot: parent_slot.map(str::to_string),
            position,
            kind: f.kind.to_string(),
            from_account_id: f.from_account_id,
            to_account_id: f.to_account_id,
            asset_id: f.asset_id,
            amount_id,
            target_event_id: f.target_event_id,
            amount_mode: f.amount_mode.map(str::to_string),
            income_type: f.income_type.map(str::to_string),
            lot_method: f.lot_method.map(str::to_string),
            probability: f.probability,
            units: f.units,
            sell_to_cover: f.sell_to_cover,
            loan_account_id: f.loan_account_id,
            down_payment_amount_id,
            term_months: f.term_months,
            selling_cost_rate: f.selling_cost_rate,
            gain_exclusion: f.gain_exclusion,
            shock_drop: f.shock_drop,
        });

        match self {
            EffectSpec::Sweep {
                sources: Some(sources),
                ..
            } => lower_withdrawal_sources(batch, id, sources)?,
            EffectSpec::Random {
                on_true, on_false, ..
            } => {
                on_true.lower(
                    batch,
                    EffectParent::Branch {
                        parent_id: id,
                        on_true: true,
                    },
                    depth + 1,
                )?;
                if let Some(on_false) = on_false {
                    on_false.lower(
                        batch,
                        EffectParent::Branch {
                            parent_id: id,
                            on_true: false,
                        },
                        depth + 1,
                    )?;
                }
            }
            _ => {}
        }

        Ok(id)
    }

    fn amount_spec(&self) -> Option<&AmountSpec> {
        match self {
            EffectSpec::Income { amount, .. }
            | EffectSpec::Expense { amount, .. }
            | EffectSpec::AssetPurchase { amount, .. }
            | EffectSpec::AssetSale { amount, .. }
            | EffectSpec::Sweep { amount, .. }
            | EffectSpec::AdjustBalance { amount, .. }
            | EffectSpec::CashTransfer { amount, .. }
            | EffectSpec::BuyProperty { price: amount, .. } => Some(amount),
            _ => None,
        }
    }
}

/// The flat column values for one effect row.
struct EffectFields {
    kind: &'static str,
    from_account_id: Option<i64>,
    to_account_id: Option<i64>,
    asset_id: Option<i64>,
    target_event_id: Option<i64>,
    amount_mode: Option<&'static str>,
    income_type: Option<&'static str>,
    lot_method: Option<&'static str>,
    probability: Option<f64>,
    units: Option<f64>,
    sell_to_cover: Option<i64>,
    loan_account_id: Option<i64>,
    term_months: Option<i64>,
    selling_cost_rate: Option<f64>,
    gain_exclusion: Option<f64>,
    shock_drop: Option<f64>,
}

impl EffectFields {
    fn blank(kind: &'static str) -> Self {
        Self {
            kind,
            from_account_id: None,
            to_account_id: None,
            asset_id: None,
            target_event_id: None,
            amount_mode: None,
            income_type: None,
            lot_method: None,
            probability: None,
            units: None,
            sell_to_cover: None,
            loan_account_id: None,
            term_months: None,
            selling_cost_rate: None,
            gain_exclusion: None,
            shock_drop: None,
        }
    }
}

fn mode_str(mode: AmountMode) -> &'static str {
    match mode {
        AmountMode::Gross => "Gross",
        AmountMode::Net => "Net",
    }
}

fn income_str(income: IncomeType) -> &'static str {
    match income {
        IncomeType::Taxable => "Taxable",
        IncomeType::TaxFree => "TaxFree",
    }
}

fn lot_str(lot: LotMethod) -> &'static str {
    match lot {
        LotMethod::Fifo => "Fifo",
        LotMethod::Lifo => "Lifo",
        LotMethod::HighestCost => "HighestCost",
        LotMethod::LowestCost => "LowestCost",
        LotMethod::AverageCost => "AverageCost",
    }
}

impl From<&EffectSpec> for EffectFields {
    fn from(spec: &EffectSpec) -> Self {
        match spec {
            EffectSpec::Income {
                to_account_id,
                amount_mode,
                income_type,
                ..
            } => EffectFields {
                to_account_id: Some(*to_account_id),
                amount_mode: Some(mode_str(*amount_mode)),
                income_type: Some(income_str(*income_type)),
                ..EffectFields::blank("Income")
            },
            EffectSpec::Expense {
                from_account_id, ..
            } => EffectFields {
                from_account_id: Some(*from_account_id),
                ..EffectFields::blank("Expense")
            },
            EffectSpec::AssetPurchase {
                from_account_id,
                to_account_id,
                asset_id,
                ..
            } => EffectFields {
                from_account_id: Some(*from_account_id),
                to_account_id: Some(*to_account_id),
                asset_id: Some(*asset_id),
                ..EffectFields::blank("AssetPurchase")
            },
            EffectSpec::AssetSale {
                from_account_id,
                asset_id,
                amount_mode,
                lot_method,
                ..
            } => EffectFields {
                from_account_id: Some(*from_account_id),
                asset_id: *asset_id,
                amount_mode: Some(mode_str(*amount_mode)),
                lot_method: Some(lot_str(*lot_method)),
                ..EffectFields::blank("AssetSale")
            },
            EffectSpec::Sweep {
                to_account_id,
                amount_mode,
                lot_method,
                income_type,
                ..
            } => EffectFields {
                to_account_id: Some(*to_account_id),
                amount_mode: Some(mode_str(*amount_mode)),
                lot_method: Some(lot_str(*lot_method)),
                income_type: Some(income_str(*income_type)),
                ..EffectFields::blank("Sweep")
            },
            EffectSpec::AdjustBalance { account_id, .. } => EffectFields {
                to_account_id: Some(*account_id),
                ..EffectFields::blank("AdjustBalance")
            },
            EffectSpec::CashTransfer {
                from_account_id,
                to_account_id,
                ..
            } => EffectFields {
                from_account_id: Some(*from_account_id),
                to_account_id: Some(*to_account_id),
                ..EffectFields::blank("CashTransfer")
            },
            EffectSpec::TriggerEvent { target_event_id } => EffectFields {
                target_event_id: Some(*target_event_id),
                ..EffectFields::blank("TriggerEvent")
            },
            EffectSpec::PauseEvent { target_event_id } => EffectFields {
                target_event_id: Some(*target_event_id),
                ..EffectFields::blank("PauseEvent")
            },
            EffectSpec::ResumeEvent { target_event_id } => EffectFields {
                target_event_id: Some(*target_event_id),
                ..EffectFields::blank("ResumeEvent")
            },
            EffectSpec::TerminateEvent { target_event_id } => EffectFields {
                target_event_id: Some(*target_event_id),
                ..EffectFields::blank("TerminateEvent")
            },
            EffectSpec::DeleteAccount { account_id } => EffectFields {
                to_account_id: Some(*account_id),
                ..EffectFields::blank("DeleteAccount")
            },
            EffectSpec::ApplyRmd {
                to_account_id,
                lot_method,
            } => EffectFields {
                to_account_id: Some(*to_account_id),
                lot_method: Some(lot_str(*lot_method)),
                ..EffectFields::blank("ApplyRmd")
            },
            EffectSpec::RsuVesting {
                to_account_id,
                asset_id,
                units,
                sell_to_cover,
                lot_method,
            } => EffectFields {
                to_account_id: Some(*to_account_id),
                asset_id: Some(*asset_id),
                units: Some(*units),
                sell_to_cover: Some(i64::from(*sell_to_cover)),
                lot_method: Some(lot_str(*lot_method)),
                ..EffectFields::blank("RsuVesting")
            },
            EffectSpec::Random { probability, .. } => EffectFields {
                probability: Some(*probability),
                ..EffectFields::blank("Random")
            },
            EffectSpec::BuyProperty {
                property_account_id,
                from_account_id,
                financing,
                ..
            } => EffectFields {
                from_account_id: Some(*from_account_id),
                to_account_id: Some(*property_account_id),
                loan_account_id: financing.as_ref().map(|f| f.loan_account_id),
                term_months: financing.as_ref().map(|f| i64::from(f.term_months)),
                ..EffectFields::blank("BuyProperty")
            },
            EffectSpec::SellProperty {
                property_account_id,
                to_account_id,
                selling_cost_rate,
                gain_exclusion,
                payoff_account_id,
            } => EffectFields {
                from_account_id: Some(*property_account_id),
                to_account_id: Some(*to_account_id),
                loan_account_id: *payoff_account_id,
                selling_cost_rate: Some(*selling_cost_rate),
                gain_exclusion: Some(*gain_exclusion),
                ..EffectFields::blank("SellProperty")
            },
            EffectSpec::MarketShock { drop } => EffectFields {
                shock_drop: Some(*drop),
                ..EffectFields::blank("MarketShock")
            },
        }
    }
}

/// Lower a sweep's withdrawal sources onto the effect at local id `effect_id`.
fn lower_withdrawal_sources(
    batch: &mut RowBatch,
    effect_id: i64,
    spec: &WithdrawalSourcesSpec,
) -> PlanResult<()> {
    let (mode, account_id, asset_id, strategy) = match spec {
        WithdrawalSourcesSpec::SingleAsset {
            account_id,
            asset_id,
        } => ("SingleAsset", Some(*account_id), Some(*asset_id), None),
        WithdrawalSourcesSpec::SingleAccount { account_id } => {
            ("SingleAccount", Some(*account_id), None, None)
        }
        WithdrawalSourcesSpec::Strategy { strategy, .. } => {
            ("Strategy", None, None, Some(strategy.as_str()))
        }
        WithdrawalSourcesSpec::Custom { .. } => ("Custom", None, None, None),
    };
    // Only bracket filling reads a ceiling; anything else would be dead weight.
    let bracket_ceiling = match spec {
        WithdrawalSourcesSpec::Strategy {
            strategy: WithdrawalStrategy::BracketFilling,
            bracket_ceiling,
            ..
        } => *bracket_ceiling,
        _ => None,
    };
    if let Some(rate) = bracket_ceiling
        && !(0.0..1.0).contains(&rate)
    {
        return Err(PlanError::unprocessable(
            "bracket_ceiling must be a rate from 0 up to (not including) 1",
        ));
    }

    batch.push_withdrawal_source(WithdrawalSourceRow {
        effect_id,
        mode: mode.to_string(),
        account_id,
        asset_id,
        strategy: strategy.map(str::to_string),
        bracket_ceiling,
    });

    match spec {
        WithdrawalSourcesSpec::Strategy {
            exclude_accounts, ..
        } => {
            for (position, account_id) in exclude_accounts.iter().enumerate() {
                batch.push_withdrawal_item(WithdrawalItemRow {
                    effect_id,
                    role: "exclude".to_string(),
                    position: position as i64,
                    account_id: *account_id,
                    asset_id: None,
                });
            }
        }
        WithdrawalSourcesSpec::Custom { entries } => {
            if entries.is_empty() {
                return Err(PlanError::invalid(
                    "a Custom withdrawal source needs at least one entry",
                ));
            }
            for (position, entry) in entries.iter().enumerate() {
                batch.push_withdrawal_item(WithdrawalItemRow {
                    effect_id,
                    role: "custom".to_string(),
                    position: position as i64,
                    account_id: entry.account_id,
                    asset_id: Some(entry.asset_id),
                });
            }
        }
        _ => {}
    }

    Ok(())
}
