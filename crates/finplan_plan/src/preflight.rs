//! The pre-run review of a plan: what would stop it running, and what looks
//! unintentional. Pure over a [`ScenarioGraph`], so the server and the browser
//! raise the same notes.

use serde::Serialize;
use ts_rs::TS;

use crate::graph::ScenarioGraph;

/// One finding of a [`PreflightReport`].
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct PreflightIssue {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub section: String,
    pub record_id: Option<i64>,
}

/// Everything a plan's review found, and whether the plan can run at all.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct PreflightReport {
    pub issues: Vec<PreflightIssue>,
    pub can_run: bool,
}

/// Declares [`Code`] and [`Code::ALL`] from one list, so a code cannot be
/// raised without being listed: the standard checks' catalogue
/// (`crate::rules::catalogue`) is held to that list.
macro_rules! codes {
    ($($variant:ident => $code:literal,)*) => {
        /// What a [`PreflightIssue`] is about; `as_str` is its wire `code`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Code {
            $($variant,)*
        }

        impl Code {
            /// Every code [`preflight`] can raise.
            pub const ALL: &[Code] = &[$(Code::$variant,)*];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Code::$variant => $code,)*
                }
            }
        }
    };
}

codes! {
    InvalidPlan => "invalid_plan",
    UnmappedAsset => "unmapped_asset",
    EmptyEvent => "empty_event",
    MissingSpending => "missing_spending",
    FundingIntent => "funding_intent",
    MissingBirth => "missing_birth",
    Horizon => "horizon",
    NoInflation => "no_inflation",
    Assumptions => "assumptions",
    RothConversionCandidate => "roth_conversion_candidate",
}

/// Review `g`: compile it, then look for the omissions a run cannot report
/// for itself (no spending, no inflation, an asset with no return, ...).
/// `can_run` is false only when an issue is an error; warnings do not block.
pub fn preflight(g: &ScenarioGraph) -> PreflightReport {
    let mut issues = vec![];
    let mut add = |code: Code, severity: &str, message: String, section: &str, record_id| {
        issues.push(PreflightIssue {
            code: code.as_str().into(),
            severity: severity.into(),
            message,
            section: section.into(),
            record_id,
        })
    };
    if let Err(e) = crate::compile::compile(g) {
        add(Code::InvalidPlan, "error", e.to_string(), "plan", None);
    }
    for a in &g.assets {
        if a.return_profile_id.is_none() {
            add(
                Code::UnmappedAsset,
                "warning",
                format!(
                    "{} has no return assumption. Its price stays fixed in nominal dollars; confirm that this is intentional.",
                    a.name
                ),
                "portfolio",
                Some(a.id),
            );
        }
    }
    for e in &g.events {
        if e.enabled != 0 && g.event_effects.get(&e.id).is_none_or(|v| v.is_empty()) {
            add(
                Code::EmptyEvent,
                "warning",
                format!("{} is enabled but has no effects.", e.name),
                "plan",
                Some(e.id),
            );
        }
    }
    let active_events: std::collections::HashSet<i64> = g
        .events
        .iter()
        .filter(|e| e.enabled != 0)
        .map(|e| e.id)
        .collect();
    let enabled_effect =
        |e: &&crate::graph::EffectRow| e.event_id.is_some_and(|id| active_events.contains(&id));
    if !g
        .effects
        .values()
        .filter(enabled_effect)
        .any(|e| e.kind == "Expense")
    {
        add(
            Code::MissingSpending,
            "warning",
            "No spending is modeled. This run cannot assess your ability to fund living costs."
                .into(),
            "plan",
            None,
        );
    }
    if g.scenario.funding_strategy.is_none()
        && !g
            .effects
            .values()
            .filter(enabled_effect)
            .any(|e| e.kind == "Sweep" || e.kind == "CashTransfer")
    {
        add(Code::FundingIntent,"warning","No withdrawal or transfer rule is modeled. Spending uses only its named account; review whether this is intentional.".into(),"plan",None);
    }
    if g.scenario.birth_date.is_none() {
        add(
            Code::MissingBirth,
            "warning",
            "No birth date is set. Review age-based retirement and withdrawal assumptions.".into(),
            "plan",
            None,
        );
    }
    if let (Ok(start), Some(Ok(birth))) = (
        g.scenario.start_date.parse::<jiff::civil::Date>(),
        g.scenario
            .birth_date
            .as_ref()
            .map(|s| s.parse::<jiff::civil::Date>()),
    ) {
        let end_age = i64::from(start.year()) + g.scenario.duration_years
            - i64::from(birth.year())
            - i64::from((start.month(), start.day()) < (birth.month(), birth.day()));
        add(
            Code::Horizon,
            "warning",
            format!(
                "The modeled horizon ends around age {end_age}. Confirm that it covers your household's intended planning lifetime."
            ),
            "plan",
            None,
        );
    }
    if let Some((account, year)) = roth_conversion_candidate(g) {
        add(
            Code::RothConversionCandidate,
            "info",
            format!(
                "{account} holds pre-tax money, and its required minimum distributions begin in {year}, inside the plan. Converting some of it to your Roth account in lower-income years before then can lower the tax on those distributions: Add Roth conversions on the Plan tab."
            ),
            "plan",
            None,
        );
    }
    if g.scenario.inflation_profile_id.is_none() {
        add(
            Code::NoInflation,
            "warning",
            "No inflation is modeled. Inflation-adjusted spending will not grow.".into(),
            "plan",
            None,
        );
    }
    add(
        Code::Assumptions,
        "warning",
        format!(
            "Review tax assumptions: {}. Returns and inflation are annual nominal assumptions. Tax modeling is simplified; profile labels retain their original year and filing status. Review omitted household income, benefits and costs before relying on results.",
            g.tax_config
                .as_ref()
                .map(|t| t.name.as_str())
                .unwrap_or("no tax profile")
        ),
        "plan",
        None,
    );
    let can_run = !issues.iter().any(|i| i.severity == "error");
    PreflightReport { issues, can_run }
}

/// A plan a Roth conversion could help: a tax-deferred account with money in
/// it, a Roth to convert into, no conversion yet, and RMDs that begin before
/// the plan ends. The largest such account's name and the first RMD year.
fn roth_conversion_candidate(g: &ScenarioGraph) -> Option<(String, i64)> {
    let start = g.scenario.start_date.parse::<jiff::civil::Date>().ok()?;
    let birth = g
        .scenario
        .birth_date
        .as_deref()?
        .parse::<jiff::civil::Date>()
        .ok()?;
    let first_rmd_year = i64::from(birth.year()) + i64::from(crate::templates::RMD_AGE);
    if first_rmd_year >= i64::from(start.year()) + g.scenario.duration_years {
        return None;
    }
    let status = |status: &'static str| {
        g.investment
            .values()
            .filter(move |i| i.tax_status == status)
            .map(|i| i.account_id)
    };
    // A Roth to convert into.
    status("TaxFree").next()?;
    if g.effects.values().any(|e| e.kind == "RothConversion") {
        return None;
    }
    // Opening value at cost: the plan's prices are not known before a run,
    // and any money at all is reason enough.
    let held = |id: i64| {
        g.investment.get(&id).map_or(0.0, |i| i.cash_value)
            + g.positions
                .get(&id)
                .into_iter()
                .flatten()
                .map(|p| p.cost_basis)
                .sum::<f64>()
    };
    let (account, _) = status("TaxDeferred")
        .map(|id| (id, held(id)))
        .filter(|(_, held)| *held > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let name = g.accounts.iter().find(|a| a.id == account)?.name.clone();
    Some((name, first_rmd_year))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_plan() -> ScenarioGraph {
        serde_json::from_str(include_str!("../testdata/default_snapshot.json")).unwrap()
    }

    fn candidate(g: &ScenarioGraph) -> Option<String> {
        preflight(g)
            .issues
            .into_iter()
            .find(|i| i.code == "roth_conversion_candidate")
            .map(|i| {
                assert_eq!(i.severity, "info");
                i.message
            })
    }

    #[test]
    fn a_pre_tax_balance_with_rmds_inside_the_plan_suggests_conversions() {
        // Born 1996, the plan runs to 2096: RMDs begin in 2069.
        let g = default_plan();
        let message = candidate(&g).expect("a note");
        assert!(message.contains("Fidelity 401(k)") && message.contains("2069"));
        assert!(preflight(&g).can_run);
    }

    #[test]
    fn no_note_without_rmds_in_the_plan_a_roth_or_pre_tax_money_or_with_a_conversion() {
        let mut short = default_plan();
        short.scenario.duration_years = 40;
        assert!(candidate(&short).is_none());

        let mut no_roth = default_plan();
        no_roth.investment.retain(|_, i| i.tax_status != "TaxFree");
        assert!(candidate(&no_roth).is_none());

        let mut empty = default_plan();
        empty.positions.remove(&3);
        empty.investment.get_mut(&3).unwrap().cash_value = 0.0;
        assert!(candidate(&empty).is_none());

        let mut converting = default_plan();
        let effect = converting.effects.values_mut().next().unwrap();
        effect.kind = "RothConversion".into();
        assert!(candidate(&converting).is_none());
    }
}
