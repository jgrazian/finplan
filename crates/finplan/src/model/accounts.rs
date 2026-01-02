//! Account and Asset definitions
//!
//! Accounts are containers for assets with specific tax treatments.
//! Assets represent individual investments or property within accounts.

use super::ids::{AccountId, AssetId};
use serde::{Deserialize, Serialize};

/// Classification of an asset for valuation behavior
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetClass {
    /// Cash, money market - fully liquid, no capital gains
    Cash,
    /// Stocks, bonds, mutual funds - liquid and investable
    Investable,
    /// Property value - typically illiquid
    RealEstate,
    /// Cars, boats, equipment - lose value over time
    Depreciating,
    /// Loans, mortgages - value should be negative
    Liability,
}

/// An individual asset within an account
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Asset {
    pub asset_id: AssetId,
    pub asset_class: AssetClass,
    pub initial_value: f64,
    /// Index into the simulation's return_profiles vector
    pub return_profile_index: usize,
    /// Initial cost basis (defaults to initial_value if not specified)
    /// Only relevant for Taxable accounts
    #[serde(default)]
    pub initial_cost_basis: Option<f64>,
}

/// Tax treatment for an account
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AccountType {
    /// Regular brokerage - capital gains taxed
    Taxable,
    /// 401k, Traditional IRA - contributions tax-deferred, withdrawals taxed as income
    TaxDeferred,
    /// Roth IRA, Roth 401k - contributions post-tax, withdrawals tax-free
    TaxFree,
    /// Real estate, vehicles - not liquid for withdrawal purposes
    Illiquid,
}

/// A container for assets with a specific tax treatment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub assets: Vec<Asset>,
}
