//! What-if: an ordered stack of override layers over a plan.
//!
//! The stack itself is a client document (a host stores it whole per plan,
//! the same bargain a sweep's layout makes). Running it is an analysis
//! (`CreateAnalysis::WhatIf`, or a [`QuickWhatIf`] answered in one call), and
//! applying it writes the layers into the plan for real: parameter values are
//! set, and shocks and one-offs become fires-once age events ([`apply`]).

use finplan_core::config::SimulationConfig;
use finplan_core::model::{
    AmountMode, Event as CoreEvent, EventEffect, EventId, EventTrigger, IncomeType, ParameterValue,
    TransferAmount,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::analysis::params::{ParamKind, PlanParameter, parameters};
use crate::compile::{self, CompiledScenario};
use crate::edit;
use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;
use crate::specs::events::EventBody;
use crate::specs::{AmountSpec, EffectSpec, TriggerSpec};

/// Most layers one analysis or apply may carry.
pub const MAX_LAYERS: usize = 8;
/// Most rows a stored stack may hold, enabled or not.
pub const MAX_ENTRIES: usize = 16;
/// Longest client-generated row key.
pub const MAX_ENTRY_ID: usize = 64;

/// Most simulations a quick what-if may spend. It holds a request open while
/// it runs, so it stays small; anything bigger is a job.
pub const MAX_QUICK_ITERATIONS: usize = 500;
/// What a quick what-if spends when the request names nothing.
pub const DEFAULT_QUICK_ITERATIONS: usize = 200;

/// A small what-if answered in the response itself.
#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct QuickWhatIf {
    /// The enabled layers only, in order. At most eight.
    pub layers: Vec<WhatIfLayer>,
    /// Simulations for the whole stack, split across its steps; at most 500.
    #[serde(default)]
    pub iterations: Option<usize>,
}

/// Simulations a quick what-if runs: what it asked for (or the default),
/// capped at [`MAX_QUICK_ITERATIONS`] and at the caller's own cap.
#[must_use]
pub fn quick_iterations(requested: Option<usize>, iteration_cap: usize) -> usize {
    requested
        .unwrap_or(DEFAULT_QUICK_ITERATIONS)
        .min(MAX_QUICK_ITERATIONS.min(iteration_cap))
}

/// One override on top of the plan.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[ts(export)]
pub enum WhatIfLayer {
    /// value in the analysis units of AnalysisParameter: age = years, amount = dollars,
    /// rate = fraction, date = UTC epoch days.
    Parameter {
        parameter_id: i64,
        value: f64,
    },
    MarketShock {
        age: u8,
        drop: f64,
    },
    OneOff {
        age: u8,
        amount: f64,
        account_id: Option<i64>,
    },
}

/// One row of the stored stack. `id` is a client-generated stable key.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WhatIfEntry {
    pub id: String,
    pub enabled: bool,
    pub layer: WhatIfLayer,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WhatIfStack {
    pub entries: Vec<WhatIfEntry>,
}

impl WhatIfStack {
    /// Whether a host may store this stack: not too long, every row keyed,
    /// every layer well formed.
    pub fn validate(&self) -> PlanResult<()> {
        if self.entries.len() > MAX_ENTRIES {
            return Err(PlanError::invalid(format!(
                "a what-if stack holds at most {MAX_ENTRIES} layers"
            )));
        }
        for entry in &self.entries {
            if entry.id.is_empty() || entry.id.len() > MAX_ENTRY_ID {
                return Err(PlanError::invalid(format!(
                    "each layer needs an id of 1–{MAX_ENTRY_ID} characters"
                )));
            }
            check_shape(&entry.layer)?;
        }
        Ok(())
    }

    /// The enabled layers, in order: what an analysis or an apply is sent.
    #[must_use]
    pub fn enabled_layers(&self) -> Vec<WhatIfLayer> {
        self.entries
            .iter()
            .filter(|e| e.enabled)
            .map(|e| e.layer.clone())
            .collect()
    }
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ApplyWhatIf {
    /// Enabled layers, in order.
    pub layers: Vec<WhatIfLayer>,
    /// None = write into this scenario. Some(name) = duplicate first, then write into the copy
    /// (parameter/account ids mapped onto the copy's rows).
    #[serde(default)]
    pub new_scenario_name: Option<String>,
}

/// Checks that need nothing but the layer itself.
pub fn check_shape(layer: &WhatIfLayer) -> PlanResult<()> {
    match layer {
        WhatIfLayer::Parameter { value, .. } if !value.is_finite() => {
            Err(PlanError::invalid("a parameter override must be finite"))
        }
        WhatIfLayer::MarketShock { drop, .. } if !(*drop > 0.0 && *drop < 1.0) => Err(
            PlanError::invalid("a market shock's drop is a fraction between 0 and 1"),
        ),
        WhatIfLayer::OneOff { amount, .. } if !amount.is_finite() || *amount == 0.0 => {
            Err(PlanError::invalid("a one-off needs a non-zero amount"))
        }
        _ => Ok(()),
    }
}

// ── lowering layers onto a plan ─────────────────────────────────────────────

/// A layer checked against the plan it is applied to, with every id resolved.
pub enum Resolved<'a> {
    Parameter {
        param: &'a PlanParameter,
        value: ParameterValue,
    },
    MarketShock {
        age: u8,
        drop: f64,
    },
    OneOff {
        age: u8,
        amount: f64,
        /// Plan id of the account the money moves through.
        account_id: i64,
    },
}

impl Resolved<'_> {
    /// The fires-once event an applied shock or one-off becomes, named
    /// uniquely among `names` (which it adds to). `account` maps the plan's
    /// account id onto the one the event should point at, for a host writing
    /// into a copy. `None` for a parameter layer, which writes a value.
    pub fn event_body(
        &self,
        names: &mut Vec<String>,
        account: impl FnOnce(i64) -> PlanResult<i64>,
    ) -> PlanResult<Option<EventBody>> {
        let (name, age, effect) = match self {
            Resolved::Parameter { .. } => return Ok(None),
            Resolved::MarketShock { age, drop } => (
                unique_name(
                    names,
                    format!("Market shock −{:.0}% at {age}", drop * 100.0),
                ),
                *age,
                EffectSpec::MarketShock { drop: *drop },
            ),
            Resolved::OneOff {
                age,
                amount,
                account_id,
            } => {
                let account_id = account(*account_id)?;
                let (label, effect) = if *amount < 0.0 {
                    (
                        "One-off cost",
                        EffectSpec::Expense {
                            from_account_id: account_id,
                            amount: AmountSpec::Fixed { value: -amount },
                        },
                    )
                } else {
                    (
                        "Windfall",
                        EffectSpec::Income {
                            to_account_id: account_id,
                            amount: AmountSpec::Fixed { value: *amount },
                            amount_mode: crate::specs::AmountMode::Gross,
                            income_type: crate::specs::IncomeType::TaxFree,
                        },
                    )
                };
                (
                    unique_name(names, format!("{label} {} at {age}", money(amount.abs()))),
                    *age,
                    effect,
                )
            }
        };
        Ok(Some(EventBody {
            name,
            description: Some("Applied from a what-if".to_string()),
            fires_once: true,
            enabled: true,
            sort_order: None,
            trigger: TriggerSpec::Age {
                years: age,
                months: None,
            },
            effects: vec![effect],
        }))
    }
}

/// Check `layers` against the plan and resolve their ids.
pub fn resolve<'a>(
    graph: &ScenarioGraph,
    available: &'a [PlanParameter],
    layers: &[WhatIfLayer],
) -> PlanResult<Vec<Resolved<'a>>> {
    if layers.len() > MAX_LAYERS {
        return Err(PlanError::invalid(format!(
            "a what-if applies at most {MAX_LAYERS} layers"
        )));
    }
    let has_birth_date = graph.scenario.birth_date.is_some();
    let default_account = || {
        let mut banks: Vec<_> = graph
            .accounts
            .iter()
            .filter(|a| a.flavor == "Bank")
            .collect();
        banks.sort_by_key(|a| (a.sort_order, a.id));
        banks.first().map(|a| a.id)
    };

    layers
        .iter()
        .map(|layer| {
            check_shape(layer)?;
            let needs_birth_date = || {
                if has_birth_date {
                    Ok(())
                } else {
                    Err(PlanError::invalid(
                        "age-based what-if layers need the scenario's birth date; set it on the plan first",
                    ))
                }
            };
            Ok(match layer {
                WhatIfLayer::Parameter {
                    parameter_id,
                    value,
                } => {
                    let param = available
                        .iter()
                        .find(|p| p.parameter_id == *parameter_id)
                        .ok_or_else(|| {
                            PlanError::invalid(format!(
                                "parameter {parameter_id} is not in this plan"
                            ))
                        })?;
                    Resolved::Parameter {
                        param,
                        value: param.typed_value(*value)?,
                    }
                }
                WhatIfLayer::MarketShock { age, drop } => {
                    needs_birth_date()?;
                    Resolved::MarketShock {
                        age: *age,
                        drop: *drop,
                    }
                }
                WhatIfLayer::OneOff {
                    age,
                    amount,
                    account_id,
                } => {
                    needs_birth_date()?;
                    let account_id = match account_id {
                        Some(id) => {
                            let account =
                                graph.accounts.iter().find(|a| a.id == *id).ok_or_else(|| {
                                    PlanError::invalid(format!("account {id} is not in this plan"))
                                })?;
                            if account.flavor != "Bank" && account.flavor != "Investment" {
                                return Err(PlanError::invalid(
                                    "a one-off moves cash, so it needs a cash or investment account",
                                ));
                            }
                            *id
                        }
                        None => default_account().ok_or_else(|| {
                            PlanError::invalid(
                                "this plan has no cash account for a one-off; name an account",
                            )
                        })?,
                    };
                    Resolved::OneOff {
                        age: *age,
                        amount: *amount,
                        account_id,
                    }
                }
            })
        })
        .collect()
}

/// The plan's retirement age: its Age parameter whose name mentions "retire".
/// The engine has no notion of retirement, so a name is all there is to go on.
pub fn retirement_age_parameter(available: &[PlanParameter]) -> Option<&PlanParameter> {
    available
        .iter()
        .find(|p| p.kind == ParamKind::Age && p.name.to_lowercase().contains("retire"))
}

/// What a what-if analysis runs: the plan and each cumulative step.
pub struct Lowered {
    pub steps: Vec<SimulationConfig>,
    pub plan_retirement_age: Option<f64>,
    pub what_if_retirement_age: Option<f64>,
}

/// Lower `layers` onto the compiled plan, one cumulative config per step.
pub fn lower(
    graph: &ScenarioGraph,
    compiled: &CompiledScenario,
    layers: &[WhatIfLayer],
) -> PlanResult<Lowered> {
    let available = parameters(compiled);
    let resolved = resolve(graph, &available, layers)?;

    let retirement = retirement_age_parameter(&available);
    let plan_retirement_age = retirement.map(|p| p.current);
    let mut what_if_retirement_age = plan_retirement_age;

    let mut next_event = compiled
        .config
        .events
        .iter()
        .map(|e| e.event_id.0)
        .max()
        .map_or(0, |id| id + 1);
    let mut synthetic_event = |effect: EventEffect, age: u8| -> PlanResult<CoreEvent> {
        let event_id = EventId(next_event);
        next_event = next_event
            .checked_add(1)
            .ok_or_else(|| PlanError::unprocessable("this plan has too many events"))?;
        Ok(CoreEvent {
            event_id,
            trigger: EventTrigger::Age {
                years: age,
                months: None,
            },
            effects: vec![effect],
            once: true,
        })
    };

    let mut current = compiled.config.clone();
    let mut steps = Vec::with_capacity(resolved.len() + 1);
    steps.push(current.clone());
    for layer in &resolved {
        match layer {
            Resolved::Parameter { param, value } => {
                current.parameters.insert(param.dense_id, *value);
                if retirement.is_some_and(|r| r.parameter_id == param.parameter_id)
                    && let ParameterValue::Age(age) = value
                {
                    what_if_retirement_age =
                        Some(f64::from(age.years) + f64::from(age.months) / 12.0);
                }
            }
            Resolved::MarketShock { age, drop } => {
                let event = synthetic_event(EventEffect::MarketShock { drop: *drop }, *age)?;
                current.events.push(event);
            }
            Resolved::OneOff {
                age,
                amount,
                account_id,
            } => {
                let account = compiled.id_map.account(*account_id)?;
                let effect = if *amount < 0.0 {
                    EventEffect::Expense {
                        from: account,
                        amount: TransferAmount::fixed(-amount),
                    }
                } else {
                    EventEffect::Income {
                        to: account,
                        amount: TransferAmount::fixed(*amount),
                        amount_mode: AmountMode::Gross,
                        income_type: IncomeType::TaxFree,
                    }
                };
                current.events.push(synthetic_event(effect, *age)?);
            }
        }
        steps.push(current.clone());
    }

    Ok(Lowered {
        steps,
        plan_retirement_age,
        what_if_retirement_age,
    })
}

// ── apply ───────────────────────────────────────────────────────────────────

/// Write the layers into the plan, as `POST …/what-if/apply` does for the
/// plan itself: parameter values are set (a later layer on the same parameter
/// wins), and each shock or one-off is appended as a fires-once age event.
/// Atomic: on an error the graph is as it was.
///
/// Copying first (`ApplyWhatIf::new_scenario_name`) is a library operation,
/// not an edit: duplicate the plan, then apply to the copy.
pub fn apply(graph: &mut ScenarioGraph, layers: &[WhatIfLayer]) -> PlanResult<()> {
    let compiled = compile::compile(graph)?;
    let available = parameters(&compiled);
    let resolved = resolve(graph, &available, layers)?;

    let mut staged = graph.clone();
    let mut names: Vec<String> = staged.events.iter().map(|e| e.name.clone()).collect();
    for layer in &resolved {
        match layer {
            Resolved::Parameter { param, value } => {
                let row = staged
                    .parameters
                    .iter_mut()
                    .find(|p| p.id == param.parameter_id)
                    .ok_or(PlanError::NotFound("parameter"))?;
                let (kind, number, date, years, months) = value_columns(value);
                row.kind = kind.to_string();
                row.number_value = number;
                row.date_value = date;
                row.age_years = years;
                row.age_months = months;
            }
            other => {
                if let Some(body) = other.event_body(&mut names, Ok)? {
                    edit::create_event(&mut staged, &body)?;
                }
            }
        }
    }
    *graph = staged;
    Ok(())
}

/// A typed parameter value as `named_parameters` columns:
/// `(kind, number, date, age years, age months)`.
#[must_use]
pub fn value_columns(
    value: &ParameterValue,
) -> (
    &'static str,
    Option<f64>,
    Option<String>,
    Option<i64>,
    Option<i64>,
) {
    match value {
        ParameterValue::Money(v) => ("Money", Some(*v), None, None, None),
        ParameterValue::Rate(v) => ("Rate", Some(*v), None, None, None),
        ParameterValue::Date(d) => ("Date", None, Some(d.to_string()), None, None),
        ParameterValue::Age(age) => (
            "Age",
            None,
            None,
            Some(i64::from(age.years)),
            Some(i64::from(age.months)),
        ),
    }
}

/// `base`, or `base (2)`, `base (3)`… — event names are unique per scenario.
pub fn unique_name(taken: &mut Vec<String>, base: String) -> String {
    let mut name = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t == &name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    taken.push(name.clone());
    name
}

/// `$40k`, `$1.2M`, `$500`.
#[must_use]
pub fn money(value: f64) -> String {
    if value >= 1_000_000.0 {
        let m = value / 1_000_000.0;
        if (m - m.round()).abs() < 0.05 {
            format!("${m:.0}M")
        } else {
            format!("${m:.1}M")
        }
    } else if value >= 1_000.0 {
        format!("${:.0}k", value / 1_000.0)
    } else {
        format!("${value:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_round_trip_with_kebab_case_tags() {
        let stack: WhatIfStack = serde_json::from_value(serde_json::json!({
            "entries": [
                {"id": "a", "enabled": true,
                 "layer": {"kind": "parameter", "parameter_id": 3, "value": 62}},
                {"id": "b", "enabled": false,
                 "layer": {"kind": "market-shock", "age": 67, "drop": 0.3}},
                {"id": "c", "enabled": true,
                 "layer": {"kind": "one-off", "age": 58, "amount": -40000, "account_id": null}}
            ]
        }))
        .unwrap();
        assert_eq!(stack.entries.len(), 3);
        let back = serde_json::to_value(&stack).unwrap();
        assert_eq!(back["entries"][1]["layer"]["kind"], "market-shock");
        assert_eq!(back["entries"][2]["layer"]["kind"], "one-off");
    }

    #[test]
    fn names_are_short_and_unique() {
        assert_eq!(money(40_000.0), "$40k");
        assert_eq!(money(1_250_000.0), "$1.2M");
        assert_eq!(money(2_000_000.0), "$2M");
        let mut taken = vec!["Windfall $40k at 58".to_string()];
        assert_eq!(
            unique_name(&mut taken, "Windfall $40k at 58".into()),
            "Windfall $40k at 58 (2)"
        );
    }

    #[test]
    fn quick_iterations_default_and_clamp() {
        assert_eq!(quick_iterations(None, 10_000), DEFAULT_QUICK_ITERATIONS);
        assert_eq!(quick_iterations(Some(900), 10_000), MAX_QUICK_ITERATIONS);
        assert_eq!(quick_iterations(Some(900), 120), 120);
    }
}
