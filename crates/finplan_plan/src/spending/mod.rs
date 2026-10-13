//! Compare spending rules (spec 22, analysis): the plan's spending under each
//! rule, over the same markets.
//!
//! The plan's spending event (its `Spending` effect, or else a fixed
//! retirement `Expense`) has its effect swapped for each choice, and every
//! choice runs a small Monte Carlo on [`ANALYSIS_SEED`] (common random numbers:
//! differences come from the rule, not from sampling). The rate rules start at
//! the plan's own first-year draw: Guardrails at that amount, the rate rules
//! at the rate it is of the portfolio when spending starts, so the comparison
//! is about how each rule *moves*, not where it starts. Pure: no I/O.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use finplan_core::analysis::McRunner;
use finplan_core::config::SimulationConfig;
use finplan_core::model::{
    EventEffect, EventId, EventTrigger, MonteCarloConfig, MonteCarloStats, StateEvent,
    TransferAmount,
};
use finplan_core::simulation::simulate;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::analysis::run::AnalysisError;
use crate::analysis::{ANALYSIS_SEED, iterations_or_default};
use crate::compile::{self, CompiledScenario};
use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;
use crate::specs::{AmountSpec, EffectSpec, SpendingPortfolioSpec, SpendingRuleSpec};

/// Iterations per choice when the request names none.
pub const DEFAULT_COMPARE_ITERATIONS: usize = 200;
/// The most a request may ask for per choice.
pub const MAX_COMPARE_ITERATIONS: usize = 500;
/// Most choices one request may ask for.
pub const MAX_CHOICES: usize = 8;
const PARALLEL_BATCHES: usize = 4;

/// One row of the comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum SpendingChoice {
    /// The plan as it is.
    AsPlanned,
    /// A plain `Expense` of the plan's first-year draw, raised for inflation.
    Fixed,
    /// The spending event under this rule, from the paying account, reading
    /// every liquid account, with pass-through.
    Rule { rule: SpendingRuleSpec },
}

/// `POST /runs/{id}/spending/compare`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, optional_fields = nullable)]
pub struct SpendingCompareRequest {
    /// Absent means the default list: As planned, Fixed, Percent of
    /// portfolio at the plan's initial rate, Guardrails, Capped change, VPW.
    pub choices: Option<Vec<SpendingChoice>>,
    /// The event whose spending is swapped; absent, the plan's first event
    /// with a `Spending` effect, else its retirement spending `Expense`.
    pub event_id: Option<i64>,
    /// Per choice; default 200, at most 500.
    pub iterations: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct SpendingComparisonRow {
    pub choice: SpendingChoice,
    /// What the row is called on screen.
    pub label: String,
    /// The effect Apply writes into the event in place of its spending; null
    /// for As planned.
    pub effect: Option<EffectSpec>,
    pub funding_success_rate: Option<f64>,
    pub success_rate: f64,
    /// Real (start-year dollars), over every iteration.
    pub median_lifetime_spending: f64,
    pub p10_lowest_draw: f64,
    /// Share of markets with a cut deeper than 10% in the real draw.
    pub cut_rate: f64,
    pub median_first_draw: f64,
    /// Median final net worth, nominal, and in start-year dollars on the
    /// median path's inflation.
    pub median_final_net_worth: f64,
    pub median_final_net_worth_real: Option<f64>,
    /// The draw by calendar year, real: P10, P50 and P90 over the markets
    /// that drew that year. A fixed expense draws one amount every year.
    pub draws: Vec<SpendingDrawYear>,
}

/// One year of a row's [`draws`](SpendingComparisonRow::draws).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpendingDrawYear {
    pub year: i16,
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct SpendingComparison {
    pub iterations: i64,
    pub event_id: i64,
    pub event_name: String,
    /// The plan's first-year draw, real: where Fixed and Guardrails start.
    pub first_draw: f64,
    /// That draw as a share of the portfolio when spending starts: where the
    /// rate rules start.
    pub initial_rate: f64,
    pub rows: Vec<SpendingComparisonRow>,
}

/// The event being compared, and how its spending reads in the plan.
struct Target {
    db_id: i64,
    name: String,
    engine: EventId,
    /// Index of the spending effect among the event's effects.
    effect_index: usize,
    from: i64,
    per_year: u32,
    /// The plan's own rule, when the event already has one.
    own: Option<SpendingRuleSpec>,
}

fn per_year(trigger: &EventTrigger) -> Option<u32> {
    match trigger {
        EventTrigger::Repeating { interval, .. } => interval.per_year(),
        _ => None,
    }
}

/// The plan's spending event: `event_id`, or the first event with a
/// `Spending` effect, or one named for retirement with an `Expense`.
fn target(
    graph: &ScenarioGraph,
    compiled: &CompiledScenario,
    event_id: Option<i64>,
) -> PlanResult<Target> {
    let events = graph
        .events
        .iter()
        .map(|e| crate::specs::events::read_event(graph, e.id))
        .collect::<PlanResult<Vec<_>>>()?;
    let enabled = |e: &&crate::specs::events::Event| e.enabled;
    let spending = |e: &crate::specs::events::Event| {
        e.effects
            .iter()
            .position(|f| matches!(f, EffectSpec::Spending { .. }))
            .or_else(|| {
                e.effects
                    .iter()
                    .position(|f| matches!(f, EffectSpec::Expense { .. }))
            })
    };
    let event = match event_id {
        Some(id) => events
            .iter()
            .find(|e| e.id == id)
            .ok_or(PlanError::NotFound("event"))?,
        None => events
            .iter()
            .filter(enabled)
            .find(|e| {
                e.effects
                    .iter()
                    .any(|f| matches!(f, EffectSpec::Spending { .. }))
            })
            .or_else(|| {
                events.iter().filter(enabled).find(|e| {
                    e.name.to_lowercase().contains("retire")
                        && e.effects
                            .iter()
                            .any(|f| matches!(f, EffectSpec::Expense { .. }))
                })
            })
            .ok_or_else(|| {
                PlanError::unprocessable(
                    "the plan has no spending rule or retirement spending event to compare",
                )
            })?,
    };
    let effect_index = spending(event).ok_or_else(|| {
        PlanError::unprocessable(format!("{} has no spending to compare", event.name))
    })?;
    let engine = compiled.id_map.event(event.id)?;
    let trigger = &compiled
        .config
        .events
        .iter()
        .find(|e| e.event_id == engine)
        .ok_or_else(|| PlanError::unprocessable(format!("{} is switched off", event.name)))?
        .trigger;
    let per_year = per_year(trigger).ok_or_else(|| {
        PlanError::unprocessable(format!(
            "{} must repeat monthly, quarterly or yearly to compare spending rules",
            event.name
        ))
    })?;
    let (from, own) = match &event.effects[effect_index] {
        EffectSpec::Spending {
            from_account_id,
            rule,
            ..
        } => (*from_account_id, Some(rule.clone())),
        EffectSpec::Expense {
            from_account_id, ..
        } => (*from_account_id, None),
        _ => unreachable!("the index found a Spending or Expense effect"),
    };
    Ok(Target {
        db_id: event.id,
        name: event.name.clone(),
        engine,
        effect_index,
        from,
        per_year,
        own,
    })
}

/// The default list, starting where the plan starts.
#[must_use]
pub fn default_choices(first_draw: f64, initial_rate: f64, final_age: u8) -> Vec<SpendingChoice> {
    let rate = (initial_rate * 10_000.0).round() / 10_000.0;
    vec![
        SpendingChoice::AsPlanned,
        SpendingChoice::Fixed,
        SpendingChoice::Rule {
            rule: SpendingRuleSpec::PercentOfPortfolio {
                rate,
                floor: None,
                ceiling: None,
            },
        },
        SpendingChoice::Rule {
            rule: SpendingRuleSpec::guardrails(first_draw.round()),
        },
        SpendingChoice::Rule {
            rule: SpendingRuleSpec::CappedChange {
                rate,
                max_cut: 0.025,
                max_raise: 0.05,
            },
        },
        SpendingChoice::Rule {
            rule: SpendingRuleSpec::Vpw {
                real_return: None,
                final_age,
            },
        },
    ]
}

impl SpendingChoice {
    fn label(&self) -> String {
        match self {
            SpendingChoice::AsPlanned => "As planned".into(),
            SpendingChoice::Fixed => "Fixed".into(),
            SpendingChoice::Rule { rule } => rule.label().into(),
        }
    }

    /// The effect Apply writes, paying from `from`; None for As planned.
    fn effect(&self, from: i64, first_draw: f64, per_year: u32) -> Option<EffectSpec> {
        match self {
            SpendingChoice::AsPlanned => None,
            SpendingChoice::Fixed => Some(EffectSpec::Expense {
                from_account_id: from,
                amount: AmountSpec::Expression {
                    source: format!(
                        "inflation({})",
                        (first_draw / f64::from(per_year) * 100.0).round() / 100.0
                    ),
                },
            }),
            SpendingChoice::Rule { rule } => Some(EffectSpec::Spending {
                from_account_id: from,
                rule: rule.clone(),
                portfolio: SpendingPortfolioSpec::Liquid,
                pass_through: true,
            }),
        }
    }
}

/// `config` with the target's spending effect replaced by `choice`'s.
fn swapped(
    config: &SimulationConfig,
    compiled: &CompiledScenario,
    target: &Target,
    choice: &SpendingChoice,
    first_draw: f64,
) -> PlanResult<SimulationConfig> {
    let mut config = config.clone();
    let from = compiled.id_map.account(target.from)?;
    let effect = match choice {
        SpendingChoice::AsPlanned => return Ok(config),
        SpendingChoice::Fixed => EventEffect::Expense {
            from,
            amount: TransferAmount::inflation_adjusted(first_draw / f64::from(target.per_year)),
        },
        SpendingChoice::Rule { rule } => {
            rule.validate()?;
            let portfolio = finplan_core::model::SpendingPortfolio::Liquid;
            let real_return = finplan_core::spending::expected_real_return(&config, &portfolio);
            EventEffect::Spending {
                from,
                rule: rule.to_core(real_return),
                portfolio,
                pass_through: true,
            }
        }
    };
    let event = config
        .events
        .iter_mut()
        .find(|e| e.event_id == target.engine)
        .ok_or_else(|| PlanError::internal("the spending event is not in the compiled plan"))?;
    event.effects[target.effect_index] = effect;
    Ok(config)
}

/// The plan's first-year draw (real) and the share of the liquid portfolio it
/// is when spending starts, read off the plan's median path: a rule's own
/// median first draw, an expense's own figure, or else what the expense paid
/// in its first full year.
fn starting_point(
    config: &SimulationConfig,
    target: &Target,
    stats: &MonteCarloStats,
    median_seed: Option<u64>,
) -> (f64, f64) {
    let mut ledgered = config.clone();
    ledgered.collect_ledger = true;
    let path = median_seed.and_then(|seed| simulate(&ledgered, seed).ok());
    let Some(path) = path else {
        return (0.0, 0.04);
    };
    let paid: Vec<_> = path
        .ledger
        .iter()
        .filter(|e| e.source_event == Some(target.engine))
        .filter_map(|e| match e.event {
            StateEvent::CashDebit { amount, .. } => Some((e.date, amount)),
            _ => None,
        })
        .collect();
    let Some(&(first_date, _)) = paid.first() else {
        return (0.0, 0.04);
    };
    let factor = |year: i16| {
        let start = path
            .wealth_snapshots
            .first()
            .map_or(year, |s| s.date.year());
        path.cumulative_inflation
            .get((year - start).max(0) as usize)
            .or(path.cumulative_inflation.last())
            .copied()
            .unwrap_or(1.0)
    };
    let first_year_paid = || {
        // The first year it paid every occurrence.
        let year = if first_date.month() == 1 && first_date.day() <= 28 {
            first_date.year()
        } else {
            first_date.year() + 1
        };
        let nominal: f64 = paid
            .iter()
            .filter(|(d, _)| d.year() == year)
            .map(|(_, a)| a)
            .sum();
        let nominal = if nominal > 0.0 {
            nominal
        } else {
            paid[0].1 * f64::from(target.per_year)
        };
        nominal / factor(year)
    };
    let first_draw = match stats.spending.as_ref().filter(|_| target.own.is_some()) {
        Some(spending) => spending.median_first_draw,
        // A plain figure says its own real amount.
        None => fixed_draw(config, target).unwrap_or_else(first_year_paid),
    };
    // The liquid portfolio at the last snapshot before spending began.
    let portfolio = path
        .wealth_snapshots
        .iter()
        .rev()
        .find(|s| s.date <= first_date)
        .or(path.wealth_snapshots.first())
        .map_or(0.0, |s| {
            s.accounts
                .iter()
                .filter(|a| {
                    matches!(
                        a.flavor,
                        finplan_core::model::AccountSnapshotFlavor::Bank(_)
                            | finplan_core::model::AccountSnapshotFlavor::Investment { .. }
                    )
                })
                .map(finplan_core::model::AccountSnapshot::total_value)
                .sum::<f64>()
                / factor(s.date.year())
        });
    let rate = if portfolio > 0.0 {
        (first_draw / portfolio).clamp(0.001, 0.2)
    } else {
        0.04
    };
    (first_draw, rate)
}

/// The VPW final age the default list uses: 100, or a year past the age at
/// the end of the plan when that is later.
fn vpw_age(graph: &ScenarioGraph) -> u8 {
    let (Some(birth), Ok(start)) = (
        graph
            .scenario
            .birth_date
            .as_deref()
            .and_then(|d| d.parse::<jiff::civil::Date>().ok()),
        graph.scenario.start_date.parse::<jiff::civil::Date>(),
    ) else {
        return 100;
    };
    let end_age = i64::from(start.year() - birth.year()) + graph.scenario.duration_years;
    u8::try_from(end_age.clamp(100, 120)).unwrap_or(100)
}

/// Run a Monte Carlo of `graph` per spending choice, all on one seed.
pub fn compare(
    graph: &ScenarioGraph,
    body: &SpendingCompareRequest,
    runner: &mut dyn McRunner,
) -> Result<SpendingComparison, AnalysisError> {
    let iterations = iterations_or_default(
        body.iterations,
        DEFAULT_COMPARE_ITERATIONS,
        MAX_COMPARE_ITERATIONS,
    )?;
    if let Some(choices) = &body.choices
        && (choices.is_empty() || choices.len() > MAX_CHOICES)
    {
        return Err(
            PlanError::invalid(format!("ask for between 1 and {MAX_CHOICES} choices")).into(),
        );
    }
    let compiled = compile::compile(graph)?;
    let target = target(graph, &compiled, body.event_id)?;
    let mut base = compiled.config.clone();
    base.collect_ledger = false;
    let mc = MonteCarloConfig {
        iterations,
        percentiles: vec![0.5],
        compute_mean: false,
        parallel_batches: PARALLEL_BATCHES,
        seed: Some(ANALYSIS_SEED),
        ..Default::default()
    };
    let count = body.choices.as_ref().map_or(6, Vec::len);
    runner.begin((iterations + 1) * (count + 1));

    // As planned first: it is where the others start.
    let planned = runner.stats(&base, &mc)?;
    let median_seed = planned
        .1
        .iter()
        .find(|(p, _)| (*p - 0.5).abs() < 1e-9)
        .map(|(_, seed)| *seed);
    let (first_draw, initial_rate) = starting_point(&base, &target, &planned.0, median_seed);
    let choices = body
        .choices
        .clone()
        .unwrap_or_else(|| default_choices(first_draw, initial_rate, vpw_age(graph)));

    let mut rows = Vec::with_capacity(choices.len());
    for choice in choices {
        if runner.cancelled() {
            return Err(AnalysisError::Cancelled);
        }
        let config = swapped(&base, &compiled, &target, &choice, first_draw)?;
        let (stats, seeds) = if choice == SpendingChoice::AsPlanned {
            planned.clone()
        } else {
            runner.stats(&config, &mc)?
        };
        let median = stats
            .percentile_values
            .iter()
            .find(|(p, _)| (*p - 0.5).abs() < 1e-9)
            .map_or(stats.mean_final_net_worth, |(_, v)| *v);
        // The median path, with its ledger: its inflation deflates the
        // ending balance, and a fixed expense's payments are counted on it.
        let path = seeds
            .iter()
            .find(|(p, _)| (*p - 0.5).abs() < 1e-9)
            .and_then(|(_, seed)| {
                let mut ledgered = config.clone();
                ledgered.collect_ledger = true;
                simulate(&ledgered, *seed).ok()
            });
        let inflation = path
            .as_ref()
            .and_then(|p| p.cumulative_inflation.last().copied());
        let spending = match &stats.spending {
            Some(s) => (
                s.median_lifetime_spending,
                s.p10_lowest_draw,
                s.cut_rate,
                s.median_first_draw,
                s.by_year
                    .iter()
                    .map(|y| SpendingDrawYear {
                        year: y.year,
                        p10: y.p10,
                        p50: y.p50,
                        p90: y.p90,
                    })
                    .collect(),
            ),
            // A fixed expense draws the same real amount in every market.
            None => {
                let paid: Vec<i16> = path.as_ref().map_or_else(Vec::new, |p| {
                    p.ledger
                        .iter()
                        .filter(|e| {
                            e.source_event == Some(target.engine)
                                && matches!(e.event, StateEvent::CashDebit { .. })
                        })
                        .map(|e| e.date.year())
                        .collect()
                });
                let occurrences = paid.len();
                let years: BTreeSet<i16> = paid.into_iter().collect();
                let draw = fixed_draw(&config, &target).unwrap_or(first_draw);
                (
                    draw / f64::from(target.per_year) * occurrences as f64,
                    draw,
                    0.0,
                    draw,
                    years
                        .into_iter()
                        .map(|year| SpendingDrawYear {
                            year,
                            p10: draw,
                            p50: draw,
                            p90: draw,
                        })
                        .collect(),
                )
            }
        };
        rows.push(SpendingComparisonRow {
            label: choice.label(),
            effect: choice.effect(target.from, first_draw, target.per_year),
            choice,
            funding_success_rate: stats.funding_success_rate,
            success_rate: stats.success_rate,
            median_lifetime_spending: spending.0,
            p10_lowest_draw: spending.1,
            cut_rate: spending.2,
            median_first_draw: spending.3,
            median_final_net_worth: median,
            median_final_net_worth_real: inflation.map(|factor| median / factor),
            draws: spending.4,
        });
    }

    Ok(SpendingComparison {
        iterations: iterations as i64,
        event_id: target.db_id,
        event_name: target.name,
        first_draw,
        initial_rate,
        rows,
    })
}

/// A fixed expense's yearly amount in start-year dollars, when its amount is
/// a plain figure (inflation-adjusted or not).
fn fixed_draw(config: &SimulationConfig, target: &Target) -> Option<f64> {
    let event = config.events.iter().find(|e| e.event_id == target.engine)?;
    let EventEffect::Expense { amount, .. } = event.effects.get(target.effect_index)? else {
        return None;
    };
    let source = amount.expression().to_source(&Default::default()).ok()?;
    let figure = source
        .strip_prefix("inflation(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(&source)
        .trim()
        .parse::<f64>()
        .ok()?;
    Some(figure * f64::from(target.per_year))
}
