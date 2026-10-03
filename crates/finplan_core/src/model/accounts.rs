//! Account and Asset definitions
//!
//! Accounts are containers for assets with specific tax treatments.
//! Assets represent individual investments or property within accounts.

use std::collections::BTreeMap;

use crate::model::Market;

use super::ids::{AccountId, AssetId, ReturnProfileId};
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// Period type for contribution limits
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContributionLimitPeriod {
    Monthly,
    Yearly,
}

/// Contribution limit configuration for an account
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContributionLimit {
    /// Maximum contribution per period
    pub amount: f64,
    /// Period type for the limit
    pub period: ContributionLimitPeriod,
    /// Extra room by age, on top of `amount`. Tiers do not stack: when more
    /// than one applies, the largest wins, which is how the 401(k) age 60-63
    /// catch-up replaces the age-50 one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub catch_up: Vec<CatchUp>,
}

impl ContributionLimit {
    #[must_use]
    pub fn yearly(amount: f64) -> Self {
        Self {
            amount,
            period: ContributionLimitPeriod::Yearly,
            catch_up: Vec::new(),
        }
    }

    #[must_use]
    pub fn monthly(amount: f64) -> Self {
        Self {
            amount,
            period: ContributionLimitPeriod::Monthly,
            catch_up: Vec::new(),
        }
    }

    /// Add `amount` of room for anyone turning `from_age` through
    /// `through_age` (inclusive, `None` for no upper bound) during the year.
    #[must_use]
    pub fn with_catch_up(mut self, from_age: u8, through_age: Option<u8>, amount: f64) -> Self {
        self.catch_up.push(CatchUp {
            from_age,
            through_age,
            amount,
        });
        self
    }

    /// The extra room for someone who reaches `age` by the end of the year.
    #[must_use]
    pub fn catch_up_for_age(&self, age: i16) -> f64 {
        self.catch_up
            .iter()
            .filter(|c| c.applies_at(age))
            .map(|c| c.amount)
            .fold(0.0, f64::max)
    }

    /// The whole limit for someone who reaches `age` by the end of the year.
    #[must_use]
    pub fn amount_for_age(&self, age: i16) -> f64 {
        self.amount + self.catch_up_for_age(age)
    }
}

/// Extra contribution room by age, such as the 401(k) catch-up at 50.
///
/// Eligibility follows the IRS rule: the age reached by December 31 of the
/// contribution year counts, so someone turning 50 in November has the full
/// catch-up from January.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CatchUp {
    pub from_age: u8,
    /// Last eligible age, inclusive. `None` means no upper bound.
    #[serde(default)]
    pub through_age: Option<u8>,
    /// Extra room per period
    pub amount: f64,
}

impl CatchUp {
    #[must_use]
    pub fn applies_at(&self, age: i16) -> bool {
        age >= i16::from(self.from_age) && self.through_age.is_none_or(|t| age <= i16::from(t))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Cash {
    pub value: f64,
    pub return_profile_id: ReturnProfileId,
}

/// A single fixed asset — a house, a car — held in a Property account.
///
/// `value` is stated at the asset's plan-start price level: what the holding
/// is worth today is `value` times the asset's growth since the plan began
/// (see [`Market::asset_growth`]). Held that way, a balance adjustment made
/// mid-plan is converted into the same terms, so it counts from the moment it
/// lands and appreciates from there — a house bought at 35 is worth its price
/// at 35, not its price grown from the plan's start.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct FixedAsset {
    pub asset_id: AssetId,
    pub value: f64,
    /// What was paid for the holding, for the gain on a `SellProperty`. `None`
    /// on a property the plan opens with means "no gain before the plan":
    /// the simulation takes its opening value as the basis.
    #[serde(default)]
    pub cost_basis: Option<f64>,
}

impl FixedAsset {
    /// What the asset is worth on `current_date`.
    #[must_use]
    pub fn current_value(&self, market: &Market, start_date: Date, current_date: Date) -> f64 {
        self.value
            * market
                .asset_growth(start_date, current_date, self.asset_id)
                .unwrap_or(1.0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LoanDetail {
    pub principal: f64, // The amount owed (store as positive, treat as negative in calc)
    pub interest_rate: f64, // Useful for projections
    /// How the loan pays itself down. `None` leaves it to explicit transfers:
    /// interest accrues and nothing is repaid unless an event pays it.
    #[serde(default)]
    pub repayment: Option<Repayment>,
    /// The payment the engine is currently making, worked out when the loan is
    /// funded — at plan start for a loan the plan opens with, or by the
    /// `BuyProperty` that draws it. Runtime state, never configured.
    #[serde(skip)]
    pub schedule: Option<PaymentSchedule>,
}

/// A loan that amortizes: a fixed monthly payment drawn from a cash account.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Repayment {
    /// Cash account the monthly payment is drawn from.
    pub from: AccountId,
    /// Months to pay the principal off in, counted from when the loan is
    /// funded. For a loan the plan opens with, that is the term remaining.
    pub term_months: u32,
}

/// The fixed monthly payment of a funded loan, and where it is in its run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaymentSchedule {
    pub from: AccountId,
    pub monthly_payment: f64,
    /// The day the loan was funded; payment `n` falls `n` months after it.
    pub funded: Date,
    /// Payments made so far.
    pub paid: u32,
}

impl PaymentSchedule {
    #[must_use]
    pub fn next_due(&self) -> Date {
        crate::model::TriggerOffset::Months(i32::try_from(self.paid + 1).unwrap_or(i32::MAX))
            .add_to_date(self.funded)
    }
}

impl LoanDetail {
    /// Fix the monthly payment for the principal owed now, the loan's rate and
    /// its term, and start paying a month after `funded`. A loan with no
    /// repayment, no term or nothing owed gets no schedule.
    ///
    /// The monthly rate is the one equivalent to the annual rate the engine
    /// accrues at, so the payment clears the loan in `term_months` rather than
    /// in some slightly different number of months.
    pub fn start_repayment(&mut self, funded: Date) {
        self.schedule = self
            .repayment
            .filter(|r| r.term_months > 0 && self.principal > 0.005)
            .map(|r| PaymentSchedule {
                from: r.from,
                monthly_payment: amortized_payment(
                    self.principal,
                    self.interest_rate,
                    r.term_months,
                ),
                funded,
                paid: 0,
            });
    }
}

/// The level monthly payment that pays `principal` off in `months` at an
/// annual `rate` compounded the way the engine accrues it.
#[must_use]
pub fn amortized_payment(principal: f64, rate: f64, months: u32) -> f64 {
    let n = f64::from(months.max(1));
    let monthly_rate = (1.0 + rate).powf(1.0 / 12.0) - 1.0;
    if monthly_rate.abs() < 1e-12 {
        principal / n
    } else {
        principal * monthly_rate / (1.0 - (1.0 + monthly_rate).powf(-n))
    }
}

/// A single purchase lot for cost basis tracking
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct AssetLot {
    pub asset_id: AssetId,
    pub purchase_date: jiff::civil::Date,
    /// Number of shares/units (or dollar amount for non-share assets)
    pub units: f64,
    /// Total cost basis for this lot
    pub cost_basis: f64,
}

/// Tax treatment for an account
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaxStatus {
    /// Regular brokerage - capital gains taxed
    Taxable,
    /// 401k, Traditional IRA - contributions tax-deferred, withdrawals taxed as income
    TaxDeferred,
    /// Roth IRA, Roth 401k - contributions post-tax, withdrawals tax-free
    TaxFree,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestmentContainer {
    pub tax_status: TaxStatus,
    pub cash: Cash,
    pub positions: Vec<AssetLot>,
    pub contribution_limit: Option<ContributionLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountFlavor {
    /// LIQUID ASSETS (Positive Value)
    /// Checking, Savings, HYSA
    Bank(Cash),

    /// INVESTMENT ASSETS (Positive Value)
    /// Brokerage, 401k, Roth IRA
    /// (Uses the `InvestmentContainer` from the previous answer)
    Investment(InvestmentContainer),

    /// FIXED ASSETS (Positive Value)
    /// Real Estate, Vehicles, Art, Business Equity
    /// Each property account holds a single fixed asset
    Property(FixedAsset),

    /// LIABILITIES (Negative Value)
    /// Mortgages, Student Loans, Auto Loans, Credit Card Debt
    Liability(LoanDetail),
}

/// A container for assets with a specific tax treatment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub account_id: AccountId,
    pub flavor: AccountFlavor,
}

impl Account {
    #[must_use]
    pub fn total_value(&self, market: &Market, start_date: Date, current_date: Date) -> f64 {
        match &self.flavor {
            // Cash value is compounded incrementally during simulation, just return it
            AccountFlavor::Bank(cash) => cash.value,
            // One match arm handles Taxable, Roth, and Trad IRA!
            AccountFlavor::Investment(inv) => {
                let assets_val: f64 = inv
                    .positions
                    .iter()
                    .map(|p| {
                        p.units
                            * market
                                .get_asset_value(start_date, current_date, p.asset_id)
                                .unwrap_or(0.0)
                    })
                    .sum();
                // Cash is compounded incrementally during simulation
                inv.cash.value + assets_val
            }
            AccountFlavor::Property(asset) => asset.current_value(market, start_date, current_date),
            AccountFlavor::Liability(loan) => -loan.principal,
        }
    }

    #[must_use]
    #[inline]
    pub fn cash_balance(&self) -> Option<f64> {
        match &self.flavor {
            AccountFlavor::Bank(cash) => Some(cash.value),
            AccountFlavor::Investment(inv) => Some(inv.cash.value),
            AccountFlavor::Property(_) => None,
            AccountFlavor::Liability(_) => None,
        }
    }

    #[must_use]
    #[inline]
    pub fn is_liquid(&self) -> bool {
        match &self.flavor {
            AccountFlavor::Bank { .. } => true,
            AccountFlavor::Investment { .. } => true,
            AccountFlavor::Property { .. } => false,
            AccountFlavor::Liability { .. } => false,
        }
    }

    #[must_use]
    pub fn snapshot(
        &self,
        market: &Market,
        start_date: Date,
        current_date: Date,
    ) -> AccountSnapshot {
        let flavor =
            match &self.flavor {
                AccountFlavor::Bank(cash) => AccountSnapshotFlavor::Bank(cash.value),
                AccountFlavor::Investment(inv) => {
                    let mut assets: BTreeMap<AssetId, f64> = BTreeMap::new();

                    for asset in &inv.positions {
                        let value = asset.units
                            * market
                                .get_asset_value(start_date, current_date, asset.asset_id)
                                .unwrap_or(0.0);

                        assets
                            .entry(asset.asset_id)
                            .and_modify(|v| *v += value)
                            .or_insert(value);
                    }

                    AccountSnapshotFlavor::Investment {
                        cash: inv.cash.value,
                        assets,
                    }
                }
                AccountFlavor::Property(asset) => AccountSnapshotFlavor::Property(
                    asset.current_value(market, start_date, current_date),
                ),
                AccountFlavor::Liability(loan) => AccountSnapshotFlavor::Liability(-loan.principal),
            };

        AccountSnapshot {
            account_id: self.account_id,
            flavor,
        }
    }
}

// Point-in-time snapshot of an account's state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountSnapshotFlavor {
    Bank(f64),
    Investment {
        cash: f64,
        /// Ordered so that summing it (`total_value`) adds in the same order on
        /// every run: a randomly seeded `HashMap` made seeded runs differ in
        /// the last bits whenever an account held several assets.
        assets: BTreeMap<AssetId, f64>,
    },
    Property(f64),
    Liability(f64),
}

// Point-in-time snapshot of an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub account_id: AccountId,
    pub flavor: AccountSnapshotFlavor,
}

impl AccountSnapshot {
    #[must_use]
    #[inline]
    pub fn total_value(&self) -> f64 {
        match &self.flavor {
            AccountSnapshotFlavor::Bank(cash) => *cash,
            AccountSnapshotFlavor::Investment { cash, assets } => {
                let assets_val: f64 = assets.values().sum();
                *cash + assets_val
            }
            AccountSnapshotFlavor::Property(value) => *value,
            AccountSnapshotFlavor::Liability(balance) => *balance,
        }
    }
}
