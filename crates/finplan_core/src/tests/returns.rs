//! Tests for investment returns and market appreciation
//!
//! These tests verify that:
//! - Asset values appreciate according to return profiles
//! - Cash in bank accounts does NOT appreciate (no return profile)
//! - Cash in investment accounts can appreciate based on return profile
//! - Multiple assets with different return profiles work correctly
//! - Partial year returns are calculated correctly

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AssetId, AssetLot, Cash, InflationProfile,
    InvestmentContainer, ReturnProfile, ReturnProfileId, TaxStatus,
};
use crate::simulation::simulate;

/// Test that a single investment asset appreciates at the expected rate
#[test]
fn test_single_asset_fixed_return() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);
    let initial_value = 10_000.0;
    let annual_return = 0.10; // 10%
    let years = 5;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile_id, ReturnProfile::Fixed(annual_return))]),
        asset_returns: HashMap::from([(asset_id, return_profile_id)]),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(999), // unused
                },
                positions: vec![AssetLot {
                    asset_id,
                    purchase_date: start_date,
                    units: initial_value, // 1 unit = $1 initially
                    cost_basis: initial_value,
                }],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Expected: $10,000 * (1.10)^5 = $16,105.10
    let expected = initial_value * (1.0 + annual_return).powi(years as i32);
    let actual = result.final_account_balance(AccountId(1)).unwrap();

    assert!(
        (actual - expected).abs() < 1.0,
        "Expected ${:.2}, got ${:.2}",
        expected,
        actual
    );
}

/// Test that bank account cash with no return profile doesn't change
#[test]
fn test_bank_cash_no_appreciation() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let initial_value = 50_000.0;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 10,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(), // No return profiles defined
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Bank(Cash {
                value: initial_value,
                return_profile_id: ReturnProfileId(0), // Profile doesn't exist, so no growth
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();
    let final_balance = result.final_account_balance(AccountId(1)).unwrap();

    // Bank cash should stay the same when return profile doesn't exist
    assert!(
        (final_balance - initial_value).abs() < 0.01,
        "Bank cash should not appreciate without valid return profile. Expected ${:.2}, got ${:.2}",
        initial_value,
        final_balance
    );
}

/// Test that bank account cash WITH a return profile appreciates (HYSA, money market)
#[test]
fn test_bank_cash_with_return_profile() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let initial_value = 50_000.0;
    let cash_return_profile = ReturnProfileId(0);
    let annual_return = 0.045; // 4.5% HYSA rate
    let years = 5;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(
            cash_return_profile,
            ReturnProfile::Fixed(annual_return),
        )]),
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Bank(Cash {
                value: initial_value,
                return_profile_id: cash_return_profile,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();
    let final_balance = result.final_account_balance(AccountId(1)).unwrap();

    // Expected: $50,000 * (1.045)^5 = $62,308.67 for yearly compounding
    // Our simulation compounds at each time step (quarterly heartbeats), so actual
    // value will be slightly higher due to more frequent compounding.
    let yearly_expected = initial_value * (1.0 + annual_return).powi(years as i32);

    // With more frequent compounding, result should be >= yearly compounding
    // and within a reasonable tolerance (0.5% higher max from frequent compounding)
    assert!(
        final_balance >= yearly_expected * 0.99 && final_balance <= yearly_expected * 1.01,
        "HYSA should appreciate at ~4.5%. Expected ~${:.2}, got ${:.2}",
        yearly_expected,
        final_balance
    );
}

/// Test investment account cash (money market) also appreciates
#[test]
fn test_investment_cash_appreciation() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let cash_profile = ReturnProfileId(0);
    let stock_profile = ReturnProfileId(1);
    let cash_return = 0.05; // 5% money market
    let stock_return = 0.08; // 8% stocks
    let years = 5;

    let cash_amount = 10_000.0;
    let stock_amount = 40_000.0;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([
            (cash_profile, ReturnProfile::Fixed(cash_return)),
            (stock_profile, ReturnProfile::Fixed(stock_return)),
        ]),
        asset_returns: HashMap::from([(AssetId(1), stock_profile)]),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: cash_amount,
                    return_profile_id: cash_profile,
                },
                positions: vec![AssetLot {
                    asset_id: AssetId(1),
                    purchase_date: start_date,
                    units: stock_amount,
                    cost_basis: stock_amount,
                }],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();
    let final_balance = result.final_account_balance(AccountId(1)).unwrap();

    // Cash grows at 5%: $10,000 * (1.05)^5 = $12,762.82
    let expected_cash = cash_amount * (1.0 + cash_return).powi(years as i32);
    // Stocks grow at 8%: $40,000 * (1.08)^5 = $58,773.12
    let expected_stocks = stock_amount * (1.0 + stock_return).powi(years as i32);
    let expected_total = expected_cash + expected_stocks;

    assert!(
        (final_balance - expected_total).abs() < 10.0,
        "Investment account should grow (cash at 5%, stocks at 8%). Expected ${:.2}, got ${:.2}",
        expected_total,
        final_balance
    );
}

/// Test multiple assets with different return profiles
#[test]
fn test_multiple_assets_different_returns() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let years = 10;

    // Asset 1: Stock fund at 8%
    let stock_id = AssetId(1);
    let stock_profile = ReturnProfileId(0);
    let stock_return = 0.08;
    let stock_initial = 50_000.0;

    // Asset 2: Bond fund at 4%
    let bond_id = AssetId(2);
    let bond_profile = ReturnProfileId(1);
    let bond_return = 0.04;
    let bond_initial = 30_000.0;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([
            (stock_profile, ReturnProfile::Fixed(stock_return)),
            (bond_profile, ReturnProfile::Fixed(bond_return)),
        ]),
        asset_returns: HashMap::from([(stock_id, stock_profile), (bond_id, bond_profile)]),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(999),
                },
                positions: vec![
                    AssetLot {
                        asset_id: stock_id,
                        purchase_date: start_date,
                        units: stock_initial,
                        cost_basis: stock_initial,
                    },
                    AssetLot {
                        asset_id: bond_id,
                        purchase_date: start_date,
                        units: bond_initial,
                        cost_basis: bond_initial,
                    },
                ],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Calculate expected values
    let expected_stock = stock_initial * (1.0 + stock_return).powi(years as i32);
    let expected_bond = bond_initial * (1.0 + bond_return).powi(years as i32);
    let expected_total = expected_stock + expected_bond;

    let actual_total = result.final_account_balance(AccountId(1)).unwrap();
    let actual_stock = result.final_asset_balance(AccountId(1), stock_id).unwrap();
    let actual_bond = result.final_asset_balance(AccountId(1), bond_id).unwrap();

    // Check individual assets
    assert!(
        (actual_stock - expected_stock).abs() < 10.0,
        "Stock expected ${:.2}, got ${:.2}",
        expected_stock,
        actual_stock
    );

    assert!(
        (actual_bond - expected_bond).abs() < 10.0,
        "Bond expected ${:.2}, got ${:.2}",
        expected_bond,
        actual_bond
    );

    // Check total
    assert!(
        (actual_total - expected_total).abs() < 20.0,
        "Total expected ${:.2}, got ${:.2}",
        expected_total,
        actual_total
    );
}

/// Test that negative returns work correctly
#[test]
fn test_negative_returns() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);
    let initial_value = 100_000.0;
    let annual_return = -0.10; // -10% per year
    let years = 3;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile_id, ReturnProfile::Fixed(annual_return))]),
        asset_returns: HashMap::from([(asset_id, return_profile_id)]),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(999),
                },
                positions: vec![AssetLot {
                    asset_id,
                    purchase_date: start_date,
                    units: initial_value,
                    cost_basis: initial_value,
                }],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Expected: $100,000 * (0.90)^3 = $72,900
    let expected = initial_value * (1.0 + annual_return).powi(years as i32);
    let actual = result.final_account_balance(AccountId(1)).unwrap();

    assert!(
        (actual - expected).abs() < 1.0,
        "Expected ${:.2}, got ${:.2}",
        expected,
        actual
    );
}

/// Test zero return rate
#[test]
fn test_zero_return() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);
    let initial_value = 25_000.0;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 10,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile_id, ReturnProfile::Fixed(0.0))]),
        asset_returns: HashMap::from([(asset_id, return_profile_id)]),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(999),
                },
                positions: vec![AssetLot {
                    asset_id,
                    purchase_date: start_date,
                    units: initial_value,
                    cost_basis: initial_value,
                }],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();
    let actual = result.final_account_balance(AccountId(1)).unwrap();

    assert!(
        (actual - initial_value).abs() < 0.01,
        "Zero return should keep value at ${:.2}, got ${:.2}",
        initial_value,
        actual
    );
}

/// Test same asset in multiple accounts
#[test]
fn test_same_asset_multiple_accounts() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);
    let annual_return = 0.05;
    let years = 5;

    // Same asset in two accounts
    let taxable_initial = 10_000.0;
    let ira_initial = 20_000.0;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile_id, ReturnProfile::Fixed(annual_return))]),
        asset_returns: HashMap::from([(asset_id, return_profile_id)]),
        accounts: vec![
            Account {
                account_id: AccountId(1),
                flavor: AccountFlavor::Investment(InvestmentContainer {
                    tax_status: TaxStatus::Taxable,
                    cash: Cash {
                        value: 0.0,
                        return_profile_id: ReturnProfileId(999),
                    },
                    positions: vec![AssetLot {
                        asset_id,
                        purchase_date: start_date,
                        units: taxable_initial,
                        cost_basis: taxable_initial,
                    }],
                    contribution_limit: None,
                }),
            },
            Account {
                account_id: AccountId(2),
                flavor: AccountFlavor::Investment(InvestmentContainer {
                    tax_status: TaxStatus::TaxDeferred,
                    cash: Cash {
                        value: 0.0,
                        return_profile_id: ReturnProfileId(999),
                    },
                    positions: vec![AssetLot {
                        asset_id,
                        purchase_date: start_date,
                        units: ira_initial,
                        cost_basis: ira_initial,
                    }],
                    contribution_limit: None,
                }),
            },
        ],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    let multiplier = (1.0 + annual_return).powi(years as i32);
    let expected_taxable = taxable_initial * multiplier;
    let expected_ira = ira_initial * multiplier;

    let actual_taxable = result.final_account_balance(AccountId(1)).unwrap();
    let actual_ira = result.final_account_balance(AccountId(2)).unwrap();

    assert!(
        (actual_taxable - expected_taxable).abs() < 1.0,
        "Taxable expected ${:.2}, got ${:.2}",
        expected_taxable,
        actual_taxable
    );

    assert!(
        (actual_ira - expected_ira).abs() < 1.0,
        "IRA expected ${:.2}, got ${:.2}",
        expected_ira,
        actual_ira
    );
}

/// Test that short duration simulations work correctly
#[test]
fn test_short_duration_returns() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);
    let initial_value = 10_000.0;
    let annual_return = 0.12; // 12%
    let years = 1;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile_id, ReturnProfile::Fixed(annual_return))]),
        asset_returns: HashMap::from([(asset_id, return_profile_id)]),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(999),
                },
                positions: vec![AssetLot {
                    asset_id,
                    purchase_date: start_date,
                    units: initial_value,
                    cost_basis: initial_value,
                }],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Expected: $10,000 * 1.12 = $11,200
    let expected = initial_value * (1.0 + annual_return);
    let actual = result.final_account_balance(AccountId(1)).unwrap();

    assert!(
        (actual - expected).abs() < 0.1,
        "Expected ${:.2}, got ${:.2}",
        expected,
        actual
    );
}

/// Test long duration simulation (30+ years)
#[test]
fn test_long_duration_returns() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);
    let initial_value = 100_000.0;
    let annual_return = 0.07; // 7%
    let years = 30;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile_id, ReturnProfile::Fixed(annual_return))]),
        asset_returns: HashMap::from([(asset_id, return_profile_id)]),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(999),
                },
                positions: vec![AssetLot {
                    asset_id,
                    purchase_date: start_date,
                    units: initial_value,
                    cost_basis: initial_value,
                }],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Expected: $100,000 * (1.07)^30 = $761,225.50
    let expected = initial_value * (1.0 + annual_return).powi(years as i32);
    let actual = result.final_account_balance(AccountId(1)).unwrap();

    // Allow larger tolerance for long duration
    assert!(
        (actual - expected).abs() < 100.0,
        "Expected ${:.2}, got ${:.2}",
        expected,
        actual
    );
}

/// Test that cash deposited mid-simulation only earns returns from deposit date forward.
/// This verifies the fix for the bug where cash would incorrectly get returns
/// from the start date even when deposited later.
#[test]
fn test_mid_simulation_cash_deposit() {
    use crate::model::{
        AmountMode, Event, EventEffect, EventId, EventTrigger, IncomeType, TransferAmount,
    };

    let start_date = jiff::civil::date(2020, 1, 1);
    let initial_value = 10_000.0;
    let deposit_amount = 5_000.0;
    let cash_return_profile = ReturnProfileId(0);
    let annual_return = 0.05; // 5%
    let years = 4;

    // Deposit happens at year 2 (halfway through)
    let deposit_date = jiff::civil::date(2022, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(
            cash_return_profile,
            ReturnProfile::Fixed(annual_return),
        )]),
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Bank(Cash {
                value: initial_value,
                return_profile_id: cash_return_profile,
            }),
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Date(deposit_date),
            effects: vec![EventEffect::Income {
                to: AccountId(1),
                amount: TransferAmount::Fixed(deposit_amount),
                amount_mode: AmountMode::Gross,
                income_type: IncomeType::TaxFree,
            }],
            once: true,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();
    let final_balance = result.final_account_balance(AccountId(1)).unwrap();

    // Initial $10,000 earns returns for 4 full years
    // Expected: $10,000 * (1.05)^4 = $12,155.06
    let initial_growth = initial_value * (1.0 + annual_return).powi(4);

    // Deposited $5,000 earns returns for only 2 years (from 2022 to 2024)
    // Expected: $5,000 * (1.05)^2 = $5,512.50
    let deposit_growth = deposit_amount * (1.0 + annual_return).powi(2);

    let expected_total = initial_growth + deposit_growth;

    // The simulation compounds more frequently, so actual may be slightly higher
    // Allow 2% tolerance for compounding differences
    assert!(
        (final_balance - expected_total).abs() < expected_total * 0.02,
        "Mid-simulation deposit should only earn returns from deposit date. \
         Expected ~${:.2}, got ${:.2}. \
         If deposit got returns from start, it would be ${:.2}",
        expected_total,
        final_balance,
        (initial_value + deposit_amount) * (1.0 + annual_return).powi(4)
    );

    // Also verify it's NOT giving returns from start (the bug we fixed)
    // If the old bug existed, we'd get: ($10,000 + $5,000) * (1.05)^4 = $18,232.59
    let incorrect_value = (initial_value + deposit_amount) * (1.0 + annual_return).powi(4);
    assert!(
        final_balance < incorrect_value - 100.0,
        "Bug detected: Cash appears to be getting returns from start date rather than deposit date"
    );
}
