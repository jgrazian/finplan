//! Asset liquidation with proper lot tracking and tax calculation
//!
//! This module provides functions to liquidate assets from investment accounts,
//! handling cost basis tracking, capital gains calculation, and tax implications.

use jiff::civil::Date;

use crate::{
    evaluate::EvalEvent,
    model::{
        AccountId, AssetCoord, AssetId, AssetLot, CashFlowKind, InvestmentContainer, LotMethod,
        Market, TaxConfig, TaxStatus,
    },
    taxes::calculate_federal_marginal_tax,
};

/// Calculate current price for an asset using the Market struct
pub fn get_current_price(
    market: &Market,
    start_date: Date,
    current_date: Date,
    asset_id: AssetId,
) -> Option<f64> {
    market.get_asset_value(start_date, current_date, asset_id)
}

/// Result of liquidating assets from a single source
#[derive(Debug, Clone, Default)]
pub struct LiquidationResult {
    /// Total gross proceeds from the sale
    pub gross_amount: f64,
    /// Net proceeds after taxes
    pub net_proceeds: f64,
}

/// Parameters for liquidating assets from an investment container
#[derive(Debug, Clone)]
pub struct LiquidationParams<'a> {
    /// The investment container to liquidate from
    pub investment: &'a InvestmentContainer,
    /// The full asset coordinate (for generating StateEvents)
    pub asset_coord: AssetCoord,
    /// Where to credit the cash proceeds
    pub to_account: AccountId,
    /// Dollar amount to liquidate (at current prices)
    pub amount: f64,
    /// Current price per unit from Market
    pub current_price: f64,
    /// How to select lots for sale
    pub lot_method: LotMethod,
    /// Current simulation date
    pub current_date: Date,
    /// Tax configuration for calculating taxes
    pub tax_config: &'a TaxConfig,
    /// Year-to-date ordinary income for marginal tax calculation
    pub ytd_ordinary_income: f64,
}

/// Liquidate assets from an investment container.
///
/// This is the main entry point for liquidating assets. It takes the investment
/// container directly along with the current asset price from Market.
///
/// # Arguments
/// * `params` - Liquidation parameters containing all necessary information
pub fn liquidate_investment(params: &LiquidationParams) -> (LiquidationResult, Vec<EvalEvent>) {
    // Get positions for this specific asset
    let lots: Vec<AssetLot> = params
        .investment
        .positions
        .iter()
        .filter(|lot| lot.asset_id == params.asset_coord.asset_id)
        .cloned()
        .collect();

    if lots.is_empty() || params.amount <= 0.001 || params.current_price <= 0.0 {
        return (LiquidationResult::default(), Vec::new());
    }

    // Calculate total available value
    let total_units: f64 = lots.iter().map(|l| l.units).sum();
    let available_value = total_units * params.current_price;
    let actual_amount = params.amount.min(available_value);

    if actual_amount <= 0.001 {
        return (LiquidationResult::default(), Vec::new());
    }

    match params.investment.tax_status {
        TaxStatus::Taxable => liquidate_taxable(&lots, actual_amount, params),
        TaxStatus::TaxDeferred => liquidate_tax_deferred(&lots, actual_amount, params),
        TaxStatus::TaxFree => liquidate_tax_free(&lots, actual_amount, params),
    }
}

/// Liquidate from a taxable account with full lot tracking
fn liquidate_taxable(
    lots: &[AssetLot],
    amount: f64,
    params: &LiquidationParams,
) -> (LiquidationResult, Vec<EvalEvent>) {
    let lot_result = consume_lots(
        lots,
        amount,
        params.current_price,
        params.lot_method,
        params.current_date,
    );
    let mut effects = lot_subtractions_to_effects(params.asset_coord, &lot_result);

    let gross_amount = lot_result.proceeds;
    let mut net_amount = gross_amount;

    // Short-term gains taxed as ordinary income
    if lot_result.short_term_gain > 0.0 {
        let federal_short_term_tax = calculate_federal_marginal_tax(
            lot_result.short_term_gain,
            params.ytd_ordinary_income,
            &params.tax_config.federal_brackets,
        );
        let state_short_term_tax = lot_result.short_term_gain * params.tax_config.state_rate;

        effects.push(EvalEvent::ShortTermCapitalGainsTax {
            gross_gain_amount: lot_result.short_term_gain,
            federal_tax: federal_short_term_tax,
            state_tax: state_short_term_tax,
        });

        net_amount -= federal_short_term_tax + state_short_term_tax;
    }

    // Long-term gains taxed at capital gains rate
    if lot_result.long_term_gain > 0.0 {
        let federal_long_term_tax =
            lot_result.long_term_gain * params.tax_config.capital_gains_rate;
        let state_long_term_tax = lot_result.long_term_gain * params.tax_config.state_rate;

        effects.push(EvalEvent::LongTermCapitalGainsTax {
            gross_gain_amount: lot_result.long_term_gain,
            federal_tax: federal_long_term_tax,
            state_tax: state_long_term_tax,
        });

        net_amount -= federal_long_term_tax + state_long_term_tax;
    }

    effects.push(EvalEvent::CashCredit {
        to: params.to_account,
        net_amount,
        kind: CashFlowKind::LiquidationProceeds,
    });

    (
        LiquidationResult {
            gross_amount,
            net_proceeds: net_amount,
        },
        effects,
    )
}

/// Liquidate from a tax-deferred account (Traditional IRA, 401k)
/// All withdrawals are taxed as ordinary income
fn liquidate_tax_deferred(
    lots: &[AssetLot],
    amount: f64,
    params: &LiquidationParams,
) -> (LiquidationResult, Vec<EvalEvent>) {
    let lot_result = consume_lots(
        lots,
        amount,
        params.current_price,
        params.lot_method,
        params.current_date,
    );
    let mut effects = lot_subtractions_to_effects(params.asset_coord, &lot_result);

    let gross_amount = lot_result.proceeds;

    // Entire withdrawal taxed as ordinary income
    let federal_tax = calculate_federal_marginal_tax(
        gross_amount,
        params.ytd_ordinary_income,
        &params.tax_config.federal_brackets,
    );
    let state_tax = gross_amount * params.tax_config.state_rate;
    let net_amount = gross_amount - federal_tax - state_tax;

    effects.push(EvalEvent::IncomeTax {
        gross_income_amount: gross_amount,
        federal_tax,
        state_tax,
    });

    effects.push(EvalEvent::CashCredit {
        to: params.to_account,
        net_amount,
        kind: CashFlowKind::LiquidationProceeds,
    });

    (
        LiquidationResult {
            gross_amount,
            net_proceeds: net_amount,
        },
        effects,
    )
}

/// Liquidate from a tax-free account (Roth IRA)
/// Qualified withdrawals are completely tax-free
fn liquidate_tax_free(
    lots: &[AssetLot],
    amount: f64,
    params: &LiquidationParams,
) -> (LiquidationResult, Vec<EvalEvent>) {
    let lot_result = consume_lots(
        lots,
        amount,
        params.current_price,
        params.lot_method,
        params.current_date,
    );
    let mut effects = lot_subtractions_to_effects(params.asset_coord, &lot_result);

    let gross_amount = lot_result.proceeds;
    let net_amount = gross_amount; // No taxes

    effects.push(EvalEvent::CashCredit {
        to: params.to_account,
        net_amount,
        kind: CashFlowKind::LiquidationProceeds,
    });

    (
        LiquidationResult {
            gross_amount,
            net_proceeds: net_amount,
        },
        effects,
    )
}

/// Result of lot consumption calculation
#[derive(Debug, Clone, Default)]
pub struct LotConsumptionResult {
    /// Number of units consumed
    pub units_consumed: f64,
    /// Total cost basis of consumed units
    pub cost_basis: f64,
    /// Total proceeds from sale (units * current_price)
    pub proceeds: f64,
    /// Short-term capital gain (held < 1 year)
    pub short_term_gain: f64,
    /// Long-term capital gain (held >= 1 year)
    pub long_term_gain: f64,
    /// Individual lot subtractions to apply
    pub lot_subtractions: Vec<LotSubtraction>,
}

/// Represents a single lot subtraction with gain/loss information
#[derive(Debug, Clone)]
pub struct LotSubtraction {
    pub lot_date: Date,
    pub units: f64,
    pub cost_basis: f64,
    pub proceeds: f64,
    pub short_term_gain: f64,
    pub long_term_gain: f64,
}

/// Consume lots to satisfy a liquidation amount (in dollar value at current prices)
///
/// # Arguments
/// * `lots` - The lots to consume from (read-only)
/// * `amount` - The dollar amount to liquidate (at current prices)
/// * `current_price` - Current price per unit of the asset
/// * `method` - Which lot selection method to use
/// * `current_date` - Current date for determining short/long-term holding period
pub fn consume_lots(
    lots: &[AssetLot],
    amount: f64,
    current_price: f64,
    method: LotMethod,
    current_date: Date,
) -> LotConsumptionResult {
    if amount <= 0.0 || lots.is_empty() || current_price <= 0.0 {
        return LotConsumptionResult::default();
    }

    // Convert dollar amount to units
    let units_to_sell = amount / current_price;

    if method == LotMethod::AverageCost {
        consume_lots_average_cost(lots, units_to_sell, current_price, current_date)
    } else {
        let sorted_lots = sort_lots_by_method(lots, method);
        consume_lots_specific(&sorted_lots, units_to_sell, current_price, current_date)
    }
}

/// Sort lots according to the specified method
fn sort_lots_by_method(lots: &[AssetLot], method: LotMethod) -> Vec<AssetLot> {
    let mut sorted: Vec<AssetLot> = lots.to_vec();
    match method {
        LotMethod::Fifo => sorted.sort_by_key(|l| l.purchase_date),
        LotMethod::Lifo => sorted.sort_by(|a, b| b.purchase_date.cmp(&a.purchase_date)),
        LotMethod::HighestCost => sorted.sort_by(|a, b| {
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
        LotMethod::LowestCost => sorted.sort_by(|a, b| {
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
        LotMethod::AverageCost => {} // No sorting needed
    }
    sorted
}

/// Consume lots using average cost basis method
fn consume_lots_average_cost(
    lots: &[AssetLot],
    units_to_sell: f64,
    current_price: f64,
    current_date: Date,
) -> LotConsumptionResult {
    let total_units: f64 = lots.iter().map(|l| l.units).sum();
    let total_basis: f64 = lots.iter().map(|l| l.cost_basis).sum();

    if total_units <= 0.0 {
        return LotConsumptionResult::default();
    }

    let actual_units = units_to_sell.min(total_units);
    let proportion = actual_units / total_units;
    let avg_basis_per_unit = total_basis / total_units;

    let mut result = LotConsumptionResult {
        units_consumed: actual_units,
        proceeds: actual_units * current_price,
        ..Default::default()
    };

    for lot in lots.iter() {
        let units_from_lot = lot.units * proportion;
        if units_from_lot <= 0.001 {
            continue;
        }

        // Cost basis uses average
        let basis_for_portion = units_from_lot * avg_basis_per_unit;
        result.cost_basis += basis_for_portion;

        // Calculate gain using current market price
        let proceeds_from_portion = units_from_lot * current_price;
        let gain = proceeds_from_portion - basis_for_portion;

        // Classify by holding period
        let holding_days = (current_date - lot.purchase_date).get_days();
        let (short_term, long_term) = if holding_days >= 365 {
            result.long_term_gain += gain.max(0.0);
            (0.0, gain.max(0.0))
        } else {
            result.short_term_gain += gain.max(0.0);
            (gain.max(0.0), 0.0)
        };

        // Record subtraction with actual lot basis and gains
        result.lot_subtractions.push(LotSubtraction {
            lot_date: lot.purchase_date,
            units: units_from_lot,
            cost_basis: lot.cost_basis * proportion,
            proceeds: proceeds_from_portion,
            short_term_gain: short_term,
            long_term_gain: long_term,
        });
    }

    result
}

/// Consume lots using specific identification (FIFO, LIFO, HighestCost, LowestCost)
fn consume_lots_specific(
    lots: &[AssetLot],
    units_to_sell: f64,
    current_price: f64,
    current_date: Date,
) -> LotConsumptionResult {
    let mut result = LotConsumptionResult::default();
    let mut remaining_units = units_to_sell;

    for lot in lots.iter() {
        if remaining_units <= 0.001 {
            break;
        }

        let take_units = remaining_units.min(lot.units);
        let take_fraction = if lot.units > 0.0 {
            take_units / lot.units
        } else {
            0.0
        };
        let basis_used = lot.cost_basis * take_fraction;

        result.units_consumed += take_units;
        result.cost_basis += basis_used;

        let proceeds = take_units * current_price;
        result.proceeds += proceeds;
        let gain = proceeds - basis_used;

        // Classify by holding period
        let holding_days = (current_date - lot.purchase_date).get_days();
        let (short_term, long_term) = if holding_days >= 365 {
            result.long_term_gain += gain.max(0.0);
            (0.0, gain.max(0.0))
        } else {
            result.short_term_gain += gain.max(0.0);
            (gain.max(0.0), 0.0)
        };

        result.lot_subtractions.push(LotSubtraction {
            lot_date: lot.purchase_date,
            units: take_units,
            cost_basis: basis_used,
            proceeds,
            short_term_gain: short_term,
            long_term_gain: long_term,
        });

        remaining_units -= take_units;
    }

    result
}

/// Convert lot consumption result to StateEvent lot subtractions
pub fn lot_subtractions_to_effects(
    asset_coord: AssetCoord,
    result: &LotConsumptionResult,
) -> Vec<EvalEvent> {
    result
        .lot_subtractions
        .iter()
        .map(|sub| EvalEvent::SubtractAssetLot {
            from: asset_coord,
            lot_date: sub.lot_date,
            units: sub.units,
            cost_basis: sub.cost_basis,
            proceeds: sub.proceeds,
            short_term_gain: sub.short_term_gain,
            long_term_gain: sub.long_term_gain,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Lot-Based Tax Calculation Tests
    // ========================================================================

    #[test]
    fn test_consume_lots_fifo() {
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1); // > 1 year ago
        let recent_date = jiff::civil::date(2025, 3, 1); // < 1 year ago
        let asset_id = AssetId(1);

        let lots = vec![
            AssetLot {
                asset_id,
                purchase_date: old_date,
                units: 100.0,
                cost_basis: 80.0,
            },
            AssetLot {
                asset_id,
                purchase_date: recent_date,
                units: 100.0,
                cost_basis: 95.0,
            },
        ];

        let result = consume_lots(&lots, 120.0, 1.0, LotMethod::Fifo, today);

        // Should consume from oldest lot first: 100 from old + 20 from recent
        assert!(
            (result.units_consumed - 120.0).abs() < 0.01,
            "Expected 120 consumed, got {}",
            result.units_consumed
        );
        // Cost basis: 80 (full old lot) + 19 (20% of recent lot's 95) = 99
        assert!(
            (result.cost_basis - 99.0).abs() < 0.01,
            "Expected 99 basis, got {}",
            result.cost_basis
        );
        // Old lot gain: 100 - 80 = 20, held > 1 year = long-term
        assert!(
            (result.long_term_gain - 20.0).abs() < 0.01,
            "Expected 20 LT gain, got {}",
            result.long_term_gain
        );
        // Recent lot gain: 20 - 19 = 1, held < 1 year = short-term
        assert!(
            (result.short_term_gain - 1.0).abs() < 0.01,
            "Expected 1 ST gain, got {}",
            result.short_term_gain
        );
        // Should have 2 lot subtractions
        assert_eq!(result.lot_subtractions.len(), 2);
    }

    #[test]
    fn test_consume_lots_lifo() {
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1); // > 1 year ago
        let recent_date = jiff::civil::date(2025, 3, 1); // < 1 year ago
        let asset_id = AssetId(1);

        let lots = vec![
            AssetLot {
                asset_id,
                purchase_date: old_date,
                units: 100.0,
                cost_basis: 80.0,
            },
            AssetLot {
                asset_id,
                purchase_date: recent_date,
                units: 100.0,
                cost_basis: 95.0,
            },
        ];

        let result = consume_lots(&lots, 120.0, 1.0, LotMethod::Lifo, today);

        // Should consume from newest lot first: 100 from recent + 20 from old
        // Cost basis: 95 (full recent lot) + 16 (20% of old lot's 80) = 111
        assert!(
            (result.cost_basis - 111.0).abs() < 0.01,
            "Expected 111 basis, got {}",
            result.cost_basis
        );
        // Recent lot gain: 100 - 95 = 5, held < 1 year = short-term
        assert!(
            (result.short_term_gain - 5.0).abs() < 0.01,
            "Expected 5 ST gain, got {}",
            result.short_term_gain
        );
        // Old lot gain: 20 - 16 = 4, held > 1 year = long-term
        assert!(
            (result.long_term_gain - 4.0).abs() < 0.01,
            "Expected 4 LT gain, got {}",
            result.long_term_gain
        );
        // Should have 2 lot subtractions
        assert_eq!(result.lot_subtractions.len(), 2);
    }

    #[test]
    fn test_consume_lots_highest_cost() {
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1);
        let asset_id = AssetId(1);

        let lots = vec![
            AssetLot {
                asset_id,
                purchase_date: old_date,
                units: 100.0,
                cost_basis: 80.0,
            }, // $0.80/unit
            AssetLot {
                asset_id,
                purchase_date: old_date,
                units: 100.0,
                cost_basis: 120.0,
            }, // $1.20/unit
        ];

        let result = consume_lots(&lots, 120.0, 1.0, LotMethod::HighestCost, today);

        // Should consume from highest cost lot first: 100 from $1.20/unit + 20 from $0.80/unit
        // Cost basis: 120 (full high-cost lot) + 16 (20% of low-cost lot's 80) = 136
        assert!(
            (result.cost_basis - 136.0).abs() < 0.01,
            "Expected 136 basis, got {}",
            result.cost_basis
        );
        // High-cost lot: 100 proceeds - 120 basis = -20 (loss, not tracked as gain)
        // Low-cost lot portion: 20 proceeds - 16 basis = 4 gain (long-term)
        assert!(
            (result.long_term_gain - 4.0).abs() < 0.01,
            "Expected 4 LT gain, got {}",
            result.long_term_gain
        );
        // Should have 2 lot subtractions
        assert_eq!(result.lot_subtractions.len(), 2);
    }

    #[test]
    fn test_consume_lots_partial() {
        let today = jiff::civil::date(2025, 6, 15);
        let old_date = jiff::civil::date(2024, 1, 1);
        let asset_id = AssetId(1);

        let lots = vec![AssetLot {
            asset_id,
            purchase_date: old_date,
            units: 100.0,
            cost_basis: 80.0,
        }];

        let result = consume_lots(&lots, 50.0, 1.0, LotMethod::Fifo, today);

        assert!((result.units_consumed - 50.0).abs() < 0.01);
        assert!((result.cost_basis - 40.0).abs() < 0.01); // Half of 80
        assert!((result.long_term_gain - 10.0).abs() < 0.01); // 50 - 40 = 10
        // Should have 1 lot subtraction recording the partial consumption
        assert_eq!(result.lot_subtractions.len(), 1);
        assert!((result.lot_subtractions[0].units - 50.0).abs() < 0.01);
        assert!((result.lot_subtractions[0].cost_basis - 40.0).abs() < 0.01);
    }
}
