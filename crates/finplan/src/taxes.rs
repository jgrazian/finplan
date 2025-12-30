//! Tax calculation utilities for retirement withdrawal modeling

use crate::models::{AccountType, TaxBracket, TaxConfig};

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
pub fn calculate_marginal_tax(
    additional_income: f64,
    ytd_income: f64,
    brackets: &[TaxBracket],
) -> f64 {
    let tax_with_additional = calculate_federal_tax(ytd_income + additional_income, brackets);
    let tax_without = calculate_federal_tax(ytd_income, brackets);
    tax_with_additional - tax_without
}

/// Result of a withdrawal tax calculation
#[derive(Debug, Clone)]
pub struct WithdrawalTaxResult {
    /// The gross amount withdrawn from the account
    pub gross_amount: f64,
    /// Federal income tax owed
    pub federal_tax: f64,
    /// State income tax owed
    pub state_tax: f64,
    /// Capital gains tax owed (for Taxable accounts only)
    pub capital_gains_tax: f64,
    /// Total tax owed
    pub total_tax: f64,
    /// Amount remaining after taxes
    pub net_amount: f64,
}

/// Calculate taxes on a withdrawal from an account
///
/// # Arguments
/// * `gross_amount` - The pre-tax amount to withdraw
/// * `account_type` - The type of account being withdrawn from
/// * `tax_config` - Tax configuration
/// * `ytd_ordinary_income` - Year-to-date ordinary income (for bracket calculation)
pub fn calculate_withdrawal_tax(
    gross_amount: f64,
    account_type: &AccountType,
    tax_config: &TaxConfig,
    ytd_ordinary_income: f64,
) -> WithdrawalTaxResult {
    match account_type {
        AccountType::TaxFree => {
            // Roth IRA: No tax on qualified withdrawals
            WithdrawalTaxResult {
                gross_amount,
                federal_tax: 0.0,
                state_tax: 0.0,
                capital_gains_tax: 0.0,
                total_tax: 0.0,
                net_amount: gross_amount,
            }
        }
        AccountType::TaxDeferred => {
            // Traditional IRA/401k: All withdrawals taxed as ordinary income
            let federal_tax = calculate_marginal_tax(
                gross_amount,
                ytd_ordinary_income,
                &tax_config.federal_brackets,
            );
            let state_tax = gross_amount * tax_config.state_rate;
            let total_tax = federal_tax + state_tax;

            WithdrawalTaxResult {
                gross_amount,
                federal_tax,
                state_tax,
                capital_gains_tax: 0.0,
                total_tax,
                net_amount: gross_amount - total_tax,
            }
        }
        AccountType::Taxable => {
            // Brokerage account: Only gains are taxed (at capital gains rate)
            let gains = gross_amount * tax_config.taxable_gains_percentage;
            let capital_gains_tax = gains * tax_config.capital_gains_rate;
            // State tax typically applies to capital gains too
            let state_tax = gains * tax_config.state_rate;
            let total_tax = capital_gains_tax + state_tax;

            WithdrawalTaxResult {
                gross_amount,
                federal_tax: 0.0,
                state_tax,
                capital_gains_tax,
                total_tax,
                net_amount: gross_amount - total_tax,
            }
        }
        AccountType::Illiquid => {
            // Cannot withdraw from illiquid accounts
            WithdrawalTaxResult {
                gross_amount: 0.0,
                federal_tax: 0.0,
                state_tax: 0.0,
                capital_gains_tax: 0.0,
                total_tax: 0.0,
                net_amount: 0.0,
            }
        }
    }
}

/// Calculate the gross withdrawal needed to achieve a target net (after-tax) amount
/// Uses binary search to find the gross amount that results in the target net
///
/// # Arguments
/// * `target_net` - The desired after-tax amount
/// * `account_type` - The type of account being withdrawn from
/// * `tax_config` - Tax configuration
/// * `ytd_ordinary_income` - Year-to-date ordinary income
///
/// # Returns
/// The gross amount needed, or None if the account is Illiquid
pub fn gross_up_for_net_target(
    target_net: f64,
    account_type: &AccountType,
    tax_config: &TaxConfig,
    ytd_ordinary_income: f64,
) -> Option<f64> {
    if matches!(account_type, AccountType::Illiquid) {
        return None;
    }

    if matches!(account_type, AccountType::TaxFree) {
        // No tax, gross = net
        return Some(target_net);
    }

    // Binary search for the gross amount
    let mut low = target_net;
    let mut high = target_net * 2.0; // Start with 2x as upper bound

    // Expand upper bound if needed
    let check_high = calculate_withdrawal_tax(high, account_type, tax_config, ytd_ordinary_income);
    if check_high.net_amount < target_net {
        high = target_net * 3.0;
    }

    const TOLERANCE: f64 = 0.01;
    const MAX_ITERATIONS: usize = 50;

    for _ in 0..MAX_ITERATIONS {
        let mid = (low + high) / 2.0;
        let result = calculate_withdrawal_tax(mid, account_type, tax_config, ytd_ordinary_income);

        let diff = result.net_amount - target_net;

        if diff.abs() < TOLERANCE {
            return Some(mid);
        }

        if diff < 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }

    // Return best estimate after max iterations
    Some((low + high) / 2.0)
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
            taxable_gains_percentage: 0.50,
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
        let marginal = calculate_marginal_tax(10_000.0, 35_000.0, &config.federal_brackets);
        assert!(
            (marginal - 1_700.0).abs() < 0.01,
            "Expected 1700, got {}",
            marginal
        );
    }

    #[test]
    fn test_tax_free_withdrawal() {
        let config = test_tax_config();
        let result = calculate_withdrawal_tax(10_000.0, &AccountType::TaxFree, &config, 0.0);
        assert_eq!(result.total_tax, 0.0);
        assert_eq!(result.net_amount, 10_000.0);
    }

    #[test]
    fn test_tax_deferred_withdrawal() {
        let config = test_tax_config();
        let result = calculate_withdrawal_tax(10_000.0, &AccountType::TaxDeferred, &config, 0.0);
        // Federal: $10,000 at 10% = $1,000
        // State: $10,000 at 5% = $500
        // Total: $1,500
        assert!((result.federal_tax - 1_000.0).abs() < 0.01);
        assert!((result.state_tax - 500.0).abs() < 0.01);
        assert!((result.total_tax - 1_500.0).abs() < 0.01);
        assert!((result.net_amount - 8_500.0).abs() < 0.01);
    }

    #[test]
    fn test_taxable_withdrawal() {
        let config = test_tax_config();
        let result = calculate_withdrawal_tax(10_000.0, &AccountType::Taxable, &config, 0.0);
        // Gains: $10,000 * 50% = $5,000
        // Capital gains tax: $5,000 * 15% = $750
        // State tax on gains: $5,000 * 5% = $250
        // Total: $1,000
        assert!((result.capital_gains_tax - 750.0).abs() < 0.01);
        assert!((result.state_tax - 250.0).abs() < 0.01);
        assert!((result.total_tax - 1_000.0).abs() < 0.01);
        assert!((result.net_amount - 9_000.0).abs() < 0.01);
    }

    #[test]
    fn test_gross_up_tax_free() {
        let config = test_tax_config();
        let gross = gross_up_for_net_target(10_000.0, &AccountType::TaxFree, &config, 0.0);
        assert_eq!(gross, Some(10_000.0));
    }

    #[test]
    fn test_gross_up_tax_deferred() {
        let config = test_tax_config();
        // If we want $8,500 net and tax rate is ~15% (10% fed + 5% state)
        // We need to withdraw more to end up with $8,500 after taxes
        let gross = gross_up_for_net_target(8_500.0, &AccountType::TaxDeferred, &config, 0.0);
        assert!(gross.is_some());

        // Verify by calculating tax on the result
        let result =
            calculate_withdrawal_tax(gross.unwrap(), &AccountType::TaxDeferred, &config, 0.0);
        assert!(
            (result.net_amount - 8_500.0).abs() < 1.0,
            "Expected net ~8500, got {}",
            result.net_amount
        );
    }

    #[test]
    fn test_illiquid_returns_none() {
        let config = test_tax_config();
        let gross = gross_up_for_net_target(10_000.0, &AccountType::Illiquid, &config, 0.0);
        assert_eq!(gross, None);
    }
}
