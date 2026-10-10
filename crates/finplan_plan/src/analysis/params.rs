//! Named scenario inputs exposed to analysis. Coordinates on the wire are dollars,
//! fractional rates, calendar years (including months / 12), or UTC epoch days.
//! The registry is varied, and so are the terms of a spending rule (spec 22);
//! no other event literal is ever rewritten.

use finplan_core::analysis::{
    EffectParam, EffectTarget, SpendingField, SweepParameter, SweepTarget,
};
use finplan_core::model::{
    CalendarAge, EventEffect, EventId, ParameterId, ParameterValue, SpendingInitial, SpendingRule,
};
use finplan_core::optimization::OptimizableParameter;
use jiff::civil::Date;

use crate::compile::CompiledScenario;
use crate::error::{PlanError, PlanResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Age,
    Amount,
    Rate,
    Date,
}

#[derive(Debug, Clone)]
pub struct PlanParameter {
    pub id: String,
    pub parameter_id: i64,
    pub name: String,
    pub kind: ParamKind,
    pub current: f64,
    pub min: f64,
    pub max: f64,
    pub dense_id: ParameterId,
    /// Set for a spending rule's term: which effect, and which term. The
    /// registry fields above then name the event (`parameter_id` is its
    /// database id) and `dense_id` means nothing.
    pub spending: Option<SpendingSource>,
}

/// A spending rule's term, as a sweep reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpendingSource {
    pub event: EventId,
    pub effect_index: usize,
    pub field: SpendingField,
}

fn epoch() -> Date {
    Date::constant(1970, 1, 1)
}

pub fn date_coordinate(date: Date) -> f64 {
    f64::from((date - epoch()).get_days())
}

impl ParamKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Age => "age",
            Self::Amount => "amount",
            Self::Rate => "rate",
            Self::Date => "date",
        }
    }
}

impl PlanParameter {
    pub fn typed_value(&self, value: f64) -> PlanResult<ParameterValue> {
        let invalid = || {
            PlanError::invalid(format!(
                "{} has an invalid {} bound",
                self.name,
                self.kind.as_str()
            ))
        };
        if !value.is_finite() {
            return Err(invalid());
        }
        Ok(match self.kind {
            ParamKind::Amount => ParameterValue::Money(value),
            ParamKind::Rate => ParameterValue::Rate(value),
            ParamKind::Age => {
                let months = (value * 12.0).round();
                if !(0.0..=3071.0).contains(&months) {
                    return Err(invalid());
                }
                ParameterValue::Age(CalendarAge::new(
                    (months as u16 / 12) as u8,
                    (months as u16 % 12) as u8,
                ))
            }
            ParamKind::Date => {
                if value < date_coordinate(Date::MIN) || value > date_coordinate(Date::MAX) {
                    return Err(invalid());
                }
                ParameterValue::Date(
                    epoch()
                        .checked_add(jiff::Span::new().days(value.round() as i64))
                        .map_err(|_| invalid())?,
                )
            }
        })
    }

    pub fn sweep(&self, min: f64, max: f64, steps: usize) -> PlanResult<SweepParameter> {
        if let Some(source) = self.spending {
            if !(min.is_finite() && max.is_finite()) || max < min || (steps > 1 && max == min) {
                return Err(PlanError::invalid(format!(
                    "{} needs distinct ordered bounds",
                    self.name
                )));
            }
            return Ok(SweepParameter {
                event_id: source.event,
                target: SweepTarget::Effect {
                    param: EffectParam::SpendingRule(source.field),
                    target: EffectTarget::Index(source.effect_index),
                },
                min_value: min,
                max_value: max,
                step_count: steps,
            });
        }
        let parameter = OptimizableParameter {
            parameter_id: self.dense_id,
            min_value: self.typed_value(min)?,
            max_value: self.typed_value(max)?,
        };
        let (lo, hi) = parameter.bounds();
        if hi < lo || (steps > 1 && hi == lo) {
            return Err(PlanError::invalid(format!(
                "{} needs distinct ordered bounds",
                self.name
            )));
        }
        let steps = if parameter.is_discrete() {
            steps.min((hi - lo) as usize + 1)
        } else {
            steps
        };
        Ok(SweepParameter::parameter(parameter, steps))
    }

    /// Translate internal calendar coordinates back to the API's units.
    pub fn display_coordinate(&self, sweep: &SweepParameter, coordinate: f64) -> f64 {
        match self.kind {
            ParamKind::Age => coordinate / 12.0,
            ParamKind::Date => match &sweep.target {
                finplan_core::analysis::SweepTarget::Parameter(p) => match p.min_value {
                    ParameterValue::Date(date) => date_coordinate(date) + coordinate,
                    _ => coordinate,
                },
                _ => coordinate,
            },
            _ => coordinate,
        }
    }

    pub fn perturbed(&self, fraction: f64) -> (f64, f64) {
        let span = match self.kind {
            ParamKind::Age => 5.0,
            ParamKind::Date => 365.0,
            ParamKind::Rate => (self.current * fraction).abs().max(0.01),
            ParamKind::Amount => (self.current * fraction).abs().max(100.0),
        };
        (
            (self.current - span).max(self.min),
            (self.current + span).min(self.max),
        )
    }
}

#[must_use]
pub fn parameters(compiled: &CompiledScenario) -> Vec<PlanParameter> {
    let mut out = Vec::new();
    for (&dense_id, value) in &compiled.config.parameters {
        let Some(parameter_id) = compiled.id_map.parameter_db_id(dense_id) else {
            continue;
        };
        let name = compiled
            .metadata
            .parameter_name(dense_id)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Parameter {parameter_id}"));
        let (kind, current) = match value {
            ParameterValue::Money(v) => (ParamKind::Amount, *v),
            ParameterValue::Rate(v) => (ParamKind::Rate, *v),
            ParameterValue::Age(age) => (
                ParamKind::Age,
                f64::from(age.years) + f64::from(age.months) / 12.0,
            ),
            ParameterValue::Date(date) => (ParamKind::Date, date_coordinate(*date)),
        };
        let (min, max) = default_range(kind, current);
        out.push(PlanParameter {
            id: format!("parameter:{parameter_id}"),
            parameter_id,
            name,
            kind,
            current,
            min,
            max,
            dense_id,
            spending: None,
        });
    }
    out.sort_by_key(|p| p.parameter_id);
    out
}

/// The terms of each spending rule in the plan (spec 22), as analysis
/// parameters: `spending:<event id>:<term>`, named for the event.
#[must_use]
pub fn spending_parameters(compiled: &CompiledScenario) -> Vec<PlanParameter> {
    let mut out = Vec::new();
    for event in &compiled.config.events {
        let Some(event_db_id) = compiled.id_map.event_db_id(event.event_id) else {
            continue;
        };
        let event_name = compiled
            .event_names
            .get(&event_db_id)
            .cloned()
            .unwrap_or_else(|| format!("Event {event_db_id}"));
        for (effect_index, effect) in event.effects.iter().enumerate() {
            let EventEffect::Spending { rule, .. } = effect else {
                continue;
            };
            for (field, label, kind, current, range) in spending_terms(rule) {
                out.push(PlanParameter {
                    id: format!("spending:{event_db_id}:{}", field_key(field)),
                    parameter_id: event_db_id,
                    name: format!("{event_name} · {label}"),
                    kind,
                    current,
                    min: range.0,
                    max: range.1,
                    dense_id: ParameterId(0),
                    spending: Some(SpendingSource {
                        event: event.event_id,
                        effect_index,
                        field,
                    }),
                });
            }
        }
    }
    out
}

fn field_key(field: SpendingField) -> &'static str {
    match field {
        SpendingField::Rate => "rate",
        SpendingField::InitialAmount => "initial-amount",
        SpendingField::Band => "band",
        SpendingField::Step => "step",
        SpendingField::Floor => "floor",
    }
}

type Term = (SpendingField, &'static str, ParamKind, f64, (f64, f64));

/// What a rule lets an analysis vary, with each term's value and the range
/// a sweep defaults to (inside what validation accepts).
fn spending_terms(rule: &SpendingRule) -> Vec<Term> {
    let rate = |r: f64| -> (f64, f64) { ((r * 0.5).max(0.005), (r * 1.5).min(0.2)) };
    let amount = |a: f64| -> (f64, f64) { ((a * 0.5).max(0.0), (a * 1.5).max(1000.0)) };
    let mut terms: Vec<Term> = Vec::new();
    match rule {
        SpendingRule::Guardrails {
            initial,
            band,
            step,
            floor,
            ..
        } => {
            match initial {
                SpendingInitial::Amount(a) => terms.push((
                    SpendingField::InitialAmount,
                    "starting amount",
                    ParamKind::Amount,
                    *a,
                    amount(*a),
                )),
                SpendingInitial::Rate(r) => {
                    terms.push((
                        SpendingField::Rate,
                        "starting rate",
                        ParamKind::Rate,
                        *r,
                        rate(*r),
                    ));
                }
            }
            terms.push((
                SpendingField::Band,
                "band",
                ParamKind::Rate,
                *band,
                (0.05, 0.5),
            ));
            terms.push((
                SpendingField::Step,
                "step",
                ParamKind::Rate,
                *step,
                (0.02, 0.3),
            ));
            let f = floor.unwrap_or(0.0);
            terms.push((
                SpendingField::Floor,
                "floor",
                ParamKind::Amount,
                f,
                amount(f),
            ));
        }
        SpendingRule::PercentOfPortfolio { rate: r, floor, .. } => {
            terms.push((SpendingField::Rate, "rate", ParamKind::Rate, *r, rate(*r)));
            let f = floor.unwrap_or(0.0);
            terms.push((
                SpendingField::Floor,
                "floor",
                ParamKind::Amount,
                f,
                amount(f),
            ));
        }
        SpendingRule::CappedChange { rate: r, .. } => {
            terms.push((SpendingField::Rate, "rate", ParamKind::Rate, *r, rate(*r)));
        }
        SpendingRule::Vpw { .. } => {}
    }
    terms
}

/// Everything an analysis can vary: the named parameters, then each spending
/// rule's terms.
#[must_use]
pub fn analysis_parameters(compiled: &CompiledScenario) -> Vec<PlanParameter> {
    let mut out = parameters(compiled);
    out.extend(spending_parameters(compiled));
    out
}

fn default_range(kind: ParamKind, current: f64) -> (f64, f64) {
    match kind {
        ParamKind::Age => (
            (current - 10.0).max(0.0),
            (current + 10.0).min(3071.0 / 12.0),
        ),
        ParamKind::Date => (
            (current - 1826.0).max(date_coordinate(Date::MIN)),
            (current + 1826.0).min(date_coordinate(Date::MAX)),
        ),
        ParamKind::Rate => {
            let span = (current.abs() * 0.5).max(0.01);
            (current - span, current + span)
        }
        ParamKind::Amount => {
            let span = (current.abs() * 0.5).max(1000.0);
            (current - span, current + span)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parameter(kind: ParamKind, current: f64) -> PlanParameter {
        let (min, max) = default_range(kind, current);
        PlanParameter {
            id: "parameter:9".into(),
            parameter_id: 9,
            name: "Input".into(),
            kind,
            current,
            min,
            max,
            dense_id: ParameterId(2),
            spending: None,
        }
    }
    #[test]
    fn calendar_sweeps_round_trip_and_deduplicate_adjacent_values() {
        let age = parameter(ParamKind::Age, 40.5);
        let sweep = age.sweep(40.0, 40.0 + 1.0 / 12.0, 12).unwrap();
        assert_eq!(sweep.sweep_values(), vec![480.0, 481.0]);
        assert_eq!(age.display_coordinate(&sweep, 481.0), 40.0 + 1.0 / 12.0);
        let current = date_coordinate(Date::constant(2035, 1, 1));
        let date = parameter(ParamKind::Date, current);
        let sweep = date.sweep(current, current + 2.0, 12).unwrap();
        assert_eq!(sweep.sweep_values(), vec![0.0, 1.0, 2.0]);
        assert_eq!(date.display_coordinate(&sweep, 2.0), current + 2.0);
        assert!(age.sweep(-1.0, 30.0, 2).is_err());
        assert!(date.sweep(f64::INFINITY, current, 2).is_err());
    }
    #[test]
    fn rates_remain_fractional_and_are_not_clamped_to_one() {
        let rate = parameter(ParamKind::Rate, 1.5);
        let sweep = rate.sweep(-0.1, 2.0, 3).unwrap();
        assert_eq!(sweep.min_value, -0.1);
        assert_eq!(sweep.max_value, 2.0);
    }
}
