//! Tax calculation utilities for retirement withdrawal modeling
//!
//! This module provides lot-based tax calculations using actual `AssetLot` data
//! for accurate capital gains tracking with proper short-term vs long-term distinction.

use crate::model::{TaxBracket, TaxConfig};

/// Calculate federal income tax using progressive brackets
/// Returns the total tax owed on the given income
pub fn calculate_federal_tax(income: f64, brackets: &[TaxBracket]) -> f64 {
    if income <= 0.0 || brackets.is_empty() {
        return 0.0;
    }

    let mut tax = 0.0;
    let mut prev_threshold = 0.0;

    for (i, bracket) in brackets.iter().enumerate() {
        let next_threshold = brackets
            .get(i + 1)
            .map(|b| b.threshold)
            .unwrap_or(f64::INFINITY);

        if income <= bracket.threshold {
            break;
        }

        let taxable_in_bracket =
            (income.min(next_threshold) - bracket.threshold.max(prev_threshold)).max(0.0);
        tax += taxable_in_bracket * bracket.rate;
        prev_threshold = bracket.threshold;
    }

    tax
}

/// Calculate marginal tax on additional income given existing YTD income
/// This is useful for calculating tax on a withdrawal when there's already
/// been taxable income earlier in the year
pub fn calculate_federal_marginal_tax(
    additional_income: f64,
    ytd_income: f64,
    brackets: &[TaxBracket],
) -> f64 {
    let tax_with_additional = calculate_federal_tax(ytd_income + additional_income, brackets);
    let tax_without = calculate_federal_tax(ytd_income, brackets);
    tax_with_additional - tax_without
}

/// Calculate the gross income needed to achieve a target net income after federal marginal taxes and state taxes
/// This is the inverse of calculate_federal_marginal_tax + state tax
pub fn calculate_gross_from_net(
    net_amount: f64,
    ytd_income: f64,
    federal_brackets: &[TaxBracket], // (threshold, rate)
    state_rate: f64,
) -> f64 {
    // Find which bracket the YTD income falls into
    let mut current_bracket_idx = 0;
    for (i, bracket) in federal_brackets.iter().enumerate() {
        if ytd_income >= bracket.threshold {
            current_bracket_idx = i;
        }
    }

    let mut remaining_net = net_amount;
    let mut gross = 0.0;
    let mut income_cursor = ytd_income;

    // Work through brackets starting from current position
    for i in current_bracket_idx..federal_brackets.len() {
        let federal_rate = federal_brackets[i].rate;
        let next_threshold = federal_brackets
            .get(i + 1)
            .map(|b| b.threshold)
            .unwrap_or(f64::MAX);

        // How much gross income fits in this bracket?
        let bracket_room = next_threshold - income_cursor;

        // Combined effective rate (federal marginal + state flat)
        let combined_rate = federal_rate + state_rate;

        // Net income per dollar of gross in this bracket
        let net_per_gross = 1.0 - combined_rate;

        // How much net can we cover with this bracket's room?
        let max_net_in_bracket = bracket_room * net_per_gross;

        if remaining_net <= max_net_in_bracket {
            // All remaining net fits in this bracket
            gross += remaining_net / net_per_gross;
            break;
        } else {
            // Use up this bracket and move to next
            gross += bracket_room;
            remaining_net -= max_net_in_bracket;
            income_cursor = next_threshold;
        }
    }

    gross
}

// ============================================================================
// Tax Calculation with Actual Gains (Lot-Based)
// ============================================================================

/// Result of calculating taxes on actual realized gains
#[derive(Debug, Clone, Default)]
pub struct RealizedGainsTaxResult {
    /// Federal tax on short-term gains (taxed as ordinary income)
    pub short_term_federal_tax: f64,
    /// Federal tax on long-term gains (at preferential rate)
    pub long_term_federal_tax: f64,
    /// Total federal tax
    pub federal_tax: f64,
    /// State tax on all gains
    pub state_tax: f64,
    /// Total tax owed
    pub total_tax: f64,
}

/// Calculate taxes on actual realized gains from lot-based tracking
///
/// This is the correct way to calculate capital gains tax - using actual
/// realized gains from lot consumption rather than estimates.
///
/// # Arguments
/// * `short_term_gain` - Gains from assets held < 1 year
/// * `long_term_gain` - Gains from assets held >= 1 year
/// * `tax_config` - Tax configuration
/// * `ytd_ordinary_income` - Year-to-date ordinary income for bracket calculation
pub fn calculate_realized_gains_tax(
    short_term_gain: f64,
    long_term_gain: f64,
    tax_config: &TaxConfig,
    ytd_ordinary_income: f64,
) -> RealizedGainsTaxResult {
    // Short-term gains are taxed as ordinary income (marginal rate)
    let short_term_federal_tax = if short_term_gain > 0.0 {
        calculate_federal_marginal_tax(
            short_term_gain,
            ytd_ordinary_income,
            &tax_config.federal_brackets,
        )
    } else {
        0.0
    };

    // Long-term gains use preferential capital gains rate
    let long_term_federal_tax = long_term_gain.max(0.0) * tax_config.capital_gains_rate;

    let federal_tax = short_term_federal_tax + long_term_federal_tax;

    // State tax typically applies to all gains at the same rate
    let state_tax = (short_term_gain.max(0.0) + long_term_gain.max(0.0)) * tax_config.state_rate;

    RealizedGainsTaxResult {
        short_term_federal_tax,
        long_term_federal_tax,
        federal_tax,
        state_tax,
        total_tax: federal_tax + state_tax,
    }
}

// ============================================================================
// Tax Calculation for Tax-Deferred Accounts (Ordinary Income)
// ============================================================================

/// Result of calculating taxes on tax-deferred (ordinary income) withdrawals
#[derive(Debug, Clone, Default)]
pub struct OrdinaryIncomeTaxResult {
    /// Federal income tax
    pub federal_tax: f64,
    /// State income tax
    pub state_tax: f64,
    /// Total tax owed
    pub total_tax: f64,
    /// Net amount after taxes
    pub net_amount: f64,
}

/// Calculate taxes on a withdrawal from a tax-deferred account
///
/// Tax-deferred accounts (Traditional IRA, 401k) are taxed as ordinary income.
pub fn calculate_tax_deferred_withdrawal_tax(
    gross_amount: f64,
    tax_config: &TaxConfig,
    ytd_ordinary_income: f64,
) -> OrdinaryIncomeTaxResult {
    let federal_tax = calculate_federal_marginal_tax(
        gross_amount,
        ytd_ordinary_income,
        &tax_config.federal_brackets,
    );
    let state_tax = gross_amount * tax_config.state_rate;
    let total_tax = federal_tax + state_tax;

    OrdinaryIncomeTaxResult {
        federal_tax,
        state_tax,
        total_tax,
        net_amount: gross_amount - total_tax,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tax_config() -> TaxConfig {
        TaxConfig {
            federal_brackets: vec![
                TaxBracket {
                    threshold: 0.0,
                    rate: 0.10,
                },
                TaxBracket {
                    threshold: 10_000.0,
                    rate: 0.12,
                },
                TaxBracket {
                    threshold: 40_000.0,
                    rate: 0.22,
                },
                TaxBracket {
                    threshold: 90_000.0,
                    rate: 0.24,
                },
            ],
            state_rate: 0.05,
            capital_gains_rate: 0.15,
            early_withdrawal_penalty_rate: 0.10,
        }
    }

    #[test]
    fn test_federal_tax_first_bracket() {
        let config = test_tax_config();
        let tax = calculate_federal_tax(5_000.0, &config.federal_brackets);
        assert!((tax - 500.0).abs() < 0.01, "Expected 500, got {}", tax);
    }

    #[test]
    fn test_federal_tax_multiple_brackets() {
        let config = test_tax_config();
        // $50,000 income:
        // $10,000 at 10% = $1,000
        // $30,000 at 12% = $3,600
        // $10,000 at 22% = $2,200
        // Total = $6,800
        let tax = calculate_federal_tax(50_000.0, &config.federal_brackets);
        assert!((tax - 6_800.0).abs() < 0.01, "Expected 6800, got {}", tax);
    }

    #[test]
    fn test_marginal_tax() {
        let config = test_tax_config();
        // YTD income of $35,000 puts us in the 12% bracket
        // Additional $10,000 should be:
        // $5,000 at 12% = $600
        // $5,000 at 22% = $1,100
        // Total marginal = $1,700
        let marginal = calculate_federal_marginal_tax(10_000.0, 35_000.0, &config.federal_brackets);
        assert!(
            (marginal - 1_700.0).abs() < 0.01,
            "Expected 1700, got {}",
            marginal
        );
    }

    #[test]
    fn test_realized_gains_tax_short_term() {
        let config = test_tax_config();
        // Short-term gains taxed as ordinary income
        let result = calculate_realized_gains_tax(10_000.0, 0.0, &config, 0.0);
        // $10,000 at 10% = $1,000 federal
        assert!(
            (result.short_term_federal_tax - 1_000.0).abs() < 0.01,
            "Expected 1000, got {}",
            result.short_term_federal_tax
        );
        assert_eq!(result.long_term_federal_tax, 0.0);
        // State: $10,000 * 5% = $500
        assert!((result.state_tax - 500.0).abs() < 0.01);
    }

    #[test]
    fn test_realized_gains_tax_long_term() {
        let config = test_tax_config();
        // Long-term gains at preferential rate
        let result = calculate_realized_gains_tax(0.0, 10_000.0, &config, 0.0);
        assert_eq!(result.short_term_federal_tax, 0.0);
        // $10,000 * 15% = $1,500
        assert!(
            (result.long_term_federal_tax - 1_500.0).abs() < 0.01,
            "Expected 1500, got {}",
            result.long_term_federal_tax
        );
        // State: $10,000 * 5% = $500
        assert!((result.state_tax - 500.0).abs() < 0.01);
    }

    #[test]
    fn test_realized_gains_tax_mixed() {
        let config = test_tax_config();
        // Both short and long-term gains
        let result = calculate_realized_gains_tax(5_000.0, 10_000.0, &config, 0.0);
        // Short-term: $5,000 at 10% = $500
        assert!((result.short_term_federal_tax - 500.0).abs() < 0.01);
        // Long-term: $10,000 at 15% = $1,500
        assert!((result.long_term_federal_tax - 1_500.0).abs() < 0.01);
        // Total federal: $2,000
        assert!((result.federal_tax - 2_000.0).abs() < 0.01);
        // State: $15,000 * 5% = $750
        assert!((result.state_tax - 750.0).abs() < 0.01);
    }

    #[test]
    fn test_tax_deferred_withdrawal_helper() {
        let config = test_tax_config();
        let result = calculate_tax_deferred_withdrawal_tax(10_000.0, &config, 0.0);
        // Federal: $10,000 at 10% = $1,000
        assert!((result.federal_tax - 1_000.0).abs() < 0.01);
        // State: $10,000 at 5% = $500
        assert!((result.state_tax - 500.0).abs() < 0.01);
        assert!((result.net_amount - 8_500.0).abs() < 0.01);
    }
}
