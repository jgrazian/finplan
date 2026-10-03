//! Event bodies, and the readers that rebuild an event's nested trigger and
//! effect specs from the graph's rows (the reverse of the lowering in
//! `tree`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{
    AmountSpec, Comparison, EffectParent, EffectSpec, Interval, OffsetUnit, TriggerParent,
    TriggerSpec,
};
use crate::batch::RowBatch;
use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Event {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub fires_once: bool,
    pub enabled: bool,
    pub sort_order: i64,
    pub trigger: TriggerSpec,
    pub effects: Vec<EffectSpec>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct EventBody {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fires_once: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Omitted appends a new event to the end of the list and leaves a replaced
    /// one where it already sat — the PUT that saves an edited trigger must not
    /// silently drag the row back to the top.
    #[serde(default)]
    pub sort_order: Option<i64>,
    pub trigger: TriggerSpec,
    #[serde(default)]
    pub effects: Vec<EffectSpec>,
}

fn yes() -> bool {
    true
}

// ── reading: rows back into nested specs ────────────────────────────────────

fn read_trigger(graph: &ScenarioGraph, trigger_id: i64, depth: usize) -> PlanResult<TriggerSpec> {
    if depth > 64 {
        return Err(PlanError::internal("trigger graph is cyclic"));
    }
    let row = graph
        .triggers
        .get(&trigger_id)
        .ok_or(PlanError::NotFound("trigger"))?;

    let comparison = || match row.comparison.as_deref() {
        Some("LessThanOrEqual") => Comparison::LessThanOrEqual,
        _ => Comparison::GreaterThanOrEqual,
    };

    Ok(match row.kind.as_str() {
        "Date" if row.parameter_id.is_some() => TriggerSpec::DateParameter {
            parameter_id: row.parameter_id.unwrap(),
        },
        "Age" if row.parameter_id.is_some() => TriggerSpec::AgeParameter {
            parameter_id: row.parameter_id.unwrap(),
        },
        "Date" => TriggerSpec::Date {
            on_date: row.on_date.clone().unwrap_or_default(),
        },
        "Age" => TriggerSpec::Age {
            years: row.age_years.unwrap_or_default() as u8,
            months: row.age_months.map(|m| m as u8),
        },
        "RelativeToEvent" => TriggerSpec::RelativeToEvent {
            event_id: row.ref_event_id.unwrap_or_default(),
            unit: match row.offset_unit.as_deref() {
                Some("Days") => OffsetUnit::Days,
                Some("Years") => OffsetUnit::Years,
                _ => OffsetUnit::Months,
            },
            value: row.offset_value.unwrap_or_default() as i32,
        },
        "AccountBalance" => TriggerSpec::AccountBalance {
            account_id: row.account_id.unwrap_or_default(),
            comparison: comparison(),
            threshold: row.threshold.unwrap_or_default(),
        },
        "AssetBalance" => TriggerSpec::AssetBalance {
            account_id: row.account_id.unwrap_or_default(),
            asset_id: row.asset_id.unwrap_or_default(),
            comparison: comparison(),
            threshold: row.threshold.unwrap_or_default(),
        },
        "NetWorth" => TriggerSpec::NetWorth {
            comparison: comparison(),
            threshold: row.threshold.unwrap_or_default(),
        },
        "And" | "Or" => {
            let mut children = Vec::new();
            for child in graph
                .trigger_children
                .get(&trigger_id)
                .into_iter()
                .flatten()
            {
                children.push(read_trigger(graph, *child, depth + 1)?);
            }
            if row.kind == "And" {
                TriggerSpec::And { children }
            } else {
                TriggerSpec::Or { children }
            }
        }
        "Repeating" => TriggerSpec::Repeating {
            interval: match row.interval.as_deref() {
                Some("Never") => Interval::Never,
                Some("Weekly") => Interval::Weekly,
                Some("BiWeekly") => Interval::BiWeekly,
                Some("Quarterly") => Interval::Quarterly,
                Some("Yearly") => Interval::Yearly,
                _ => Interval::Monthly,
            },
            start_condition: row
                .start_trigger_id
                .map(|id| read_trigger(graph, id, depth + 1).map(Box::new))
                .transpose()?,
            end_condition: row
                .end_trigger_id
                .map(|id| read_trigger(graph, id, depth + 1).map(Box::new))
                .transpose()?,
            max_occurrences: row.max_occurrences.map(|m| m as u32),
        },
        _ => TriggerSpec::Manual,
    })
}

fn read_amount(graph: &ScenarioGraph, amount_id: i64, depth: usize) -> PlanResult<AmountSpec> {
    if depth > 64 {
        return Err(PlanError::internal("transfer amount graph is cyclic"));
    }
    let row = graph
        .amounts
        .get(&amount_id)
        .ok_or(PlanError::NotFound("transfer amount"))?;

    if let Some(source) = &row.expression_source {
        return Ok(AmountSpec::Expression {
            source: source.clone(),
        });
    }

    let left = |depth: usize| -> PlanResult<Box<AmountSpec>> {
        let id = row
            .left_id
            .ok_or_else(|| PlanError::internal("amount is missing its operand"))?;
        Ok(Box::new(read_amount(graph, id, depth + 1)?))
    };
    let right = |depth: usize| -> PlanResult<Box<AmountSpec>> {
        let id = row
            .right_id
            .ok_or_else(|| PlanError::internal("amount is missing its right operand"))?;
        Ok(Box::new(read_amount(graph, id, depth + 1)?))
    };

    Ok(match row.kind.as_str() {
        "Fixed" => AmountSpec::Fixed {
            value: row.value.unwrap_or_default(),
        },
        "TargetToBalance" => AmountSpec::TargetToBalance {
            value: row.value.unwrap_or_default(),
        },
        "SourceBalance" => AmountSpec::SourceBalance,
        "ZeroTargetBalance" => AmountSpec::ZeroTargetBalance,
        "InflationAdjusted" => AmountSpec::InflationAdjusted {
            inner: left(depth)?,
        },
        "Scale" => AmountSpec::Scale {
            factor: row.value.unwrap_or(1.0),
            inner: left(depth)?,
        },
        "AssetBalance" => AmountSpec::AssetBalance {
            account_id: row.account_id.unwrap_or_default(),
            asset_id: row.asset_id.unwrap_or_default(),
        },
        "AccountTotalBalance" => AmountSpec::AccountTotalBalance {
            account_id: row.account_id.unwrap_or_default(),
        },
        "AccountCashBalance" => AmountSpec::AccountCashBalance {
            account_id: row.account_id.unwrap_or_default(),
        },
        "Min" => AmountSpec::Min {
            left: left(depth)?,
            right: right(depth)?,
        },
        "Max" => AmountSpec::Max {
            left: left(depth)?,
            right: right(depth)?,
        },
        "Sub" => AmountSpec::Sub {
            left: left(depth)?,
            right: right(depth)?,
        },
        "Add" => AmountSpec::Add {
            left: left(depth)?,
            right: right(depth)?,
        },
        _ => AmountSpec::Mul {
            left: left(depth)?,
            right: right(depth)?,
        },
    })
}

fn read_effect(graph: &ScenarioGraph, effect_id: i64, depth: usize) -> PlanResult<EffectSpec> {
    use crate::specs::{AmountMode, IncomeType, LotMethod};

    if depth > 64 {
        return Err(PlanError::internal("effect graph is cyclic"));
    }
    let row = graph
        .effects
        .get(&effect_id)
        .ok_or(PlanError::NotFound("effect"))?;

    let amount = || -> PlanResult<AmountSpec> {
        let id = row
            .amount_id
            .ok_or_else(|| PlanError::internal("effect is missing its amount"))?;
        read_amount(graph, id, depth)
    };
    let amount_mode = match row.amount_mode.as_deref() {
        Some("Gross") => AmountMode::Gross,
        _ => AmountMode::Net,
    };
    let income_type = match row.income_type.as_deref() {
        Some("TaxFree") => IncomeType::TaxFree,
        _ => IncomeType::Taxable,
    };
    let lot_method = match row.lot_method.as_deref() {
        Some("Lifo") => LotMethod::Lifo,
        Some("HighestCost") => LotMethod::HighestCost,
        Some("LowestCost") => LotMethod::LowestCost,
        Some("AverageCost") => LotMethod::AverageCost,
        _ => LotMethod::Fifo,
    };
    let to = row.to_account_id.unwrap_or_default();
    let from = row.from_account_id.unwrap_or_default();
    let target = row.target_event_id.unwrap_or_default();

    Ok(match row.kind.as_str() {
        "Income" => EffectSpec::Income {
            to_account_id: to,
            amount: amount()?,
            amount_mode,
            income_type,
        },
        "Expense" => EffectSpec::Expense {
            from_account_id: from,
            amount: amount()?,
        },
        "AssetPurchase" => EffectSpec::AssetPurchase {
            from_account_id: from,
            to_account_id: to,
            asset_id: row.asset_id.unwrap_or_default(),
            amount: amount()?,
        },
        "AssetSale" => EffectSpec::AssetSale {
            from_account_id: from,
            asset_id: row.asset_id,
            amount: amount()?,
            amount_mode,
            lot_method,
        },
        "Sweep" => EffectSpec::Sweep {
            to_account_id: to,
            amount: amount()?,
            sources: read_withdrawal_sources(graph, effect_id),
            amount_mode,
            lot_method,
            income_type,
        },
        "AdjustBalance" => EffectSpec::AdjustBalance {
            account_id: to,
            amount: amount()?,
        },
        "CashTransfer" => EffectSpec::CashTransfer {
            from_account_id: from,
            to_account_id: to,
            amount: amount()?,
        },
        "TriggerEvent" => EffectSpec::TriggerEvent {
            target_event_id: target,
        },
        "PauseEvent" => EffectSpec::PauseEvent {
            target_event_id: target,
        },
        "ResumeEvent" => EffectSpec::ResumeEvent {
            target_event_id: target,
        },
        "TerminateEvent" => EffectSpec::TerminateEvent {
            target_event_id: target,
        },
        "DeleteAccount" => EffectSpec::DeleteAccount { account_id: to },
        "ApplyRmd" => EffectSpec::ApplyRmd {
            to_account_id: to,
            lot_method,
        },
        "BuyProperty" => EffectSpec::BuyProperty {
            property_account_id: to,
            from_account_id: from,
            price: amount()?,
            financing: match (
                row.loan_account_id,
                row.down_payment_amount_id,
                row.term_months,
            ) {
                (Some(loan), Some(down), Some(term)) => Some(crate::specs::FinancingSpec {
                    loan_account_id: loan,
                    down_payment: read_amount(graph, down, depth)?,
                    term_months: u32::try_from(term).unwrap_or(360),
                }),
                _ => None,
            },
        },
        "MarketShock" => EffectSpec::MarketShock {
            drop: row.shock_drop.unwrap_or_default(),
        },
        "SellProperty" => EffectSpec::SellProperty {
            property_account_id: from,
            to_account_id: to,
            selling_cost_rate: row.selling_cost_rate.unwrap_or_default(),
            gain_exclusion: row.gain_exclusion.unwrap_or_default(),
            payoff_account_id: row.loan_account_id,
        },
        "RsuVesting" => EffectSpec::RsuVesting {
            to_account_id: to,
            asset_id: row.asset_id.unwrap_or_default(),
            units: row.units.unwrap_or_default(),
            sell_to_cover: row.sell_to_cover.unwrap_or(0) != 0,
            lot_method,
        },
        _ => {
            let on_true_id = graph
                .effect_children
                .get(&(effect_id, "on_true".to_string()))
                .ok_or_else(|| PlanError::internal("Random effect has no on_true branch"))?;
            EffectSpec::Random {
                probability: row.probability.unwrap_or_default(),
                on_true: Box::new(read_effect(graph, *on_true_id, depth + 1)?),
                on_false: graph
                    .effect_children
                    .get(&(effect_id, "on_false".to_string()))
                    .map(|id| read_effect(graph, *id, depth + 1).map(Box::new))
                    .transpose()?,
            }
        }
    })
}

fn read_withdrawal_sources(
    graph: &ScenarioGraph,
    effect_id: i64,
) -> Option<crate::specs::WithdrawalSourcesSpec> {
    use crate::specs::{AssetRef, WithdrawalSourcesSpec, WithdrawalStrategy};

    let row = graph.withdrawal_sources.get(&effect_id)?;
    let items = graph.withdrawal_items.get(&effect_id);

    Some(match row.mode.as_str() {
        "SingleAsset" => WithdrawalSourcesSpec::SingleAsset {
            account_id: row.account_id.unwrap_or_default(),
            asset_id: row.asset_id.unwrap_or_default(),
        },
        "SingleAccount" => WithdrawalSourcesSpec::SingleAccount {
            account_id: row.account_id.unwrap_or_default(),
        },
        "Custom" => WithdrawalSourcesSpec::Custom {
            entries: items
                .into_iter()
                .flatten()
                .filter(|i| i.role == "custom")
                .map(|i| AssetRef {
                    account_id: i.account_id,
                    asset_id: i.asset_id.unwrap_or_default(),
                })
                .collect(),
        },
        _ => WithdrawalSourcesSpec::Strategy {
            strategy: match row.strategy.as_deref() {
                Some("TaxDeferredFirst") => WithdrawalStrategy::TaxDeferredFirst,
                Some("TaxFreeFirst") => WithdrawalStrategy::TaxFreeFirst,
                Some("ProRata") => WithdrawalStrategy::ProRata,
                Some("PenaltyAware") => WithdrawalStrategy::PenaltyAware,
                Some("BracketFilling") => WithdrawalStrategy::BracketFilling,
                _ => WithdrawalStrategy::TaxEfficientEarly,
            },
            bracket_ceiling: row.bracket_ceiling,
            exclude_accounts: items
                .into_iter()
                .flatten()
                .filter(|i| i.role == "exclude")
                .map(|i| i.account_id)
                .collect(),
        },
    })
}

pub fn read_event(graph: &ScenarioGraph, event_id: i64) -> PlanResult<Event> {
    let row = graph
        .events
        .iter()
        .find(|e| e.id == event_id)
        .ok_or(PlanError::NotFound("event"))?;

    let trigger_id = graph
        .event_trigger
        .get(&event_id)
        .ok_or_else(|| PlanError::internal("event has no trigger"))?;

    let mut effects = Vec::new();
    for id in graph.event_effects.get(&event_id).into_iter().flatten() {
        effects.push(read_effect(graph, *id, 0)?);
    }

    Ok(Event {
        id: row.id,
        name: row.name.clone(),
        description: row.description.clone(),
        fires_once: row.fires_once != 0,
        enabled: row.enabled != 0,
        sort_order: row.sort_order,
        trigger: read_trigger(graph, *trigger_id, 0)?,
        effects,
    })
}

/// Lower `body`'s trigger and effects into `batch`, hung on event `event_id`.
pub fn lower_tree(batch: &mut RowBatch, event_id: i64, body: &EventBody) -> PlanResult<()> {
    body.trigger
        .lower(batch, TriggerParent::Event(event_id), 0)?;

    for (position, effect) in body.effects.iter().enumerate() {
        effect.lower(
            batch,
            EffectParent::Event {
                event_id,
                position: position as i64,
            },
            0,
        )?;
    }
    Ok(())
}
