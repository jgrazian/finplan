//! The standard plan checks, as data: every check a review makes, in one
//! list, whichever layer makes it (spec 21, part 4).
//!
//! A plan is checked in three places: preflight before a run
//! (`crate::preflight`, one entry per [`Code`](crate::preflight::Code)), the
//! deterministic rules on every review (one entry per rule `super::review`
//! runs), and the AI reviewer, which owns the checks no rule can make
//! ([`CheckLayer::Reviewer`]: they need judgment or a preview). A test holds
//! the Preflight and Rule entries to the codes and rules that exist, both
//! ways.
//!
//! The reviewer's prompt and the Review tab's "What Review checks" read the
//! catalogue through [`review_checks`], with each rule's notes for the review
//! counted, so "the rule looked and found nothing" reads differently from
//! "no rule covers this".

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Kind, Section};

/// Which layer makes a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CheckLayer {
    /// Before every run (`crate::preflight`): warnings on the Run button.
    Preflight,
    /// A deterministic rule, on every review.
    Rule,
    /// The AI reviewer's own checklist: no rule makes it.
    Reviewer,
}

/// One standard check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanCheck {
    /// The rule (`Draft::rule`) or preflight code; for a reviewer check, a
    /// name of the same shape.
    pub id: &'static str,
    pub layer: CheckLayer,
    pub section: Section,
    pub kind: Kind,
    /// What it looks for, in one sentence.
    pub looks_for: &'static str,
    /// The path it proposes, or "none".
    pub offers: &'static str,
}

const fn check(
    id: &'static str,
    layer: CheckLayer,
    section: Section,
    kind: Kind,
    looks_for: &'static str,
    offers: &'static str,
) -> PlanCheck {
    PlanCheck {
        id,
        layer,
        section,
        kind,
        looks_for,
        offers,
    }
}

use CheckLayer::{Preflight, Reviewer, Rule};
use Kind::{Check, Fix, Read, Stress};
use Section::{Plan, Portfolio, Results};

const CATALOGUE: &[PlanCheck] = &[
    // ── preflight ──
    check(
        "invalid_plan",
        Preflight,
        Plan,
        Fix,
        "The plan does not compile, so it cannot run.",
        "none",
    ),
    check(
        "unmapped_asset",
        Preflight,
        Portfolio,
        Check,
        "An asset with no return assumption, whose price stays fixed.",
        "none",
    ),
    check(
        "empty_event",
        Preflight,
        Plan,
        Check,
        "An enabled event with no effects.",
        "none",
    ),
    check(
        "missing_spending",
        Preflight,
        Plan,
        Check,
        "No expense anywhere in the plan.",
        "none",
    ),
    check(
        "funding_intent",
        Preflight,
        Plan,
        Check,
        "Spending, but no sweep, transfer or funding policy to pay for it.",
        "none",
    ),
    check(
        "missing_birth",
        Preflight,
        Plan,
        Check,
        "No birth date, so ages and RMDs are guesses.",
        "none",
    ),
    check(
        "horizon",
        Preflight,
        Plan,
        Check,
        "The age the plan ends at, to confirm against a planning lifetime.",
        "none",
    ),
    check(
        "no_inflation",
        Preflight,
        Plan,
        Check,
        "No inflation profile, so inflation-adjusted amounts never grow.",
        "none",
    ),
    check(
        "assumptions",
        Preflight,
        Plan,
        Check,
        "The tax, return and inflation assumptions to confirm.",
        "none",
    ),
    check(
        "roth_conversion_candidate",
        Preflight,
        Plan,
        Read,
        "Pre-tax money whose RMDs begin inside the plan, with a Roth account and no conversion yet.",
        "none (points at Add Roth conversions on the Plan tab)",
    ),
    // ── rules ──
    check(
        "cost_basis_equals_value",
        Rule,
        Portfolio,
        Check,
        "Taxable lots that all cost exactly today's price, which assumes no embedded gains.",
        "none",
    ),
    check(
        "idle_bank_cash",
        Rule,
        Portfolio,
        Check,
        "A bank account holding over two years of spending from the plan's start, year after \
         year, on the shown path.",
        "none",
    ),
    check(
        "cash_accumulates",
        Rule,
        Portfolio,
        Fix,
        "Cash, in a bank or uninvested in an investment account, that builds up above two years \
         of spending for three or more year-ends after the plan begins, typically from the first \
         RMD year.",
        "A yearly Reinvest cash event that invests the cash above two years of spending in the \
         largest taxable account's holdings, or in place.",
    ),
    check(
        "unused_contribution_limits",
        Rule,
        Portfolio,
        Check,
        "Accounts with a contribution limit that nothing pays into, in a plan with income.",
        "none",
    ),
    check(
        "unmapped_or_mismatched_assets",
        Rule,
        Portfolio,
        Check,
        "Held assets whose return model does not fit them: unmapped, a single stock moving \
         exactly like an index, a fund on another class's profile.",
        "none",
    ),
    check(
        "sweep_sells_while_cash",
        Rule,
        Plan,
        Fix,
        "A one-off sweep that sells investments into an account already holding the expense \
         it pays.",
        "Remove the sweep and pay from the cash.",
    ),
    check(
        "liability_payment_inflation_adjusted",
        Rule,
        Plan,
        Fix,
        "A loan payment that grows with inflation.",
        "Fix the payment in dollars, or at the loan's amortizing payment.",
    ),
    check(
        "rmd_missing",
        Rule,
        Plan,
        Fix,
        "Tax-deferred money at the RMD age inside the plan, but no event that applies RMDs, so \
         the run never takes or taxes them.",
        "A yearly Apply RMD event from the RMD age into the main bank account.",
    ),
    check(
        "rmd_into_investment_cash",
        Rule,
        Plan,
        Check,
        "An Apply RMD that pays into an investment account, where the cash sits unspent and \
         uninvested.",
        "Pay the RMD into the main bank account, or invest the cash in place with a yearly \
         Reinvest cash event.",
    ),
    check(
        "roth_conversion_opportunity",
        Rule,
        Plan,
        Fix,
        "Pre-tax money whose RMDs fall due inside the plan, after years with ordinary income \
         below the top of the 22% bracket, where the RMDs outrun spending or are taxed at a \
         higher marginal rate, and no Roth conversion yet.",
        "Yearly Roth conversions (the Roth conversions template) up to the 12% or the 22% \
         bracket until the year before RMDs, tax paid from the largest taxable account, \
         opening a Roth first when there is none. Judged on after-tax ending balance and \
         lifetime tax, not success rate.",
    ),
    check(
        "shortfall_account_concentration",
        Rule,
        Results,
        Read,
        "Funding failures concentrated in one account, or failing paths that still end with \
         money.",
        "none",
    ),
    check(
        "success_vs_funding_gap",
        Rule,
        Results,
        Read,
        "A success rate well above funding success, or one carried by illiquid property.",
        "none",
    ),
    // ── the reviewer's checklist ──
    check(
        "missing_social_security",
        Reviewer,
        Plan,
        Check,
        "Decades of earnings but no Social Security income.",
        "A Social Security income from 67, estimated with estimate_social_security.",
    ),
    check(
        "retirement_spending_shape",
        Reviewer,
        Plan,
        Check,
        "Retirement spending that never changes, with no health care or late-life costs.",
        "Spending events for what is missing, labelled as estimates.",
    ),
    check(
        "early_withdrawal_penalties",
        Reviewer,
        Plan,
        Fix,
        "Early-withdrawal penalties on the shown path above about 1% of lifetime spending \
         (the run's penalties line).",
        "The Penalty-aware withdrawal order on the sweeps that pay for spending (the funding \
         policy, a plan setting, when it is what sells), or a conversion ladder: Roth \
         conversions started five years before the penalized withdrawals.",
    ),
    check(
        "early_retirement_stress",
        Reviewer,
        Results,
        Stress,
        "How the plan fares through a market drop in the first retired year, high inflation \
         or lost income.",
        "A stress event (market crash, inflation, job loss), applied to a copy.",
    ),
];

/// Every standard check: preflight first, then the rules, then the
/// reviewer's checklist.
pub fn catalogue() -> &'static [PlanCheck] {
    CATALOGUE
}

/// One standard check and what it came to in one review, as the Review tab
/// and the reviewer's prompt read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewCheck {
    pub id: String,
    pub layer: CheckLayer,
    pub section: Section,
    pub kind: Kind,
    pub looks_for: String,
    pub offers: String,
    /// A rule's notes in this review: 0 when it ran and found nothing. Null
    /// for preflight and reviewer checks, which a review does not count.
    pub notes: Option<i64>,
}

/// The catalogue with each rule's notes counted: `rules` holds the `rule` of
/// every note the review's rules wrote (one entry per note).
pub fn review_checks<'a>(rules: impl IntoIterator<Item = &'a str>) -> Vec<ReviewCheck> {
    let mut counts = std::collections::HashMap::<&str, i64>::new();
    for rule in rules {
        *counts.entry(rule).or_default() += 1;
    }
    CATALOGUE
        .iter()
        .map(|c| ReviewCheck {
            id: c.id.to_string(),
            layer: c.layer,
            section: c.section,
            kind: c.kind,
            looks_for: c.looks_for.to_string(),
            offers: c.offers.to_string(),
            notes: (c.layer == Rule).then(|| counts.get(c.id).copied().unwrap_or(0)),
        })
        .collect()
}
