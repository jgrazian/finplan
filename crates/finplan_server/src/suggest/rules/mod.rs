//! Rule-based review notes: deterministic checks over a run's inputs and
//! results, each written up as a draft suggestion in the Review tab's voice.
//!
//! Pure — no database, no async. The caller supplies the base run's snapshot
//! [`ScenarioGraph`] and the [`Results`] the API serves for that run; every
//! number a note quotes comes from one of the two, and every [`Evidence`]
//! entry points back at where. A draft offers paths — courses of action, each
//! ordered steps of [`Change`]s — only when a fix is unambiguous, and every
//! path's changes resolve against the graph the draft came from
//! (`super::resolve`), `expect` included.
//!
//! Path-level facts (balances, cash flows) come from the shown path — the
//! terminal-median-ranked one unless the caller asked for another — so notes
//! built on them say "on the median path". Run-level facts come from `stats`
//! and `funding_diagnostics`.

mod plan;
mod portfolio;
mod results;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Change;
use crate::api::events::Event;
use crate::api::runs::Results;
use crate::api::specs::{AmountSpec, OffsetUnit, TriggerSpec};
use crate::compile::rows::ScenarioGraph;

/// What a note asks of the reader. Declared in severity order: the review
/// lists fixes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "SuggestionKind")]
pub enum Kind {
    /// Something in the plan is wrong, and the note says how to change it.
    Fix,
    /// An input that may be wrong; only the user knows.
    Check,
    /// A scenario to run against the plan.
    Stress,
    /// A reading of the results; nothing to change.
    Read,
}

/// The Review column a note belongs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "SuggestionSection")]
pub enum Section {
    Portfolio,
    Plan,
    Results,
}

/// Where a number in a note came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "ref", rename_all = "snake_case")]
#[ts(export)]
pub enum Evidence {
    /// The shown path's ledger for one year, narrowed to an event or account.
    Ledger {
        year: i64,
        #[serde(default)]
        event_id: Option<i64>,
        #[serde(default)]
        account_id: Option<i64>,
    },
    /// One point of the shown path's balance for an account.
    AccountSeries {
        account_id: i64,
        date: String,
        value: f64,
    },
    /// A figure read or derived from the plan or the results.
    Stat { name: String, value: f64 },
    /// A field of `funding_diagnostics`.
    Diagnostic { field: String, value: f64 },
}

/// One review note, before anyone has previewed or stored it.
#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    /// The rule that wrote it; stable, so a dismissal can be keyed on it.
    pub rule: &'static str,
    pub kind: Kind,
    pub section: Section,
    pub title: String,
    pub reasoning: String,
    pub evidence: Vec<Evidence>,
    /// The courses of action; empty unless a fix is unambiguous.
    pub paths: Vec<DraftPath>,
}

/// One course of action a rule offers: ordered steps.
#[derive(Debug, Clone, Serialize)]
pub struct DraftPath {
    /// Unique within the draft; stable per rule.
    pub key: &'static str,
    pub label: String,
    /// Why this path, when the note's reasoning does not already say.
    pub reasoning: Option<String>,
    pub recommended: bool,
    pub steps: Vec<DraftStep>,
}

/// One step of a rule's path.
#[derive(Debug, Clone, Serialize)]
pub struct DraftStep {
    pub key: &'static str,
    pub title: String,
    pub reasoning: Option<String>,
    pub changes: Vec<Change>,
}

impl DraftPath {
    /// A path of one step.
    pub fn single(
        key: &'static str,
        label: impl Into<String>,
        recommended: bool,
        changes: Vec<Change>,
    ) -> Self {
        let label = label.into();
        Self {
            key,
            steps: vec![DraftStep {
                key: "a",
                title: label.clone(),
                reasoning: None,
                changes,
            }],
            label,
            reasoning: None,
            recommended,
        }
    }

    /// The single, recommended course of action of a note that has one.
    pub fn only(label: impl Into<String>, changes: Vec<Change>) -> Self {
        Self::single("a", label, true, changes)
    }

    /// Every change of every step, in order.
    pub fn changes(&self) -> impl Iterator<Item = &Change> {
        self.steps.iter().flat_map(|s| s.changes.iter())
    }
}

type Rule = fn(&Ctx) -> Vec<Draft>;

const RULES: &[Rule] = &[
    portfolio::cost_basis_equals_value,
    portfolio::idle_bank_cash,
    portfolio::unused_contribution_limits,
    portfolio::unmapped_or_mismatched_assets,
    plan::sweep_sells_while_cash,
    plan::liability_payment_inflation_adjusted,
    results::shortfall_account_concentration,
    results::success_vs_funding_gap,
];

/// Every note the rules find, fixes first, then by rule and title.
pub fn review(graph: &ScenarioGraph, results: &Results) -> Vec<Draft> {
    let ctx = Ctx::new(graph, results);
    let mut drafts: Vec<Draft> = RULES.iter().flat_map(|rule| rule(&ctx)).collect();
    drafts.sort_by(|a, b| (a.kind, a.rule, &a.title).cmp(&(b.kind, b.rule, &b.title)));
    drafts
}

// ── shared reading of the plan and the run ──────────────────────────────────

/// A calendar month, for placing one-off events against dated balances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Month {
    pub year: i64,
    /// 1-12.
    pub month: i64,
}

impl Month {
    fn from_date(date: &str) -> Option<Self> {
        let year = date.get(0..4)?.parse().ok()?;
        let month = date.get(5..7)?.parse().ok()?;
        Some(Month { year, month })
    }

    fn plus_months(self, months: i64) -> Self {
        let total = self.year * 12 + (self.month - 1) + months;
        Month {
            year: total.div_euclid(12),
            month: total.rem_euclid(12) + 1,
        }
    }

    /// The first of the month, comparable with the results' ISO dates.
    pub fn date(self) -> String {
        format!("{:04}-{:02}-01", self.year, self.month)
    }
}

pub(super) struct Ctx<'a> {
    pub graph: &'a ScenarioGraph,
    pub results: &'a Results,
    /// Enabled events, read back in their GET shape, in list order.
    pub events: Vec<Event>,
    /// The shown path's dates, aligned with each `account_series` entry.
    dates: Vec<&'a str>,
    series: HashMap<i64, &'a [f64]>,
}

impl<'a> Ctx<'a> {
    fn new(graph: &'a ScenarioGraph, results: &'a Results) -> Self {
        let mut rows: Vec<_> = graph.events.iter().filter(|e| e.enabled != 0).collect();
        rows.sort_by_key(|e| (e.sort_order, e.id));
        let events = rows
            .into_iter()
            .filter_map(|e| crate::api::events::read_event(graph, e.id).ok())
            .collect();
        let dates = results
            .bands
            .iter()
            .find(|band| band.path_id == results.series_id)
            .map(|band| band.dates.iter().map(String::as_str).collect())
            .unwrap_or_default();
        let series = results
            .account_series
            .iter()
            .map(|s| (s.account_id, s.values.as_slice()))
            .collect();
        Ctx {
            graph,
            results,
            events,
            dates,
            series,
        }
    }

    pub fn account_name(&self, id: i64) -> String {
        self.graph
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map_or_else(|| format!("account #{id}"), |a| a.name.clone())
    }

    pub fn flavor(&self, id: i64) -> Option<&str> {
        self.graph
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.flavor.as_str())
    }

    /// The shown path's dated balances for one account.
    pub fn balances(&self, account_id: i64) -> Vec<(&'a str, f64)> {
        let Some(values) = self.series.get(&account_id) else {
            return Vec::new();
        };
        self.dates
            .iter()
            .copied()
            .zip(values.iter().copied())
            .collect()
    }

    /// The balance at the end of `year`.
    pub fn year_end(&self, account_id: i64, year: i64) -> Option<f64> {
        let date = format!("{year:04}-12-31");
        self.balances(account_id)
            .into_iter()
            .find(|(d, _)| *d == date)
            .map(|(_, v)| v)
    }

    /// The last balance dated strictly before `date`.
    pub fn balance_before(&self, account_id: i64, date: &str) -> Option<(&'a str, f64)> {
        self.balances(account_id)
            .into_iter()
            .take_while(|(d, _)| *d < date)
            .last()
    }

    pub fn expenses(&self, year: i64) -> Option<f64> {
        self.results
            .cash_flows
            .iter()
            .find(|c| c.year == year)
            .map(|c| c.expenses)
    }

    /// Cumulative inflation for `year` on the shown path; 1.0 when unknown.
    pub fn inflation(&self, year: i64) -> f64 {
        self.results
            .inflation
            .iter()
            .find(|p| p.year == year)
            .map_or(1.0, |p| p.factor)
    }

    pub fn start(&self) -> Option<Month> {
        Month::from_date(&self.graph.scenario.start_date)
    }

    /// The plan's first year covers only part of the calendar year unless it
    /// starts on January 1st.
    pub fn is_partial_first_year(&self, year: i64) -> bool {
        self.graph.scenario.start_date.get(0..4) == Some(&year.to_string())
            && self.graph.scenario.start_date.get(5..) != Some("01-01")
    }

    pub fn age_in(&self, year: i64) -> Option<i64> {
        let birth = Month::from_date(self.graph.scenario.birth_date.as_deref()?)?;
        Some(year - birth.year)
    }

    /// When a trigger first fires, where that follows from dates and ages
    /// alone. Balance and net-worth conditions depend on the path: `None`.
    pub fn fires(&self, trigger: &TriggerSpec) -> Option<Month> {
        self.fires_within(trigger, 0)
    }

    fn fires_within(&self, trigger: &TriggerSpec, depth: usize) -> Option<Month> {
        if depth > 8 {
            return None;
        }
        match trigger {
            TriggerSpec::Date { on_date } => Month::from_date(on_date),
            TriggerSpec::Age { years, months } => {
                let birth = Month::from_date(self.graph.scenario.birth_date.as_deref()?)?;
                Some(birth.plus_months(i64::from(*years) * 12 + i64::from(months.unwrap_or(0))))
            }
            TriggerSpec::RelativeToEvent {
                event_id,
                unit,
                value,
            } => {
                let target = self.events.iter().find(|e| e.id == *event_id)?;
                let base = self.fires_within(&target.trigger, depth + 1)?;
                let months = match unit {
                    OffsetUnit::Years => i64::from(*value) * 12,
                    OffsetUnit::Months => i64::from(*value),
                    OffsetUnit::Days => i64::from(*value) / 30,
                };
                Some(base.plus_months(months))
            }
            TriggerSpec::Repeating {
                start_condition, ..
            } => match start_condition {
                Some(start) => self.fires_within(start, depth + 1),
                None => self.start(),
            },
            _ => None,
        }
    }
}

/// A fixed amount's nominal value in `year`, and whether it follows inflation.
/// Anything computed from balances or expressions has no single value: `None`.
/// A label or title cut to the 80 characters a path or step allows.
pub(super) fn clip(text: &str) -> String {
    if text.chars().count() <= 80 {
        return text.to_string();
    }
    let cut: String = text.chars().take(79).collect();
    format!("{}…", cut.trim_end())
}

pub(super) fn fixed_amount(amount: &AmountSpec, inflation: f64) -> Option<(f64, bool)> {
    match amount {
        AmountSpec::Fixed { value } => Some((*value, false)),
        AmountSpec::InflationAdjusted { inner } => match inner.as_ref() {
            AmountSpec::Fixed { value } => Some((value * inflation, true)),
            _ => None,
        },
        _ => None,
    }
}

// ── formatting ──────────────────────────────────────────────────────────────

/// Dollars the way the Review tab writes them: `$38,200`, `$218k`, `$1.45M`.
pub(super) fn money(value: f64) -> String {
    let sign = if value < 0.0 { "−" } else { "" };
    let abs = value.abs();
    let body = if abs >= 1e6 {
        let millions = abs / 1e6;
        let text = if millions >= 100.0 {
            format!("{millions:.1}")
        } else {
            format!("{millions:.2}")
        };
        format!("{}M", trim_zeros(&text))
    } else if abs >= 100_000.0 {
        format!("{}k", (abs / 1e3).round())
    } else {
        thousands(abs.round() as i64)
    };
    format!("{sign}${body}")
}

fn trim_zeros(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}

/// An integer with thousands separators: `2,000`.
pub(super) fn thousands(n: i64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A fraction as a percentage with one decimal: `95.0%`.
pub(super) fn percent(fraction: f64) -> String {
    format!("{:.1}%", fraction * 100.0)
}

/// `A`, `A and B`, `A, B and C`.
pub(super) fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

pub(super) fn year_of(date: &str) -> i64 {
    Month::from_date(date).map_or(0, |m| m.year)
}
