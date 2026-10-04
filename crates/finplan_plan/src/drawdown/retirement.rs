//! When retirement starts. The engine has no notion of it, so the date comes
//! from, in order: the request, the plan's retirement-age parameter, the last
//! year earned income landed, or the plan's first year.

use finplan_core::config::SimulationConfig;
use finplan_core::model::{CashFlowKind, SimulationResult, StateEvent};
use finplan_core::simulation::simulate;
use jiff::Span;
use jiff::civil::Date;

use super::{RetirementInfo, RetirementSource};
use crate::analysis::params::parameters;
use crate::compile::CompiledScenario;
use crate::error::{PlanError, PlanResult};
use crate::what_if::retirement_age_parameter;

pub(super) struct Retirement {
    pub year: i64,
    /// When an overlay policy starts acting.
    pub date: Date,
    pub age: Option<f64>,
    pub source: RetirementSource,
}

impl Retirement {
    pub fn info(&self) -> RetirementInfo {
        RetirementInfo {
            year: self.year,
            age: self.age,
            source: self.source,
        }
    }
}

/// Income from these is not earned: it carries on after retirement, so it says
/// nothing about when work stopped.
const BENEFIT_WORDS: [&str; 8] = [
    "social security",
    "pension",
    "annuity",
    "rental",
    "dividend",
    "interest",
    "inherit",
    "royalt",
];

fn is_benefit(name: &str) -> bool {
    let name = name.to_lowercase();
    BENEFIT_WORDS.iter().any(|word| name.contains(word))
        || name
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "ss")
}

/// The first Jan 1 of `year`, or the nearest date the calendar has.
fn january(year: i64) -> Date {
    i16::try_from(year)
        .ok()
        .and_then(|y| Date::new(y, 1, 1).ok())
        .unwrap_or(Date::constant(2000, 1, 1))
}

fn age_at(birth: Option<Date>, date: Date) -> Option<f64> {
    let birth = birth?;
    Some(f64::from((date - birth).get_days()) / 365.25)
}

/// Resolve the retirement date. `base` holds a ledger-bearing run of the plan
/// as it is, filled in here when (and only when) the income fallback needs it.
pub(super) fn resolve(
    compiled: &CompiledScenario,
    requested: Option<i64>,
    seed: u64,
    base: &mut Option<SimulationResult>,
) -> PlanResult<Retirement> {
    let config = &compiled.config;
    let birth = config.birth_date;

    if let Some(year) = requested {
        let date = january(year);
        return Ok(Retirement {
            year,
            date,
            age: age_at(birth, date),
            source: RetirementSource::Request,
        });
    }

    if let (Some(birth), Some(param)) = (
        birth,
        retirement_age_parameter(&parameters(compiled)).cloned(),
    ) {
        let months = (param.current * 12.0).round() as i32;
        if let Ok(date) = birth.checked_add(Span::new().months(months)) {
            return Ok(Retirement {
                year: i64::from(date.year()),
                date,
                age: Some(param.current),
                source: RetirementSource::Parameter,
            });
        }
    }

    let result = ledger_run(config, seed, base)?;
    let end_year = result
        .wealth_snapshots
        .last()
        .map_or(0, |s| i64::from(s.date.year()));
    let last_earned = result
        .ledger
        .iter()
        .filter_map(|entry| {
            let StateEvent::CashCredit {
                kind: CashFlowKind::Income,
                ..
            } = entry.event
            else {
                return None;
            };
            let db = compiled.id_map.event_db_id(entry.source_event?)?;
            let name = compiled.event_names.get(&db)?;
            (!is_benefit(name)).then_some(i64::from(entry.date.year()))
        })
        .max();

    let (year, source) = match last_earned {
        Some(last) if last < end_year => (last + 1, RetirementSource::Income),
        _ => (
            result
                .wealth_snapshots
                .first()
                .map_or(0, |s| i64::from(s.date.year())),
            RetirementSource::Start,
        ),
    };
    let date = january(year);
    Ok(Retirement {
        year,
        date,
        age: age_at(birth, date),
        source,
    })
}

/// A run of `config` as it is with the ledger on, simulated once and kept.
pub(super) fn ledger_run<'a>(
    config: &SimulationConfig,
    seed: u64,
    slot: &'a mut Option<SimulationResult>,
) -> PlanResult<&'a SimulationResult> {
    if slot.is_none() {
        let mut config = config.clone();
        config.collect_ledger = true;
        *slot = Some(simulate(&config, seed).map_err(|e| PlanError::internal(e.to_string()))?);
    }
    slot.as_ref()
        .ok_or_else(|| PlanError::internal("run missing"))
}
