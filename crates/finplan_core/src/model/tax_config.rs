//! Tax configuration types
//!
//! Defines tax brackets and configuration for the simulation.
//! The actual tax calculation logic is in the `taxes` module.

use serde::{Deserialize, Serialize};

/// A single bracket in a progressive tax system
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
    /// Early withdrawal penalty rate for retirement accounts (default 0.10 for 10%)
    /// Applies to withdrawals from `TaxDeferred` accounts before age 59.5
    #[serde(default = "default_early_withdrawal_penalty_rate")]
    pub early_withdrawal_penalty_rate: f64,
    /// Federal standard deduction, in the same dollars as the brackets. Each
    /// year's first dollars of ordinary income up to it are untaxed.
    #[serde(default)]
    pub standard_deduction: f64,
    /// Additional standard deduction from the tax year the filer turns 65.
    /// Needs a birth date; for a couple, enter the total for both spouses.
    #[serde(default)]
    pub age_65_extra_deduction: f64,
}

fn default_early_withdrawal_penalty_rate() -> f64 {
    0.10
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
            early_withdrawal_penalty_rate: 0.10,
            standard_deduction: 0.0,
            age_65_extra_deduction: 0.0,
        }
    }
}

impl TaxConfig {
    /// The federal brackets over gross ordinary income: `deduction` becomes a
    /// 0% band at the bottom and every threshold moves up by it, which taxes
    /// income exactly as the brackets would tax `income - deduction`.
    #[must_use]
    pub fn brackets_with_deduction(brackets: &[TaxBracket], deduction: f64) -> Vec<TaxBracket> {
        if deduction <= 0.0 {
            return brackets.to_vec();
        }
        std::iter::once(TaxBracket {
            threshold: 0.0,
            rate: 0.0,
        })
        .chain(brackets.iter().map(|b| TaxBracket {
            threshold: b.threshold + deduction,
            rate: b.rate,
        }))
        .collect()
    }
}

/// Summary of taxes for a single year
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct TaxSummary {
    pub year: i16,
    /// Income from `TaxDeferred` account withdrawals (taxed as ordinary income)
    pub ordinary_income: f64,
    /// Realized capital gains from Taxable account withdrawals
    pub capital_gains: f64,
    /// Withdrawals from `TaxFree` accounts (not taxed)
    pub tax_free_withdrawals: f64,
    /// Total federal tax owed
    pub federal_tax: f64,
    /// Total state tax owed
    pub state_tax: f64,
    /// Total tax owed (federal + state + capital gains)
    pub total_tax: f64,
    /// Early withdrawal penalties from retirement accounts (before age 59.5)
    pub early_withdrawal_penalties: f64,
}
