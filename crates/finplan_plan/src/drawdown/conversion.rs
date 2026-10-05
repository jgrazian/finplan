//! The conversion toggle (spec 21, part 3): what it can do on a plan, and the
//! plan with the `RothConversions` template added when it has no conversions
//! of its own.

use finplan_core::config::SimulationConfig;
use finplan_core::model::{EventEffect, TransferAmount};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::retirement::Retirement;
use super::{ConversionChoice, StrategyChoice};
use crate::compile::{self, CompiledScenario};
use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;
use crate::suggest::{self, Created};
use crate::templates::{
    RMD_AGE, RothConversionsParams, RowRef, Template, When, expand_template_in, holdings,
    largest_taxable,
};

/// What the conversion toggle can do on the plan.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct DrawdownConversions {
    /// Why the toggle is disabled: no tax-deferred account to convert from,
    /// or no tax-free one to convert into. None when it works.
    pub unavailable: Option<String>,
    /// The plan's enabled events that convert. A rate retargets these.
    pub events: Vec<i64>,
    /// When the plan has none: the yearly conversion a rate adds, which is
    /// also what Apply to plan writes.
    pub overlay: Option<ConversionOverlay>,
}

/// The `RothConversions` template as Drawdown fills it in: from the largest
/// tax-deferred account into the largest tax-free one, tax paid from the
/// largest taxable account (else the largest bank), from the retirement year
/// until the year before RMDs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ConversionOverlay {
    pub from_account_id: i64,
    pub to_account_id: i64,
    /// None withholds the tax from the conversion.
    pub pay_tax_from_account_id: Option<i64>,
    /// The first conversion is on Dec 30 of this year.
    pub start_year: i64,
    /// Conversions end at this age, when RMDs begin: the last is the year
    /// before.
    pub until_age: i64,
}

impl ConversionOverlay {
    fn template(&self, ceiling_rate: f64) -> Template {
        Template::RothConversions(RothConversionsParams {
            name: None,
            from_account_id: RowRef::Id(self.from_account_id),
            to_account_id: RowRef::Id(self.to_account_id),
            ceiling_rate,
            start: Some(When::Date {
                on_date: format!("{:04}-01-01", self.start_year),
            }),
            end: Some(When::age(u8::try_from(self.until_age).unwrap_or(RMD_AGE))),
            pay_tax_from_account_id: self.pay_tax_from_account_id.map(RowRef::Id),
        })
    }
}

/// What an investment account holds at the plan's start: holdings at their
/// starting prices, plus cash.
fn start_value(graph: &ScenarioGraph, id: i64) -> f64 {
    let cash = graph.investment.get(&id).map_or(0.0, |i| i.cash_value);
    cash + holdings(graph, id).iter().map(|(_, v)| v).sum::<f64>()
}

/// The investment account with `status` worth the most at the start (the
/// first listed on a tie), even when every one is empty.
fn largest(graph: &ScenarioGraph, status: &str) -> Option<i64> {
    let mut accounts: Vec<_> = graph
        .accounts
        .iter()
        .filter(|a| {
            graph
                .investment
                .get(&a.id)
                .is_some_and(|i| i.tax_status == status)
        })
        .map(|a| (a.sort_order, a.id, start_value(graph, a.id)))
        .collect();
    accounts.sort_by_key(|(order, id, _)| (*order, *id));
    accounts
        .into_iter()
        .rev()
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(_, id, _)| id)
}

/// The bank with the most cash at the start (the first listed on a tie).
fn largest_bank(graph: &ScenarioGraph) -> Option<i64> {
    let mut banks: Vec<_> = graph
        .accounts
        .iter()
        .filter_map(|a| Some((a.sort_order, a.id, graph.bank.get(&a.id)?.cash_value)))
        .collect();
    banks.sort_by_key(|(order, id, _)| (*order, *id));
    banks
        .into_iter()
        .rev()
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(_, id, _)| id)
}

fn converts(effect: &EventEffect) -> bool {
    match effect {
        EventEffect::RothConversion { .. } => true,
        EventEffect::Random {
            on_true, on_false, ..
        } => converts(on_true) || on_false.as_deref().is_some_and(converts),
        _ => false,
    }
}

/// Every conversion in `config` set to `choice`: filling to its bracket, or
/// converting nothing (nested in `Random` effects too).
pub(super) fn set_conversions(config: &mut SimulationConfig, choice: ConversionChoice) {
    let amount = match choice {
        ConversionChoice::Off => TransferAmount::fixed(0.0),
        ConversionChoice::UpTo { ceiling_rate } => TransferAmount::bracket_room(ceiling_rate),
    };
    fn set(effect: &mut EventEffect, wanted: &TransferAmount) {
        match effect {
            EventEffect::RothConversion { amount, .. } => *amount = wanted.clone(),
            EventEffect::Random {
                on_true, on_false, ..
            } => {
                set(on_true, wanted);
                if let Some(effect) = on_false {
                    set(effect, wanted);
                }
            }
            _ => {}
        }
    }
    for event in &mut config.events {
        for effect in &mut event.effects {
            set(effect, &amount);
        }
    }
}

/// The toggle on one plan, and that plan with the overlay event added when a
/// choice asks for a rate and the plan has no conversions of its own.
pub(super) struct Conversions {
    info: DrawdownConversions,
    overlay: Option<CompiledScenario>,
}

impl Conversions {
    pub fn prepare(
        graph: &ScenarioGraph,
        compiled: &CompiledScenario,
        retirement: &Retirement,
        choices: &[StrategyChoice],
    ) -> PlanResult<Self> {
        let events: Vec<i64> = compiled
            .config
            .events
            .iter()
            .filter(|event| event.effects.iter().any(converts))
            .filter_map(|event| compiled.id_map.event_db_id(event.event_id))
            .collect();
        let (from, to) = (largest(graph, "TaxDeferred"), largest(graph, "TaxFree"));
        let unavailable = match (from, to) {
            (None, _) => Some(
                "Roth conversions need a tax-deferred account (a 401(k) or traditional IRA) \
                 to convert from."
                    .to_string(),
            ),
            (_, None) => {
                Some("Roth conversions need a Roth (tax-free) account to convert into.".to_string())
            }
            _ => None,
        };
        let overlay = match (from, to) {
            (Some(from), Some(to)) if events.is_empty() => {
                let start = graph
                    .scenario
                    .start_date
                    .parse::<jiff::civil::Date>()
                    .map_or(retirement.year, |d| i64::from(d.year()));
                let birth = graph
                    .scenario
                    .birth_date
                    .as_deref()
                    .and_then(|d| d.parse::<jiff::civil::Date>().ok());
                Some(ConversionOverlay {
                    from_account_id: from,
                    to_account_id: to,
                    pay_tax_from_account_id: largest_taxable(graph).or_else(|| largest_bank(graph)),
                    // A start before the plan's would fire at once rather than
                    // on Dec 30.
                    start_year: retirement.year.max(start),
                    until_age: i64::from(
                        birth.map_or(RMD_AGE, |b| crate::rules::rmd_age(i64::from(b.year()))),
                    ),
                })
            }
            _ => None,
        };

        let wants_rate = choices
            .iter()
            .any(|c| matches!(c.conversion(), Some(ConversionChoice::UpTo { .. })));
        if wants_rate && let Some(reason) = &unavailable {
            return Err(PlanError::invalid(reason.clone()));
        }
        let compiled_overlay = match &overlay {
            // The rate is the template's; each choice sets its own.
            Some(overlay) if wants_rate => Some(compile::compile(&with_overlay(graph, overlay)?)?),
            _ => None,
        };
        Ok(Self {
            info: DrawdownConversions {
                unavailable,
                events,
                overlay,
            },
            overlay: compiled_overlay,
        })
    }

    /// The compiled plan `choice` runs from, and whether that took the
    /// overlay: the plan as it is, or with the template's event added when
    /// the choice asks for a rate and the plan has no conversions.
    pub fn base<'a>(
        &'a self,
        compiled: &'a CompiledScenario,
        choice: &StrategyChoice,
    ) -> (&'a CompiledScenario, bool) {
        match (choice.conversion(), &self.overlay) {
            (Some(ConversionChoice::UpTo { .. }), Some(overlay)) => (overlay, true),
            _ => (compiled, false),
        }
    }

    pub fn info(&self) -> DrawdownConversions {
        self.info.clone()
    }
}

/// `graph` with the `RothConversions` template's event added, through the
/// same changes a review path or the drafting agent would apply.
fn with_overlay(graph: &ScenarioGraph, overlay: &ConversionOverlay) -> PlanResult<ScenarioGraph> {
    let expansion = expand_template_in(graph, "drawdown_", &overlay.template(0.22))?;
    let resolved = suggest::resolve(graph, &expansion.changes).map_err(|problems| {
        PlanError::internal(format!(
            "the conversion overlay does not apply: {problems:?}"
        ))
    })?;
    let mut plan = graph.clone();
    suggest::apply_to_graph(&mut plan, &resolved, &mut Created::new())?.map_err(|problem| {
        PlanError::internal(format!(
            "the conversion overlay does not apply: {problem:?}"
        ))
    })?;
    Ok(plan)
}
