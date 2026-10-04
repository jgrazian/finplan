//! Swapping a strategy into a compiled plan.

use finplan_core::config::SimulationConfig;
use finplan_core::model::{EventEffect, FundingPolicy, WithdrawalOrder, WithdrawalSources};
use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::StrategyChoice;
use crate::compile::{CompiledScenario, withdrawal_order};
use crate::error::PlanResult;

/// An event with a sweep that sells from a fixed place (one asset, one account
/// or an explicit list), which no strategy changes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FixedSweep {
    pub event_id: i64,
    pub name: String,
}

/// The engine order a choice stands for; `None` for `AsPlanned`.
pub(super) fn order_of(choice: &StrategyChoice) -> PlanResult<Option<WithdrawalOrder>> {
    match choice {
        StrategyChoice::AsPlanned => Ok(None),
        StrategyChoice::Strategy {
            strategy,
            bracket_ceiling,
        } => withdrawal_order(Some(strategy.as_str()), *bracket_ceiling).map(Some),
    }
}

/// `config` with `choice` swapped in, and whether that took an overlay.
///
/// Every sweep that takes a strategy gets the order (so do those nested in a
/// `Random` effect); the funding policy's order becomes it too, or, when the
/// plan has none, a policy is installed from `retirement` and reported as an
/// overlay. `AsPlanned` is the config untouched.
pub fn normalize(
    config: &SimulationConfig,
    choice: &StrategyChoice,
    retirement: Date,
) -> PlanResult<(SimulationConfig, bool)> {
    let Some(order) = order_of(choice)? else {
        return Ok((config.clone(), false));
    };
    let mut config = config.clone();
    for event in &mut config.events {
        for effect in &mut event.effects {
            set_order(effect, order);
        }
    }
    let overlay = match &mut config.funding {
        Some(policy) => {
            policy.order = order;
            false
        }
        None => {
            config.funding = Some(FundingPolicy {
                order,
                exclude_accounts: Vec::new(),
                from: Some(retirement),
            });
            true
        }
    };
    Ok((config, overlay))
}

pub(super) fn set_order(effect: &mut EventEffect, wanted: WithdrawalOrder) {
    match effect {
        EventEffect::Sweep {
            sources: WithdrawalSources::Strategy { order, .. },
            ..
        } => *order = wanted,
        EventEffect::Random {
            on_true, on_false, ..
        } => {
            set_order(on_true, wanted);
            if let Some(effect) = on_false {
                set_order(effect, wanted);
            }
        }
        _ => {}
    }
}

pub(super) fn has_fixed_sweep(effect: &EventEffect) -> bool {
    match effect {
        EventEffect::Sweep { sources, .. } => {
            !matches!(sources, WithdrawalSources::Strategy { .. })
        }
        EventEffect::Random {
            on_true, on_false, ..
        } => has_fixed_sweep(on_true) || on_false.as_deref().is_some_and(has_fixed_sweep),
        _ => false,
    }
}

/// The events whose sweeps no strategy reaches, in plan order.
pub(super) fn fixed_sweeps(compiled: &CompiledScenario) -> Vec<FixedSweep> {
    compiled
        .config
        .events
        .iter()
        .filter(|event| event.effects.iter().any(has_fixed_sweep))
        .filter_map(|event| {
            let event_id = compiled.id_map.event_db_id(event.event_id)?;
            Some(FixedSweep {
                event_id,
                name: compiled
                    .event_names
                    .get(&event_id)
                    .cloned()
                    .unwrap_or_default(),
            })
        })
        .collect()
}
