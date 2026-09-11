//! What a plan has that could have been a different number.
//!
//! Read off the compiled scenario rather than the stored rows: the engine is
//! what the sweep will actually modify, so the list of things it can modify has
//! to come from there. Anything the engine cannot vary — a date trigger, a
//! balance-referencing amount — is simply not offered, which is a better answer
//! than offering it and failing at run time.
//!
//! Each parameter carries a suggested range as well as its identity. A sweep
//! axis needs bounds before it can draw anything, and the plan's own value is
//! the only honest place to centre them on.

use finplan_core::analysis::{
    EffectParam, EffectTarget, SweepParameter, SweepTarget, TriggerParam,
};
use finplan_core::model::{EventEffect, EventId, EventTrigger, TransferAmount};

use crate::compile::CompiledScenario;

/// The kind of number a parameter is, which is all the client needs to format
/// it and to pick a sensible step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// An age in whole years.
    Age,
    /// A dollar amount, per occurrence of the event.
    Amount,
}

/// One varyable number in the plan, with the range a sweep should default to.
#[derive(Debug, Clone)]
pub struct PlanParameter {
    /// Stable identity, unique within a scenario: `event:<db id>:<slot>`.
    /// Round-trips through the URL and the request body, so it is a string
    /// rather than a tuple the client would have to reassemble.
    pub id: String,
    /// The event this belongs to, by database id.
    pub event_id: i64,
    /// The event's own name, for display.
    pub event_name: String,
    /// What about the event varies — "retirement age", "monthly amount".
    pub role: &'static str,
    pub kind: ParamKind,
    /// The value the plan is configured with today.
    pub current: f64,
    /// A defensible default range: wide enough to bracket an answer, narrow
    /// enough that six steps still say something.
    pub min: f64,
    pub max: f64,
    /// How this parameter is applied, ready to hand to the engine.
    pub target: SweepTarget,
    /// The engine-side id, which the sweep config wants rather than the db one.
    pub dense_event_id: EventId,
}

impl PlanParameter {
    /// A sweep axis over this parameter's default range.
    #[must_use]
    pub fn sweep(&self, min: f64, max: f64, steps: usize) -> SweepParameter {
        SweepParameter {
            event_id: self.dense_event_id,
            target: self.target.clone(),
            min_value: min,
            max_value: max,
            step_count: steps,
        }
    }

    /// The range a sensitivity ranking probes: the plan's value ±`fraction`,
    /// clamped to the parameter's own bounds so an age stays an age.
    #[must_use]
    pub fn perturbed(&self, fraction: f64) -> (f64, f64) {
        let span = (self.current * fraction).abs();
        let (mut lo, mut hi) = (self.current - span, self.current + span);
        if self.kind == ParamKind::Age {
            // Ages are whole years, and a ±20% band on 65 is nobody's question.
            lo = (self.current - 5.0).round();
            hi = (self.current + 5.0).round();
        }
        (
            lo.max(self.min).min(self.max),
            hi.min(self.max).max(self.min),
        )
    }
}

/// Every parameter of the compiled plan a sweep or solve could vary.
///
/// Order follows the plan's own event order, so the list reads the way the Plan
/// tab does.
#[must_use]
pub fn parameters(compiled: &CompiledScenario) -> Vec<PlanParameter> {
    let mut out = Vec::new();

    for event in &compiled.config.events {
        let Some(db_id) = compiled.id_map.event_db_id(event.event_id) else {
            continue;
        };
        let name = compiled
            .event_names
            .get(&db_id)
            .cloned()
            .unwrap_or_else(|| format!("event {db_id}"));

        for (slot, role, kind, current, target) in trigger_params(&event.trigger) {
            let (min, max) = default_range(kind, current);
            if max <= min {
                continue;
            }
            out.push(PlanParameter {
                id: format!("event:{db_id}:{slot}"),
                event_id: db_id,
                event_name: name.clone(),
                role,
                kind,
                current,
                min,
                max,
                target,
                dense_event_id: event.event_id,
            });
        }

        // Only the first amount-carrying effect is offered. An event with two
        // of them is rare, and "which of this event's amounts" is a question
        // the axis chips have no room to ask.
        if let Some(current) = event.effects.iter().find_map(fixed_amount) {
            let (min, max) = default_range(ParamKind::Amount, current);
            if max > min {
                out.push(PlanParameter {
                    id: format!("event:{db_id}:amount"),
                    event_id: db_id,
                    event_name: name,
                    role: "amount",
                    kind: ParamKind::Amount,
                    current,
                    min,
                    max,
                    target: SweepTarget::Effect {
                        param: EffectParam::Value,
                        target: EffectTarget::FirstEligible,
                    },
                    dense_event_id: event.event_id,
                });
            }
        }
    }

    out
}

/// The ages a trigger exposes: its own, or the ones bounding a schedule.
fn trigger_params(
    trigger: &EventTrigger,
) -> Vec<(&'static str, &'static str, ParamKind, f64, SweepTarget)> {
    match trigger {
        EventTrigger::Age { years, .. } => vec![(
            "age",
            "age",
            ParamKind::Age,
            f64::from(*years),
            SweepTarget::Trigger(TriggerParam::Age),
        )],
        EventTrigger::Repeating {
            start_condition,
            end_condition,
            ..
        } => {
            let mut out = Vec::new();
            if let Some(EventTrigger::Age { years, .. }) = start_condition.as_deref() {
                out.push((
                    "start-age",
                    "starts at age",
                    ParamKind::Age,
                    f64::from(*years),
                    SweepTarget::Trigger(TriggerParam::RepeatingStart(Box::new(TriggerParam::Age))),
                ));
            }
            if let Some(EventTrigger::Age { years, .. }) = end_condition.as_deref() {
                out.push((
                    "end-age",
                    "ends at age",
                    ParamKind::Age,
                    f64::from(*years),
                    SweepTarget::Trigger(TriggerParam::RepeatingEnd(Box::new(TriggerParam::Age))),
                ));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The fixed dollar figure behind an effect's amount, if it has one.
///
/// `InflationAdjusted` wraps a real-dollar figure, which is the number the plan
/// was written in and so the one to vary; everything else — a balance, a
/// computed transfer — has no single number to move.
fn fixed_amount(effect: &EventEffect) -> Option<f64> {
    let amount = match effect {
        EventEffect::Income { amount, .. }
        | EventEffect::Expense { amount, .. }
        | EventEffect::AssetPurchase { amount, .. }
        | EventEffect::AssetSale { amount, .. }
        | EventEffect::Sweep { amount, .. }
        | EventEffect::AdjustBalance { amount, .. }
        | EventEffect::CashTransfer { amount, .. } => amount,
        _ => return None,
    };
    match amount {
        TransferAmount::Fixed(v) => Some(*v),
        TransferAmount::InflationAdjusted(inner) => match inner.as_ref() {
            TransferAmount::Fixed(v) => Some(*v),
            _ => None,
        },
        _ => None,
    }
}

/// The range a fresh axis opens on, centred on the plan's own value.
///
/// Ages get a decade to move in and stay inside a working life; amounts get
/// half to double, which brackets both "could I spend more" and "what if this
/// has to halve" without ever proposing a negative dollar.
fn default_range(kind: ParamKind, current: f64) -> (f64, f64) {
    match kind {
        ParamKind::Age => (
            (current - 10.0).max(18.0).round(),
            (current + 10.0).min(100.0).round(),
        ),
        ParamKind::Amount => {
            // A zero amount has no proportional band to open around it, so it
            // gets an absolute one rather than the degenerate (0, 0).
            if current.abs() < 1.0 {
                (0.0, 10_000.0)
            } else if current > 0.0 {
                (round_step(current * 0.5), round_step(current * 2.0))
            } else {
                (round_step(current * 2.0), round_step(current * 0.5))
            }
        }
    }
}

/// Round a bound to something a person would have typed.
fn round_step(value: f64) -> f64 {
    let magnitude = value.abs();
    let step = if magnitude >= 100_000.0 {
        10_000.0
    } else if magnitude >= 10_000.0 {
        1_000.0
    } else if magnitude >= 1_000.0 {
        100.0
    } else {
        10.0
    };
    (value / step).round() * step
}
