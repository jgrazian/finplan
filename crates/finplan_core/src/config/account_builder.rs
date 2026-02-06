//! Account Builder DSL
//!
//! Provides a fluent API for creating accounts with preset types and sensible defaults.
//!
//! # Examples
//!
//! ```ignore
//! use finplan::config::AccountBuilder;
//!
//! // Create a taxable brokerage account
//! let brokerage = AccountBuilder::taxable_brokerage("Main Brokerage")
//!     .cash(10_000.0)
//!     .build();
//!
//! // Create a traditional 401k
//! let traditional = AccountBuilder::traditional_401k("Work 401k")
//!     .cash(50_000.0)
//!     .build();
//!
//! // Create a Roth IRA
//! let roth = AccountBuilder::roth_ira("Roth IRA")
//!     .cash(20_000.0)
//!     .build();
//! ```

use crate::model::{
    Account, AccountFlavor, AccountId, AssetId, AssetLot, Cash, FixedAsset, InvestmentContainer,
    LoanDetail, ReturnProfileId, TaxStatus,
};
use jiff::civil::Date;

/// Builder for creating accounts with a fluent API
#[derive(Debug, Clone)]
pub struct AccountBuilder {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    flavor: AccountFlavorBuilder,
}

#[derive(Debug, Clone)]
enum AccountFlavorBuilder {
    Bank {
        cash_value: f64,
        return_profile_id: ReturnProfileId,
    },
    Investment {
        tax_status: TaxStatus,
        cash_value: f64,
        cash_return_profile_id: ReturnProfileId,
        positions: Vec<AssetLot>,
    },
    Property {
        asset: Option<FixedAsset>,
    },
    Liability {
        principal: f64,
        interest_rate: f64,
    },
}

impl AccountBuilder {
    // =========================================================================
    // Preset Account Type Constructors
    // =========================================================================

    /// Create a taxable brokerage account
    ///
    /// Tax treatment: Capital gains taxed when assets sold
    #[must_use]
    pub fn taxable_brokerage(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: None,
            flavor: AccountFlavorBuilder::Investment {
                tax_status: TaxStatus::Taxable,
                cash_value: 0.0,
                cash_return_profile_id: ReturnProfileId(0),
                positions: Vec::new(),
            },
        }
    }

    /// Create a traditional 401k account
    ///
    /// Tax treatment: Contributions tax-deferred, withdrawals taxed as ordinary income
    #[must_use]
    pub fn traditional_401k(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: Some("Traditional 401(k) - Tax-deferred retirement account".into()),
            flavor: AccountFlavorBuilder::Investment {
                tax_status: TaxStatus::TaxDeferred,
                cash_value: 0.0,
                cash_return_profile_id: ReturnProfileId(0),
                positions: Vec::new(),
            },
        }
    }

    /// Create a traditional IRA account
    ///
    /// Tax treatment: Contributions may be tax-deductible, withdrawals taxed as ordinary income
    #[must_use]
    pub fn traditional_ira(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: Some("Traditional IRA - Tax-deferred retirement account".into()),
            flavor: AccountFlavorBuilder::Investment {
                tax_status: TaxStatus::TaxDeferred,
                cash_value: 0.0,
                cash_return_profile_id: ReturnProfileId(0),
                positions: Vec::new(),
            },
        }
    }

    /// Create a Roth 401k account
    ///
    /// Tax treatment: Contributions post-tax, qualified withdrawals tax-free
    #[must_use]
    pub fn roth_401k(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: Some("Roth 401(k) - Tax-free growth retirement account".into()),
            flavor: AccountFlavorBuilder::Investment {
                tax_status: TaxStatus::TaxFree,
                cash_value: 0.0,
                cash_return_profile_id: ReturnProfileId(0),
                positions: Vec::new(),
            },
        }
    }

    /// Create a Roth IRA account
    ///
    /// Tax treatment: Contributions post-tax, qualified withdrawals tax-free
    #[must_use]
    pub fn roth_ira(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: Some("Roth IRA - Tax-free growth retirement account".into()),
            flavor: AccountFlavorBuilder::Investment {
                tax_status: TaxStatus::TaxFree,
                cash_value: 0.0,
                cash_return_profile_id: ReturnProfileId(0),
                positions: Vec::new(),
            },
        }
    }

    /// Create an HSA (Health Savings Account)
    ///
    /// Tax treatment: Triple tax-advantaged (contributions, growth, qualified withdrawals)
    #[must_use]
    pub fn hsa(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: Some("HSA - Triple tax-advantaged health savings account".into()),
            flavor: AccountFlavorBuilder::Investment {
                tax_status: TaxStatus::TaxFree,
                cash_value: 0.0,
                cash_return_profile_id: ReturnProfileId(0),
                positions: Vec::new(),
            },
        }
    }

    /// Create a 529 education savings account
    ///
    /// Tax treatment: Contributions post-tax, qualified education withdrawals tax-free
    #[must_use]
    pub fn education_529(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: Some("529 Plan - Tax-advantaged education savings".into()),
            flavor: AccountFlavorBuilder::Investment {
                tax_status: TaxStatus::TaxFree,
                cash_value: 0.0,
                cash_return_profile_id: ReturnProfileId(0),
                positions: Vec::new(),
            },
        }
    }

    /// Create a checking or savings bank account
    ///
    /// Tax treatment: Interest taxed as ordinary income
    #[must_use]
    pub fn bank_account(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: None,
            flavor: AccountFlavorBuilder::Bank {
                cash_value: 0.0,
                return_profile_id: ReturnProfileId(0),
            },
        }
    }

    /// Create a high-yield savings account
    ///
    /// Tax treatment: Interest taxed as ordinary income
    #[must_use]
    pub fn hysa(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: Some("High-Yield Savings Account".into()),
            flavor: AccountFlavorBuilder::Bank {
                cash_value: 0.0,
                return_profile_id: ReturnProfileId(0),
            },
        }
    }

    /// Create a property/real estate account
    #[must_use]
    pub fn property(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            description: None,
            flavor: AccountFlavorBuilder::Property { asset: None },
        }
    }

    /// Create a mortgage liability
    #[must_use]
    pub fn mortgage(name: impl Into<String>, principal: f64, interest_rate: f64) -> Self {
        Self {
            name: Some(name.into()),
            description: Some("Mortgage liability".into()),
            flavor: AccountFlavorBuilder::Liability {
                principal,
                interest_rate,
            },
        }
    }

    /// Create a student loan liability
    #[must_use]
    pub fn student_loan(name: impl Into<String>, principal: f64, interest_rate: f64) -> Self {
        Self {
            name: Some(name.into()),
            description: Some("Student loan liability".into()),
            flavor: AccountFlavorBuilder::Liability {
                principal,
                interest_rate,
            },
        }
    }

    /// Create a generic loan liability
    #[must_use]
    pub fn loan(name: impl Into<String>, principal: f64, interest_rate: f64) -> Self {
        Self {
            name: Some(name.into()),
            description: None,
            flavor: AccountFlavorBuilder::Liability {
                principal,
                interest_rate,
            },
        }
    }

    // =========================================================================
    // Builder Methods
    // =========================================================================

    /// Set or update the account name
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the account description
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the cash balance (for Bank or Investment accounts)
    #[must_use]
    pub fn cash(mut self, amount: f64) -> Self {
        match &mut self.flavor {
            AccountFlavorBuilder::Bank { cash_value, .. } => {
                *cash_value = amount;
            }
            AccountFlavorBuilder::Investment { cash_value, .. } => {
                *cash_value = amount;
            }
            _ => {
                // Ignore for Property and Liability
            }
        }
        self
    }

    /// Set the return profile for cash (interest rate)
    #[must_use]
    pub fn cash_return_profile(mut self, profile_id: ReturnProfileId) -> Self {
        match &mut self.flavor {
            AccountFlavorBuilder::Bank {
                return_profile_id, ..
            } => {
                *return_profile_id = profile_id;
            }
            AccountFlavorBuilder::Investment {
                cash_return_profile_id,
                ..
            } => {
                *cash_return_profile_id = profile_id;
            }
            _ => {}
        }
        self
    }

    /// Add an asset position to an Investment account
    ///
    /// # Arguments
    /// * `asset_id` - The asset identifier
    /// * `units` - Number of shares/units
    /// * `cost_basis` - Total cost basis for this lot
    /// * `purchase_date` - When the position was purchased
    #[must_use]
    pub fn position(
        mut self,
        asset_id: AssetId,
        units: f64,
        cost_basis: f64,
        purchase_date: Date,
    ) -> Self {
        if let AccountFlavorBuilder::Investment { positions, .. } = &mut self.flavor {
            positions.push(AssetLot {
                asset_id,
                purchase_date,
                units,
                cost_basis,
            });
        }
        self
    }

    /// Set the fixed asset for a Property account
    #[must_use]
    pub fn fixed_asset(mut self, asset_id: AssetId, value: f64) -> Self {
        if let AccountFlavorBuilder::Property { asset } = &mut self.flavor {
            *asset = Some(FixedAsset { asset_id, value });
        }
        self
    }

    /// Build the account with the given ID
    #[must_use]
    pub fn build_with_id(self, account_id: AccountId) -> Account {
        let flavor = match self.flavor {
            AccountFlavorBuilder::Bank {
                cash_value,
                return_profile_id,
            } => AccountFlavor::Bank(Cash {
                value: cash_value,
                return_profile_id,
            }),
            AccountFlavorBuilder::Investment {
                tax_status,
                cash_value,
                cash_return_profile_id,
                positions,
            } => AccountFlavor::Investment(InvestmentContainer {
                tax_status,
                cash: Cash {
                    value: cash_value,
                    return_profile_id: cash_return_profile_id,
                },
                positions,
                contribution_limit: None,
            }),
            AccountFlavorBuilder::Property { asset } => {
                AccountFlavor::Property(asset.unwrap_or(FixedAsset {
                    asset_id: AssetId(0),
                    value: 0.0,
                }))
            }
            AccountFlavorBuilder::Liability {
                principal,
                interest_rate,
            } => AccountFlavor::Liability(LoanDetail {
                principal,
                interest_rate,
            }),
        };

        Account { account_id, flavor }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_taxable_brokerage_builder() {
        let account = AccountBuilder::taxable_brokerage("Main Brokerage")
            .cash(10_000.0)
            .build_with_id(AccountId(1));

        assert_eq!(account.account_id, AccountId(1));
        match account.flavor {
            AccountFlavor::Investment(inv) => {
                assert_eq!(inv.cash.value, 10_000.0);
                assert!(matches!(inv.tax_status, TaxStatus::Taxable));
            }
            _ => panic!("Expected Investment flavor"),
        }
    }

    #[test]
    fn test_roth_ira_builder() {
        let account = AccountBuilder::roth_ira("My Roth")
            .cash(5_000.0)
            .build_with_id(AccountId(2));

        match account.flavor {
            AccountFlavor::Investment(inv) => {
                assert!(matches!(inv.tax_status, TaxStatus::TaxFree));
            }
            _ => panic!("Expected Investment flavor"),
        }
    }

    #[test]
    fn test_bank_account_builder() {
        let account = AccountBuilder::bank_account("Checking")
            .cash(2_500.0)
            .build_with_id(AccountId(3));

        match account.flavor {
            AccountFlavor::Bank(cash) => {
                assert_eq!(cash.value, 2_500.0);
            }
            _ => panic!("Expected Bank flavor"),
        }
    }

    #[test]
    fn test_mortgage_builder() {
        let account =
            AccountBuilder::mortgage("Home Mortgage", 300_000.0, 0.065).build_with_id(AccountId(4));

        match account.flavor {
            AccountFlavor::Liability(loan) => {
                assert_eq!(loan.principal, 300_000.0);
                assert_eq!(loan.interest_rate, 0.065);
            }
            _ => panic!("Expected Liability flavor"),
        }
    }
}
