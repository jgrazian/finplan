//! Account and Asset definitions
//!
//! Accounts are containers for assets with specific tax treatments.
//! Assets represent individual investments or property within accounts.

use std::collections::HashMap;

use crate::model::Market;

use super::ids::{AccountId, AssetId, ReturnProfileId};
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts")]
use ts_rs::TS;

/// Period type for contribution limits
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum ContributionLimitPeriod {
    Monthly,
    Yearly,
}

/// Contribution limit configuration for an account
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct ContributionLimit {
    /// Maximum contribution per period
    pub amount: f64,
    /// Period type for the limit
    pub period: ContributionLimitPeriod,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Cash {
    pub value: f64,
    pub return_profile_id: ReturnProfileId,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct FixedAsset {
    pub asset_id: AssetId,
    pub value: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LoanDetail {
    pub principal: f64, // The amount owed (store as positive, treat as negative in calc)
    pub interest_rate: f64, // Useful for projections
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
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
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
    /// (Uses the "InvestmentContainer" from the previous answer)
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
            AccountFlavor::Property(asset) => {
                // Use Market to get current value if asset is registered, otherwise use static value
                market
                    .get_asset_value(start_date, current_date, asset.asset_id)
                    .unwrap_or(asset.value)
            }
            AccountFlavor::Liability(loan) => -loan.principal,
        }
    }

    pub fn cash_balance(&self) -> Option<f64> {
        match &self.flavor {
            AccountFlavor::Bank(cash) => Some(cash.value),
            AccountFlavor::Investment(inv) => Some(inv.cash.value),
            AccountFlavor::Property(_) => None,
            AccountFlavor::Liability(_) => None,
        }
    }

    pub fn is_liquid(&self) -> bool {
        match &self.flavor {
            AccountFlavor::Bank { .. } => true,
            AccountFlavor::Investment { .. } => true,
            AccountFlavor::Property { .. } => false,
            AccountFlavor::Liability { .. } => false,
        }
    }

    pub fn snapshot(
        &self,
        market: &Market,
        start_date: Date,
        current_date: Date,
    ) -> AccountSnapshot {
        let flavor = match &self.flavor {
            AccountFlavor::Bank(cash) => AccountSnapshotFlavor::Bank(cash.value),
            AccountFlavor::Investment(inv) => {
                let mut assets: HashMap<AssetId, f64> = HashMap::new();

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
            AccountFlavor::Property(asset) => {
                // Use Market to get current value if asset is registered, otherwise use static value
                let value = market
                    .get_asset_value(start_date, current_date, asset.asset_id)
                    .unwrap_or(asset.value);
                AccountSnapshotFlavor::Property(value)
            }
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
        assets: HashMap<AssetId, f64>,
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
