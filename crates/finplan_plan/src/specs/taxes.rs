//! Tax config request bodies and the validation both the route and an
//! in-memory edit apply to them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{PlanError, PlanResult};

/// What a second tax config of the same name is refused with.
pub const NAME_TAKEN: &str = "a tax config with that name already exists";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Bracket {
    pub threshold: f64,
    pub rate: f64,
}

/// A tax config of the caller's library, as the tax config routes return it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct TaxConfig {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub state_rate: f64,
    pub early_withdrawal_penalty_rate: f64,
    /// Federal standard deduction, in the brackets' dollars.
    pub standard_deduction: f64,
    /// Added to the deduction from the tax year the person turns 65.
    pub age_65_extra_deduction: f64,
    /// Modified AGI above which the 3.8% net investment income tax applies
    /// to gains; `None` leaves it out. Not indexed to inflation.
    pub niit_threshold: Option<f64>,
    pub federal_brackets: Vec<Bracket>,
    /// The long-term capital gains schedule over taxable income: gains stack
    /// on top of ordinary income and take each band's rate from where they
    /// land. Indexed and moved by the deduction with the federal brackets.
    pub capital_gains_brackets: Vec<Bracket>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateTaxConfig {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub state_rate: f64,
    #[serde(default = "default_penalty")]
    pub early_withdrawal_penalty_rate: f64,
    /// Federal standard deduction for the brackets' year and filing status,
    /// in dollars (default 0). Indexed to inflation with the brackets.
    #[serde(default)]
    pub standard_deduction: f64,
    /// Extra standard deduction from the tax year the person turns 65, in
    /// dollars (default 0); for a married couple, both spouses' together.
    #[serde(default)]
    pub age_65_extra_deduction: f64,
    /// Where the 3.8% net investment income tax starts ($200,000 single,
    /// $250,000 married filing jointly); omitted, it is left out.
    #[serde(default)]
    pub niit_threshold: Option<f64>,
    pub federal_brackets: Vec<Bracket>,
    /// The long-term capital gains brackets for the same year and filing
    /// status, over taxable income (e.g. 0% from 0, 15%, 20%).
    pub capital_gains_brackets: Vec<Bracket>,
}

fn default_penalty() -> f64 {
    0.10
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateTaxConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub state_rate: Option<f64>,
    #[serde(default)]
    pub early_withdrawal_penalty_rate: Option<f64>,
    #[serde(default)]
    pub standard_deduction: Option<f64>,
    #[serde(default)]
    pub age_65_extra_deduction: Option<f64>,
    /// Absent leaves it; `null` removes the net investment income tax.
    #[serde(default, deserialize_with = "crate::specs::double_option")]
    #[ts(optional, type = "number | null")]
    pub niit_threshold: Option<Option<f64>>,
    /// Replaces the whole bracket table when present.
    #[serde(default)]
    pub federal_brackets: Option<Vec<Bracket>>,
    /// Replaces the whole capital gains schedule when present.
    #[serde(default)]
    pub capital_gains_brackets: Option<Vec<Bracket>>,
}

/// The ordinary schedule, as [`validate_brackets`] names it.
pub const FEDERAL: &str = "federal";
/// The long-term gains schedule, as [`validate_brackets`] names it.
pub const CAPITAL_GAINS: &str = "capital gains";

/// The engine walks brackets assuming ascending, gap-free thresholds beginning
/// at zero, so validate that here rather than producing quietly wrong tax.
pub fn validate_brackets(schedule: &str, brackets: &[Bracket]) -> PlanResult<Vec<Bracket>> {
    if brackets.is_empty() {
        return Err(PlanError::invalid(format!(
            "a tax config needs at least one {schedule} bracket"
        )));
    }

    let mut sorted = brackets.to_vec();
    sorted.sort_by(|a, b| {
        a.threshold
            .partial_cmp(&b.threshold)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if sorted[0].threshold != 0.0 {
        return Err(PlanError::invalid(format!(
            "the lowest {schedule} bracket must start at a threshold of 0"
        )));
    }

    for pair in sorted.windows(2) {
        if pair[0].threshold == pair[1].threshold {
            return Err(PlanError::invalid(format!(
                "duplicate {schedule} bracket threshold {}",
                pair[0].threshold
            )));
        }
    }

    for bracket in &sorted {
        if !(0.0..=1.0).contains(&bracket.rate) {
            return Err(PlanError::invalid(format!(
                "{schedule} bracket rate {} is outside 0..1; rates are fractions, not percentages",
                bracket.rate
            )));
        }
    }

    Ok(sorted)
}

/// Deductions and thresholds are dollar amounts: finite and not negative
/// (the table's CHECKs).
pub fn validate_deductions<const N: usize>(deductions: [(&str, Option<f64>); N]) -> PlanResult<()> {
    for (name, amount) in deductions {
        if let Some(amount) = amount
            && !(amount.is_finite() && amount >= 0.0)
        {
            return Err(PlanError::invalid(format!(
                "{name} is a dollar amount of 0 or more"
            )));
        }
    }
    Ok(())
}

/// Rates are fractions: the table's `BETWEEN 0 AND 1` CHECKs.
pub fn validate_rates<const N: usize>(rates: [(&str, Option<f64>); N]) -> PlanResult<()> {
    for (name, rate) in rates {
        if let Some(rate) = rate
            && !(0.0..=1.0).contains(&rate)
        {
            return Err(PlanError::invalid(format!(
                "{name} is a fraction between 0 and 1"
            )));
        }
    }
    Ok(())
}

/// What creating `body` would refuse: rates outside 0..1 and amounts below
/// zero (the table's CHECKs), and a bad bracket table. Returns the federal
/// and capital gains brackets, sorted.
pub fn checked(body: &CreateTaxConfig) -> PlanResult<(Vec<Bracket>, Vec<Bracket>)> {
    validate_deductions([
        ("standard_deduction", Some(body.standard_deduction)),
        ("age_65_extra_deduction", Some(body.age_65_extra_deduction)),
        ("niit_threshold", body.niit_threshold),
    ])?;
    validate_rates([
        ("state_rate", Some(body.state_rate)),
        (
            "early_withdrawal_penalty_rate",
            Some(body.early_withdrawal_penalty_rate),
        ),
    ])?;
    Ok((
        validate_brackets(FEDERAL, &body.federal_brackets)?,
        validate_brackets(CAPITAL_GAINS, &body.capital_gains_brackets)?,
    ))
}
