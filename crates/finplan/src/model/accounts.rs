//! Account and Asset definitions
//!
//! Accounts are containers for assets with specific tax treatments.
//! Assets represent individual investments or property within accounts.

use crate::model::Market;

use super::ids::{AccountId, AssetId, ReturnProfileId};
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
    /// Renamed from "Illiquid" to be more precise
    Property(Vec<FixedAsset>),

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
                    }) // Replace with current price lookup
                    .sum();
                inv.cash.value + assets_val
            }
            AccountFlavor::Property(assets) => assets.iter().map(|a| a.value).sum(),
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

    // pub fn asset_balance(&self, asset_id: AssetId) -> Option<f64> {
    //     match &self.flavor {
    //         AccountFlavor::Bank(_) => None,
    //         AccountFlavor::Investment(inv) => inv
    //             .positions
    //             .iter()
    //             .find(|a| a.asset_id == asset_id)
    //             .map(|a| a.current_value()),
    //         AccountFlavor::Property(assets) => assets
    //             .iter()
    //             .find(|a| a.asset_id == asset_id)
    //             .map(|a| a.value),
    //     }
    // }

    // pub fn assets(&self) -> Vec<AssetId> {
    //     match &self.flavor {
    //         AccountFlavor::Bank(_) => vec![],
    //         AccountFlavor::Investment(inv) => inv.positions.iter().map(|a| a.asset_id).collect(),
    //         AccountType::TaxDeferred { assets, .. } | AccountType::TaxFree { assets, .. } => {
    //             assets.iter().map(|a| a.asset_id).collect()
    //         }
    //         AccountType::Illiquid { assets } => assets.iter().map(|a| a.asset_id).collect(),
    //     }
    // }

    pub fn is_liquid(&self) -> bool {
        match &self.flavor {
            AccountFlavor::Bank { .. } => true,
            AccountFlavor::Investment { .. } => true,
            AccountFlavor::Property { .. } => false,
            AccountFlavor::Liability { .. } => false,
        }
    }
}
