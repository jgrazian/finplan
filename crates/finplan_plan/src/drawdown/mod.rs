//! The Drawdown projection (spec 20, part 2): which accounts pay for
//! retirement, year by year, under each withdrawal strategy.
//!
//! Re-simulates a run's input snapshot on the median path's seed with each
//! strategy swapped in, so every row sees the same market and only the
//! withdrawals differ. [`project`] folds one path's ledger per strategy into
//! yearly rows; [`compare()`] runs a small Monte Carlo per strategy on a common
//! seed. Pure: no I/O, builds for wasm.

mod compare;
mod normalize;
mod project;
mod retirement;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{PlanError, PlanResult};
use crate::specs::WithdrawalStrategy;
use crate::specs::scenarios::FundingPolicySpec;

pub use compare::{DEFAULT_COMPARE_ITERATIONS, MAX_COMPARE_ITERATIONS, compare};
pub use normalize::{FixedSweep, normalize};
pub use project::project;

/// Most strategies one request may ask for.
pub const MAX_CHOICES: usize = 8;

/// The ceiling Bracket filling uses in the default list.
const DEFAULT_CEILING: f64 = 0.12;

/// One row of the comparison: the plan as it is, or a strategy swapped in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum StrategyChoice {
    /// The snapshot unchanged: no overlay, no swapped sweeps.
    AsPlanned,
    Strategy {
        strategy: WithdrawalStrategy,
        /// `BracketFilling` only; absent means 12%.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        bracket_ceiling: Option<f64>,
    },
}

impl StrategyChoice {
    fn check(&self) -> PlanResult<()> {
        let Self::Strategy {
            strategy,
            bracket_ceiling: Some(rate),
        } = self
        else {
            return Ok(());
        };
        if !matches!(strategy, WithdrawalStrategy::BracketFilling) {
            return Err(PlanError::invalid(
                "bracket_ceiling only applies to the BracketFilling strategy",
            ));
        }
        if !(0.0..1.0).contains(rate) {
            return Err(PlanError::invalid(
                "bracket_ceiling must be a rate from 0 up to (not including) 1",
            ));
        }
        Ok(())
    }
}

/// `POST /runs/{id}/drawdown`. `{}` is the default list from the plan's own
/// retirement date.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, optional_fields = nullable)]
pub struct DrawdownRequest {
    /// Absent means the default list: As planned, then the six strategies.
    pub strategies: Option<Vec<StrategyChoice>>,
    /// Overrides the retirement date the plan implies.
    pub retirement_year: Option<i64>,
}

/// As planned, then the six engine strategies (Bracket filling at 12%).
#[must_use]
pub fn default_choices() -> Vec<StrategyChoice> {
    let plain = |strategy| StrategyChoice::Strategy {
        strategy,
        bracket_ceiling: None,
    };
    vec![
        StrategyChoice::AsPlanned,
        plain(WithdrawalStrategy::TaxEfficientEarly),
        plain(WithdrawalStrategy::TaxDeferredFirst),
        plain(WithdrawalStrategy::TaxFreeFirst),
        plain(WithdrawalStrategy::ProRata),
        plain(WithdrawalStrategy::PenaltyAware),
        StrategyChoice::Strategy {
            strategy: WithdrawalStrategy::BracketFilling,
            bracket_ceiling: Some(DEFAULT_CEILING),
        },
    ]
}

impl DrawdownRequest {
    /// The choices to run, checked.
    fn choices(&self) -> PlanResult<Vec<StrategyChoice>> {
        let choices = self.strategies.clone().unwrap_or_else(default_choices);
        if choices.is_empty() || choices.len() > MAX_CHOICES {
            return Err(PlanError::invalid(format!(
                "ask for between 1 and {MAX_CHOICES} strategies"
            )));
        }
        for choice in &choices {
            choice.check()?;
        }
        if let Some(year) = self.retirement_year
            && !(1900..=2200).contains(&year)
        {
            return Err(PlanError::invalid(
                "retirement_year must be between 1900 and 2200",
            ));
        }
        Ok(choices)
    }
}

/// Where the retirement year came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RetirementSource {
    /// `DrawdownRequest::retirement_year`.
    Request,
    /// The plan's Age parameter named "retire…", at the birth date.
    Parameter,
    /// The year after the last non-benefit income landed.
    Income,
    /// Nothing to go on: the plan's first year.
    Start,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct RetirementInfo {
    pub year: i64,
    /// With a birth date: the age at the retirement date.
    pub age: Option<f64>,
    pub source: RetirementSource,
}

/// An account that has a balance in the rows. Per-year arrays are aligned to
/// the body's `accounts`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct DrawdownAccount {
    pub id: i64,
    pub name: String,
    /// `"Bank"` or `"Investment"`.
    pub flavor: String,
    /// Investment accounts: `"Taxable"`, `"TaxDeferred"` or `"TaxFree"`, for
    /// colouring.
    pub tax_status: Option<String>,
}

/// An event that paid income in the rows. Per-year `income` is aligned to the
/// body's `income_sources`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DrawdownIncomeSource {
    pub event_id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MarkerKind {
    /// First year an income event paid; `id` is the event.
    Income,
    /// First year an account paid a required distribution; `id` is the account.
    Rmd,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DrawdownMarker {
    pub kind: MarkerKind,
    pub id: i64,
    pub year: i64,
}

/// One calendar year of one path, from the retirement year on. Dollars are
/// nominal; divide by `inflation` for Today's $.
///
/// ```text
/// sum(income) + sum(withdrawals) + cash + shortfall
///     = spending + withdrawal_taxes + surplus      (within $1)
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DrawdownYear {
    pub year: i64,
    /// The path's cumulative inflation factor for the year.
    pub inflation: f64,
    /// Expenses paid (taxes are not spending).
    pub spending: f64,
    /// Income credited, per `income_sources`.
    pub income: Vec<f64>,
    /// Gross proceeds of investment sales, per `accounts`.
    pub withdrawals: Vec<f64>,
    /// Required distributions (net), per `accounts`; a subset of `withdrawals`.
    pub rmd: Vec<f64>,
    /// Tax and penalties withheld from sale proceeds.
    pub withdrawal_taxes: f64,
    /// Drawn from bank balances: whatever else covered spending.
    pub cash: f64,
    /// Money taken out beyond need (RMDs); went to the bank balances.
    pub surplus: f64,
    /// Increase in the total of overdrawn bank balances.
    pub shortfall: f64,
    /// Year-end balance, per `accounts`.
    pub balances: Vec<f64>,
    /// All tax for the year: income, capital gains, state, plus penalties.
    pub total_tax: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct DrawdownSummary {
    /// Over the years shown (retirement year to the end).
    pub lifetime_spending: f64,
    pub lifetime_tax: f64,
    /// Final net worth, nominal and in the start year's dollars.
    pub ending_balance: f64,
    pub ending_balance_real: f64,
    pub first_shortfall_year: Option<i64>,
    pub markers: Vec<DrawdownMarker>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DrawdownChoice {
    pub choice: StrategyChoice,
    /// The strategy ran as an overlay: the plan has no funding policy, so one
    /// was installed from the retirement date.
    pub overlay: bool,
    pub summary: DrawdownSummary,
    pub years: Vec<DrawdownYear>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct DrawdownBody {
    /// The path's seed, decimal (a `u64` does not fit a JS number).
    pub seed: String,
    pub retirement: RetirementInfo,
    pub birth_year: Option<i64>,
    pub accounts: Vec<DrawdownAccount>,
    pub income_sources: Vec<DrawdownIncomeSource>,
    /// Events whose sweeps always sell from the same place, whatever the
    /// strategy.
    pub fixed_sweeps: Vec<FixedSweep>,
    /// The plan's own funding policy, so the screen can mark its chip.
    pub plan_funding: Option<FundingPolicySpec>,
    pub choices: Vec<DrawdownChoice>,
}

/// `POST /runs/{id}/drawdown/compare`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, optional_fields = nullable)]
pub struct CompareRequest {
    pub request: Option<DrawdownRequest>,
    /// Per strategy; default 200, at most 500.
    pub iterations: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ComparisonRow {
    pub choice: StrategyChoice,
    pub overlay: bool,
    pub success_rate: f64,
    pub funding_success_rate: Option<f64>,
    /// Median of final net worth across the iterations, nominal.
    pub median_final_net_worth: f64,
    /// The same, in the start year's dollars (deflated by the median path's
    /// own inflation).
    pub median_final_net_worth_real: Option<f64>,
    /// Lifetime tax on the median path (the iteration whose final net worth is
    /// the median); the Monte Carlo itself keeps no per-iteration tax.
    pub median_path_tax: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DrawdownComparison {
    pub iterations: i64,
    pub retirement: RetirementInfo,
    pub rows: Vec<ComparisonRow>,
}
