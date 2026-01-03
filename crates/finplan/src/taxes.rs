//! Tax calculation utilities for retirement withdrawal modeling
//!
//! This module provides lot-based tax calculations using actual `AssetLot` data
//! for accurate capital gains tracking with proper short-term vs long-term distinction.

use crate::model::{LotMethod, TaxBracket, TaxConfig};
use crate::simulation_state::AssetLot;

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

// ============================================================================
// Lot-Based Capital Gains Calculation
// ============================================================================

/// Result of consuming lots to satisfy a liquidation
#[derive(Debug, Clone, Default)]
pub struct LotConsumptionResult {
    /// Amount actually consumed (may be less than requested if insufficient balance)
    pub amount_consumed: f64,
    /// Total cost basis of consumed lots
    pub cost_basis: f64,
    /// Short-term capital gain (held < 1 year)
    pub short_term_gain: f64,
    /// Long-term capital gain (held >= 1 year)
    pub long_term_gain: f64,
}

/// Sort lots according to the specified method
pub fn sort_lots_by_method(lots: &mut [AssetLot], method: LotMethod) {
    match method {
        LotMethod::Fifo => lots.sort_by_key(|l| l.purchase_date),
        LotMethod::Lifo => lots.sort_by(|a, b| b.purchase_date.cmp(&a.purchase_date)),
        LotMethod::HighestCost => lots.sort_by(|a, b| {
            let a_per_unit = if a.units > 0.0 {
                a.cost_basis / a.units
            } else {
                0.0
            };
            let b_per_unit = if b.units > 0.0 {
                b.cost_basis / b.units
            } else {
                0.0
            };
            b_per_unit
                .partial_cmp(&a_per_unit)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        LotMethod::LowestCost => lots.sort_by(|a, b| {
            let a_per_unit = if a.units > 0.0 {
                a.cost_basis / a.units
            } else {
                0.0
            };
            let b_per_unit = if b.units > 0.0 {
                b.cost_basis / b.units
            } else {
                0.0
            };
            a_per_unit
                .partial_cmp(&b_per_unit)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        LotMethod::AverageCost => {
            // No sorting needed for average cost - handled specially in consume_lots
        }
    }
}

/// Consume lots to satisfy a liquidation amount
///
/// Modifies the lots in place and returns details about the consumption.
/// For taxable accounts, this provides accurate cost basis and gain/loss tracking.
///
/// # Arguments
/// * `lots` - The lots to consume from (will be modified)
/// * `amount` - The amount to liquidate
/// * `method` - Which lot selection method to use
/// * `current_date` - Current date for determining short/long-term holding period
///
/// # Returns
/// Details about what was consumed including cost basis and gains
pub fn consume_lots(
    lots: &mut Vec<AssetLot>,
    amount: f64,
    method: LotMethod,
    current_date: jiff::civil::Date,
) -> LotConsumptionResult {
    if amount <= 0.0 || lots.is_empty() {
        return LotConsumptionResult::default();
    }

    if method == LotMethod::AverageCost {
        consume_lots_average_cost(lots, amount, current_date)
    } else {
        sort_lots_by_method(lots, method);
        consume_lots_specific(lots, amount, current_date)
    }
}

/// Consume lots using average cost basis method
fn consume_lots_average_cost(
    lots: &mut Vec<AssetLot>,
    amount: f64,
    current_date: jiff::civil::Date,
) -> LotConsumptionResult {
    let total_units: f64 = lots.iter().map(|l| l.units).sum();
    let total_basis: f64 = lots.iter().map(|l| l.cost_basis).sum();

    if total_units <= 0.0 {
        return LotConsumptionResult::default();
    }

    let actual_amount = amount.min(total_units);
    let avg_basis_per_unit = total_basis / total_units;
    let cost_basis = actual_amount * avg_basis_per_unit;
    let gain = actual_amount - cost_basis;

    // For average cost, determine holding period based on weighted average of lot ages
    // Simplified: treat as long-term if majority of value is in lots held > 1 year
    let long_term_value: f64 = lots
        .iter()
        .filter(|l| (current_date - l.purchase_date).get_days() >= 365)
        .map(|l| l.units)
        .sum();
    let is_mostly_long_term = long_term_value > total_units / 2.0;

    let (short_term_gain, long_term_gain) = if is_mostly_long_term {
        (0.0, gain.max(0.0))
    } else {
        (gain.max(0.0), 0.0)
    };

    // Remove units proportionally from all lots
    let proportion = actual_amount / total_units;
    for lot in lots.iter_mut() {
        let units_to_remove = lot.units * proportion;
        let basis_to_remove = lot.cost_basis * proportion;
        lot.units -= units_to_remove;
        lot.cost_basis -= basis_to_remove;
    }

    // Remove depleted lots
    lots.retain(|l| l.units > 0.001);

    LotConsumptionResult {
        amount_consumed: actual_amount,
        cost_basis,
        short_term_gain,
        long_term_gain,
    }
}

/// Consume lots using specific identification (FIFO, LIFO, HighestCost, LowestCost)
fn consume_lots_specific(
    lots: &mut Vec<AssetLot>,
    amount: f64,
    current_date: jiff::civil::Date,
) -> LotConsumptionResult {
    let mut result = LotConsumptionResult::default();
    let mut remaining = amount;
    let mut lots_to_remove = Vec::new();

    for (idx, lot) in lots.iter_mut().enumerate() {
        if remaining <= 0.001 {
            break;
        }

        let take_amount = remaining.min(lot.units);
        let take_fraction = if lot.units > 0.0 {
            take_amount / lot.units
        } else {
            0.0
        };
        let basis_used = lot.cost_basis * take_fraction;

        result.amount_consumed += take_amount;
        result.cost_basis += basis_used;

        let gain = take_amount - basis_used;

        // Determine if short-term or long-term (>= 1 year)
        let holding_days = (current_date - lot.purchase_date).get_days();
        if holding_days >= 365 {
            result.long_term_gain += gain.max(0.0);
        } else {
            result.short_term_gain += gain.max(0.0);
        }

        lot.units -= take_amount;
        lot.cost_basis -= basis_used;

        if lot.units <= 0.001 {
            lots_to_remove.push(idx);
        }
        remaining -= take_amount;
    }

    // Remove depleted lots (in reverse order to preserve indices)
    for idx in lots_to_remove.iter().rev() {
        lots.remove(*idx);
    }

    result
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
        calculate_marginal_tax(
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

/// Estimate the effective tax rate for a taxable account based on lot data
///
/// This calculates what the tax rate would be if we liquidated a given amount
/// using the specified lot method. Returns the effective tax rate (0.0 to 1.0).
///
/// # Arguments
/// * `lots` - The lots available (will NOT be modified - uses a clone)
/// * `amount` - The amount to estimate tax for
/// * `lot_method` - Which lot selection method to use
/// * `current_date` - Current date for determining short/long-term holding period
/// * `tax_config` - Tax configuration
/// * `ytd_ordinary_income` - Year-to-date ordinary income for bracket calculation
pub fn estimate_taxable_effective_rate(
    lots: &[AssetLot],
    amount: f64,
    lot_method: LotMethod,
    current_date: jiff::civil::Date,
    tax_config: &TaxConfig,
    ytd_ordinary_income: f64,
) -> f64 {
    if amount <= 0.0 || lots.is_empty() {
        return 0.0;
    }

    // Clone lots to simulate consumption without modifying originals
    let mut lots_clone: Vec<AssetLot> = lots.to_vec();

    // Simulate lot consumption
    let lot_result = consume_lots(&mut lots_clone, amount, lot_method, current_date);

    if lot_result.amount_consumed <= 0.0 {
        return 0.0;
    }

    // Calculate taxes on the simulated gains
    let tax_result = calculate_realized_gains_tax(
        lot_result.short_term_gain,
        lot_result.long_term_gain,
        tax_config,
        ytd_ordinary_income,
    );

    // Return effective rate (total tax / gross amount)
    tax_result.total_tax / lot_result.amount_consumed
}

/// Calculate gross withdrawal needed to achieve target net for a taxable account
/// using actual lot data for precise cost basis and gain calculation.
///
/// # Arguments
/// * `target_net` - The desired after-tax amount
/// * `lots` - The lots available (will NOT be modified)
/// * `lot_method` - Which lot selection method to use
/// * `current_date` - Current date for determining short/long-term holding period
/// * `tax_config` - Tax configuration
/// * `ytd_ordinary_income` - Year-to-date ordinary income for bracket calculation
///
/// # Returns
/// The gross amount needed to achieve the target net
pub fn gross_up_for_net_target_with_lots(
    target_net: f64,
    lots: &[AssetLot],
    lot_method: LotMethod,
    current_date: jiff::civil::Date,
    tax_config: &TaxConfig,
    ytd_ordinary_income: f64,
) -> f64 {
    if target_net <= 0.0 || lots.is_empty() {
        return target_net;
    }

    // Calculate total available in lots
    let total_available: f64 = lots.iter().map(|l| l.units).sum();
    if total_available <= 0.0 {
        return target_net;
    }

    // Binary search for the gross amount
    // Upper bound is capped at what's available
    let mut low = target_net;
    let mut high = total_available.min(target_net * 2.0);

    const TOLERANCE: f64 = 0.01;
    const MAX_ITERATIONS: usize = 30;

    for _ in 0..MAX_ITERATIONS {
        let mid = (low + high) / 2.0;

        // Simulate lot consumption at this gross amount
        let mut lots_clone: Vec<AssetLot> = lots.to_vec();
        let lot_result = consume_lots(&mut lots_clone, mid, lot_method, current_date);

        // If we couldn't consume as much as requested, we're at the limit
        if lot_result.amount_consumed < mid - 0.01 {
            // Can't get enough gross, return what's available
            return lot_result.amount_consumed;
        }

        // Calculate taxes
        let tax_result = calculate_realized_gains_tax(
            lot_result.short_term_gain,
            lot_result.long_term_gain,
            tax_config,
            ytd_ordinary_income,
        );

        let net = lot_result.amount_consumed - tax_result.total_tax;
        let diff = net - target_net;

        if diff.abs() < TOLERANCE {
            return mid;
        }

        if diff < 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }

    (low + high) / 2.0
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
    let federal_tax = calculate_marginal_tax(
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

    // ========================================================================
    // Lot-Based Tax Calculation Tests
    // ========================================================================

    #[test]
    fn test_consume_lots_fifo() {
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1); // > 1 year ago
        let recent_date = jiff::civil::date(2025, 3, 1); // < 1 year ago

        let mut lots = vec![
            AssetLot {
                purchase_date: old_date,
                units: 100.0,
                cost_basis: 80.0,
            },
            AssetLot {
                purchase_date: recent_date,
                units: 100.0,
                cost_basis: 95.0,
            },
        ];

        let result = consume_lots(&mut lots, 100.0, LotMethod::Fifo, today);

        // Should consume from oldest lot first
        assert!(
            (result.amount_consumed - 100.0).abs() < 0.01,
            "Expected 100 consumed, got {}",
            result.amount_consumed
        );
        assert!(
            (result.cost_basis - 80.0).abs() < 0.01,
            "Expected 80 basis, got {}",
            result.cost_basis
        );
        // Gain of 20.0, held > 1 year = long-term
        assert!(
            (result.long_term_gain - 20.0).abs() < 0.01,
            "Expected 20 LT gain, got {}",
            result.long_term_gain
        );
        assert_eq!(result.short_term_gain, 0.0);
        // Old lot should be consumed, recent lot untouched
        assert_eq!(lots.len(), 1);
        assert!((lots[0].units - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_consume_lots_lifo() {
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1); // > 1 year ago
        let recent_date = jiff::civil::date(2025, 3, 1); // < 1 year ago

        let mut lots = vec![
            AssetLot {
                purchase_date: old_date,
                units: 100.0,
                cost_basis: 80.0,
            },
            AssetLot {
                purchase_date: recent_date,
                units: 100.0,
                cost_basis: 95.0,
            },
        ];

        let result = consume_lots(&mut lots, 100.0, LotMethod::Lifo, today);

        // Should consume from newest lot first
        assert!((result.cost_basis - 95.0).abs() < 0.01);
        // Gain of 5.0, held < 1 year = short-term
        assert!(
            (result.short_term_gain - 5.0).abs() < 0.01,
            "Expected 5 ST gain, got {}",
            result.short_term_gain
        );
        assert_eq!(result.long_term_gain, 0.0);
        // Recent lot consumed, old lot untouched
        assert_eq!(lots.len(), 1);
        assert!((lots[0].cost_basis - 80.0).abs() < 0.01);
    }

    #[test]
    fn test_consume_lots_highest_cost() {
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1);

        let mut lots = vec![
            AssetLot {
                purchase_date: old_date,
                units: 100.0,
                cost_basis: 80.0,
            }, // $0.80/unit
            AssetLot {
                purchase_date: old_date,
                units: 100.0,
                cost_basis: 120.0,
            }, // $1.20/unit
        ];

        let result = consume_lots(&mut lots, 100.0, LotMethod::HighestCost, today);

        // Should consume from highest cost lot first (120.0 basis)
        assert!(
            (result.cost_basis - 120.0).abs() < 0.01,
            "Expected 120 basis, got {}",
            result.cost_basis
        );
        // Gain of -20.0 (loss), but we track max(0) for gains
        assert_eq!(result.long_term_gain, 0.0);
        assert_eq!(lots.len(), 1);
        assert!((lots[0].cost_basis - 80.0).abs() < 0.01);
    }

    #[test]
    fn test_consume_lots_partial() {
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1);

        let mut lots = vec![AssetLot {
            purchase_date: old_date,
            units: 100.0,
            cost_basis: 80.0,
        }];

        let result = consume_lots(&mut lots, 50.0, LotMethod::Fifo, today);

        assert!((result.amount_consumed - 50.0).abs() < 0.01);
        assert!((result.cost_basis - 40.0).abs() < 0.01); // Half of 80
        assert!((result.long_term_gain - 10.0).abs() < 0.01); // 50 - 40 = 10
        // Lot should have 50 units remaining
        assert_eq!(lots.len(), 1);
        assert!((lots[0].units - 50.0).abs() < 0.01);
        assert!((lots[0].cost_basis - 40.0).abs() < 0.01);
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

    #[test]
    fn test_gross_up_for_net_target_with_lots() {
        let config = test_tax_config();
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1); // > 1 year ago

        // Lot with 50% gain (cost basis $50 for $100 value)
        let lots = vec![AssetLot {
            purchase_date: old_date,
            units: 100.0,
            cost_basis: 50.0,
        }];

        // If we want $90 net, we need to figure out the gross
        // With 50% gain and 15% cap gains + 5% state = 20% tax on gains
        // If we withdraw $100, gain is $50, tax is $50 * 0.20 = $10, net = $90
        let gross =
            gross_up_for_net_target_with_lots(90.0, &lots, LotMethod::Fifo, today, &config, 0.0);

        // Should be approximately $100
        assert!((gross - 100.0).abs() < 1.0, "Expected ~100, got {}", gross);

        // Verify by simulating the withdrawal
        let mut lots_clone = lots.clone();
        let lot_result = consume_lots(&mut lots_clone, gross, LotMethod::Fifo, today);
        let tax_result = calculate_realized_gains_tax(
            lot_result.short_term_gain,
            lot_result.long_term_gain,
            &config,
            0.0,
        );
        let actual_net = lot_result.amount_consumed - tax_result.total_tax;
        assert!(
            (actual_net - 90.0).abs() < 1.0,
            "Expected net ~90, got {}",
            actual_net
        );
    }

    #[test]
    fn test_gross_up_with_no_gain() {
        let config = test_tax_config();
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1);

        // Lot with no gain (cost basis = value)
        let lots = vec![AssetLot {
            purchase_date: old_date,
            units: 100.0,
            cost_basis: 100.0,
        }];

        // With no gain, gross should equal net
        let gross =
            gross_up_for_net_target_with_lots(50.0, &lots, LotMethod::Fifo, today, &config, 0.0);

        assert!(
            (gross - 50.0).abs() < 1.0,
            "Expected ~50 (no gain = no tax), got {}",
            gross
        );
    }

    #[test]
    fn test_estimate_taxable_effective_rate() {
        let config = test_tax_config();
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1);

        // Lot with 50% gain
        let lots = vec![AssetLot {
            purchase_date: old_date,
            units: 100.0,
            cost_basis: 50.0,
        }];

        let rate =
            estimate_taxable_effective_rate(&lots, 100.0, LotMethod::Fifo, today, &config, 0.0);

        // 50% gain, taxed at 15% cap gains + 5% state = 20% on gains
        // Effective rate = 0.5 * 0.20 = 0.10 (10%)
        assert!(
            (rate - 0.10).abs() < 0.01,
            "Expected 10% effective rate, got {}%",
            rate * 100.0
        );
    }
}
