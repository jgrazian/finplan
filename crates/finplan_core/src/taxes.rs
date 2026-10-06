//! Tax calculation utilities for retirement withdrawal modeling
//!
//! This module provides lot-based tax calculations using actual `AssetLot` data
//! for accurate capital gains tracking with proper short-term vs long-term distinction.

use crate::model::{TaxBracket, TaxConfig};

/// Calculate federal income tax using progressive brackets
/// Returns the total tax owed on the given income
#[must_use]
pub fn calculate_federal_tax(income: f64, brackets: &[TaxBracket]) -> f64 {
    if income <= 0.0 || brackets.is_empty() {
        return 0.0;
    }

    let mut tax = 0.0;
    let mut prev_threshold = 0.0;

    for (i, bracket) in brackets.iter().enumerate() {
        let next_threshold = brackets.get(i + 1).map_or(f64::INFINITY, |b| b.threshold);

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

/// The federal brackets for one tax year, over gross ordinary income: the
/// `deduction` (the standard deduction, plus its 65+ extra from the year the
/// filer turns 65) folded in as a 0% band, then every threshold scaled by
/// `factor`, the path's cumulative inflation through the end of the prior
/// year. This is how the engine indexes each year's brackets, so a reader of
/// a stored run can rebuild them from the plan's tax config and the run's
/// inflation.
#[must_use]
pub fn indexed_brackets(base: &[TaxBracket], deduction: f64, factor: f64) -> Vec<TaxBracket> {
    let mut brackets = TaxConfig::brackets_with_deduction(base, deduction);
    for bracket in &mut brackets {
        bracket.threshold *= factor;
    }
    brackets
}

/// The federal rate the next dollar of ordinary income is taxed at, with
/// `income` already earned this year.
#[must_use]
pub fn marginal_rate(income: f64, brackets: &[TaxBracket]) -> f64 {
    brackets
        .iter()
        .take_while(|bracket| bracket.threshold <= income.max(0.0))
        .last()
        .map_or(0.0, |bracket| bracket.rate)
}

/// Calculate marginal tax on additional income given existing YTD income
/// This is useful for calculating tax on a withdrawal when there's already
/// been taxable income earlier in the year
#[must_use]
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
/// This is the inverse of `calculate_federal_marginal_tax` + state tax
#[must_use]
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
            .map_or(f64::MAX, |b| b.threshold);

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
        }
        // Use up this bracket and move to next
        gross += bracket_room;
        remaining_net -= max_net_in_bracket;
        income_cursor = next_threshold;
    }

    gross
}

// ============================================================================
// Tax Calculation with Actual Gains (Lot-Based)
// ============================================================================

/// What a tax year has seen so far, which is what the next dollar's tax
/// depends on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct YtdIncome {
    /// Income taxed in the ordinary brackets: wages, distributions and
    /// short-term gains.
    pub ordinary: f64,
    /// The short-term gains within `ordinary`, which are also investment
    /// income for the net investment income tax.
    pub short_term_gains: f64,
    /// Long-term gains, taxed in the gains brackets stacked on `ordinary`.
    pub long_term_gains: f64,
}

impl YtdIncome {
    /// The year so far plus `short_term` and `long_term` more gains.
    #[must_use]
    pub fn with_gains(self, short_term: f64, long_term: f64) -> Self {
        Self {
            ordinary: self.ordinary + short_term,
            short_term_gains: self.short_term_gains + short_term,
            long_term_gains: self.long_term_gains + long_term,
        }
    }

    /// The year so far plus `amount` more ordinary income.
    #[must_use]
    pub fn with_ordinary(self, amount: f64) -> Self {
        Self {
            ordinary: self.ordinary + amount,
            ..self
        }
    }
}

impl std::ops::Add for YtdIncome {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            ordinary: self.ordinary + other.ordinary,
            short_term_gains: self.short_term_gains + other.short_term_gains,
            long_term_gains: self.long_term_gains + other.long_term_gains,
        }
    }
}

/// The net investment income tax a year owes with `ytd` realized: 3.8% of the
/// lesser of its gains and how far its income (ordinary plus long-term gains,
/// standing in for modified AGI) is over `threshold`.
#[must_use]
pub fn net_investment_income_tax(ytd: YtdIncome, threshold: Option<f64>) -> f64 {
    let Some(threshold) = threshold else {
        return 0.0;
    };
    let investment_income = ytd.short_term_gains + ytd.long_term_gains;
    let over = ytd.ordinary + ytd.long_term_gains - threshold;
    TaxConfig::NIIT_RATE * investment_income.min(over).max(0.0)
}

/// Result of calculating taxes on actual realized gains
#[derive(Debug, Clone, Default)]
pub struct RealizedGainsTaxResult {
    /// Federal tax on short-term gains: ordinary rates, plus the net
    /// investment income tax they add
    pub short_term_federal_tax: f64,
    /// Federal tax on long-term gains: the gains brackets, plus the net
    /// investment income tax they add
    pub long_term_federal_tax: f64,
    /// Total federal tax
    pub federal_tax: f64,
    /// State tax on all gains
    pub state_tax: f64,
    /// Total tax owed
    pub total_tax: f64,
}

/// Calculate taxes on gains realized with `ytd` already seen this year.
///
/// Short-term gains are ordinary income, taxed at the ordinary brackets from
/// where the year's ordinary income stands. Long-term gains stack on top of
/// all ordinary income (this sale's short-term gains too) and the year's
/// earlier long-term gains, and are taxed in `capital_gains_brackets` from
/// there. Both may add net investment income tax. State tax is flat on both.
///
/// `tax_config`'s brackets are the year's: deduction folded in and indexed,
/// as the simulation keeps them.
#[must_use]
pub fn calculate_realized_gains_tax(
    short_term_gain: f64,
    long_term_gain: f64,
    tax_config: &TaxConfig,
    ytd: YtdIncome,
) -> RealizedGainsTaxResult {
    let short_term_gain = short_term_gain.max(0.0);
    let long_term_gain = long_term_gain.max(0.0);
    let niit = |ytd| net_investment_income_tax(ytd, tax_config.niit_threshold);

    let after_short = ytd.with_gains(short_term_gain, 0.0);
    let short_term_federal_tax = if short_term_gain > 0.0 {
        calculate_federal_marginal_tax(short_term_gain, ytd.ordinary, &tax_config.federal_brackets)
            + niit(after_short)
            - niit(ytd)
    } else {
        0.0
    };

    let long_term_federal_tax = if long_term_gain > 0.0 {
        let stacked_under = after_short.ordinary + after_short.long_term_gains;
        calculate_federal_marginal_tax(
            long_term_gain,
            stacked_under,
            &tax_config.capital_gains_brackets,
        ) + niit(after_short.with_gains(0.0, long_term_gain))
            - niit(after_short)
    } else {
        0.0
    };

    let federal_tax = short_term_federal_tax + long_term_federal_tax;

    // State tax typically applies to all gains at the same rate
    let state_tax = (short_term_gain + long_term_gain) * tax_config.state_rate;

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
#[must_use]
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
            capital_gains_brackets: vec![
                TaxBracket {
                    threshold: 0.0,
                    rate: 0.0,
                },
                TaxBracket {
                    threshold: 30_000.0,
                    rate: 0.15,
                },
                TaxBracket {
                    threshold: 100_000.0,
                    rate: 0.20,
                },
            ],
            early_withdrawal_penalty_rate: 0.10,
            ..TaxConfig::default()
        }
    }

    fn ordinary(amount: f64) -> YtdIncome {
        YtdIncome::default().with_ordinary(amount)
    }

    #[test]
    fn test_deduction_is_a_zero_bracket() {
        let config = test_tax_config();
        let brackets = TaxConfig::brackets_with_deduction(&config.federal_brackets, 15_000.0);
        // $50,000 gross is taxed like $35,000 under the plain brackets.
        let with = calculate_federal_tax(50_000.0, &brackets);
        let without = calculate_federal_tax(35_000.0, &config.federal_brackets);
        assert!((with - without).abs() < 1e-9);
        assert_eq!(calculate_federal_tax(15_000.0, &brackets), 0.0);
        // A zero deduction leaves the brackets alone.
        assert_eq!(
            TaxConfig::brackets_with_deduction(&config.federal_brackets, 0.0).len(),
            config.federal_brackets.len()
        );
    }

    #[test]
    fn test_federal_tax_first_bracket() {
        let config = test_tax_config();
        let tax = calculate_federal_tax(5_000.0, &config.federal_brackets);
        assert!((tax - 500.0).abs() < 0.01, "Expected 500, got {tax}");
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
        assert!((tax - 6_800.0).abs() < 0.01, "Expected 6800, got {tax}");
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
            "Expected 1700, got {marginal}"
        );
    }

    #[test]
    fn test_realized_gains_tax_short_term() {
        let config = test_tax_config();
        // Short-term gains taxed as ordinary income
        let result = calculate_realized_gains_tax(10_000.0, 0.0, &config, YtdIncome::default());
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
        // $50,000 of ordinary income puts the gain in the 15% band.
        let result = calculate_realized_gains_tax(0.0, 10_000.0, &config, ordinary(50_000.0));
        assert_eq!(result.short_term_federal_tax, 0.0);
        assert!(
            (result.long_term_federal_tax - 1_500.0).abs() < 0.01,
            "Expected 1500, got {}",
            result.long_term_federal_tax
        );
        // State: $10,000 * 5% = $500
        assert!((result.state_tax - 500.0).abs() < 0.01);
    }

    #[test]
    fn long_term_gains_under_the_zero_band_owe_no_federal_tax() {
        let config = test_tax_config();
        let result = calculate_realized_gains_tax(0.0, 25_000.0, &config, ordinary(5_000.0));
        assert_eq!(result.long_term_federal_tax, 0.0);
        // State tax is flat all the same.
        assert!((result.state_tax - 1_250.0).abs() < 0.01);
    }

    #[test]
    fn test_realized_gains_tax_mixed() {
        let config = test_tax_config();
        // $20,000 of wages, then $5,000 short-term and $10,000 long-term.
        let result = calculate_realized_gains_tax(5_000.0, 10_000.0, &config, ordinary(20_000.0));
        // Short-term: $5,000 at 12% (from $20,000 to $25,000) = $600
        assert!((result.short_term_federal_tax - 600.0).abs() < 0.01);
        // Long-term stacks from $25,000: $5,000 at 0%, $5,000 at 15% = $750
        assert!((result.long_term_federal_tax - 750.0).abs() < 0.01);
        assert!((result.federal_tax - 1_350.0).abs() < 0.01);
        // State: $15,000 * 5% = $750
        assert!((result.state_tax - 750.0).abs() < 0.01);
    }

    #[test]
    fn long_term_gains_stack_on_the_years_earlier_gains() {
        let config = test_tax_config();
        let ytd = YtdIncome::default().with_gains(0.0, 95_000.0);
        let result = calculate_realized_gains_tax(0.0, 10_000.0, &config, ytd);
        // $5,000 at 15% and $5,000 at 20%.
        assert!((result.long_term_federal_tax - 1_750.0).abs() < 0.01);
    }

    #[test]
    fn unused_deduction_shelters_long_term_gains() {
        let mut config = test_tax_config();
        config.federal_brackets = indexed_brackets(&config.federal_brackets, 15_000.0, 1.0);
        config.capital_gains_brackets =
            indexed_brackets(&config.capital_gains_brackets, 15_000.0, 1.0);
        // $10,000 of wages leaves $5,000 of deduction: $40,000 of gains is
        // $35,000 taxable, $30,000 of it at 0% and $5,000 at 15%.
        let result = calculate_realized_gains_tax(0.0, 40_000.0, &config, ordinary(10_000.0));
        assert!(
            (result.long_term_federal_tax - 750.0).abs() < 0.01,
            "{}",
            result.long_term_federal_tax
        );
    }

    #[test]
    fn net_investment_income_tax_applies_above_its_threshold() {
        let mut config = test_tax_config();
        let ytd = ordinary(190_000.0);
        let without = calculate_realized_gains_tax(0.0, 30_000.0, &config, ytd);
        // All in the 20% band.
        assert!((without.long_term_federal_tax - 6_000.0).abs() < 0.01);

        // 3.8% of the $20,000 the gain takes income over $200,000.
        config.niit_threshold = Some(200_000.0);
        let with = calculate_realized_gains_tax(0.0, 30_000.0, &config, ytd);
        assert!((with.long_term_federal_tax - 6_760.0).abs() < 0.01);

        // Short-term gains are investment income too.
        let short = calculate_realized_gains_tax(30_000.0, 0.0, &config, ytd);
        let ordinary_only =
            calculate_federal_marginal_tax(30_000.0, 190_000.0, &config.federal_brackets);
        assert!((short.short_term_federal_tax - ordinary_only - 760.0).abs() < 0.01);
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
