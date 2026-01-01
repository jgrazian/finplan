//! Tax configuration types
//!
//! Defines tax brackets and configuration for the simulation.
//! The actual tax calculation logic is in the `taxes` module.

use serde::{Deserialize, Serialize};

/// A single bracket in a progressive tax system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxBracket {
    /// Income threshold where this bracket begins
    pub threshold: f64,
    /// Marginal tax rate for income in this bracket (e.g., 0.22 for 22%)
    pub rate: f64,
}

/// Tax configuration for the simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxConfig {
    /// Federal income tax brackets (must be sorted by threshold ascending)
    pub federal_brackets: Vec<TaxBracket>,
    /// Flat state income tax rate (e.g., 0.05 for 5%)
    pub state_rate: f64,
    /// Long-term capital gains tax rate (e.g., 0.15 for 15%)
    pub capital_gains_rate: f64,
    /// Estimated percentage of taxable account withdrawals that are gains (0.0 to 1.0)
    /// Used as a simplification instead of full cost basis tracking
    pub taxable_gains_percentage: f64,
}

impl Default for TaxConfig {
    /// Returns a reasonable default based on 2024 US federal brackets (single filer)
    fn default() -> Self {
        Self {
            federal_brackets: vec![
                TaxBracket {
                    threshold: 0.0,
                    rate: 0.10,
                },
                TaxBracket {
                    threshold: 11_600.0,
                    rate: 0.12,
                },
                TaxBracket {
                    threshold: 47_150.0,
                    rate: 0.22,
                },
                TaxBracket {
                    threshold: 100_525.0,
                    rate: 0.24,
                },
                TaxBracket {
                    threshold: 191_950.0,
                    rate: 0.32,
                },
                TaxBracket {
                    threshold: 243_725.0,
                    rate: 0.35,
                },
                TaxBracket {
                    threshold: 609_350.0,
                    rate: 0.37,
                },
            ],
            state_rate: 0.05,
            capital_gains_rate: 0.15,
            taxable_gains_percentage: 0.50,
        }
    }
}

/// Summary of taxes for a single year
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaxSummary {
    pub year: i16,
    /// Income from TaxDeferred account withdrawals (taxed as ordinary income)
    pub ordinary_income: f64,
    /// Realized capital gains from Taxable account withdrawals
    pub capital_gains: f64,
    /// Withdrawals from TaxFree accounts (not taxed)
    pub tax_free_withdrawals: f64,
    /// Total federal tax owed
    pub federal_tax: f64,
    /// Total state tax owed
    pub state_tax: f64,
    /// Total tax owed (federal + state + capital gains)
    pub total_tax: f64,
}
