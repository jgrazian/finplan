//! Tests for account operations and structures
//!
//! These tests verify:
//! - Different account types (Bank, Investment, Property, Liability)
//! - Account balance calculations
//! - Multiple lots in an account
//! - Investment container with cash + positions

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AssetId, AssetLot, Cash, FixedAsset, InflationProfile,
    InvestmentContainer, LoanDetail, ReturnProfile, ReturnProfileId, TaxStatus,
};
use crate::simulation::simulate;

/// Test investment account with both cash and positions
#[test]
fn test_investment_account_cash_and_positions() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);
    let annual_return = 0.05;
    let years = 5;

    let cash_amount = 5_000.0;
    let position_amount = 15_000.0;

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
                    value: cash_amount,
                    return_profile_id: ReturnProfileId(999), // Cash doesn't grow in investments
                },
                positions: vec![AssetLot {
                    asset_id,
                    purchase_date: start_date,
                    units: position_amount,
                    cost_basis: position_amount,
                }],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Position grows, cash stays the same
    let expected_position = position_amount * (1.0 + annual_return).powi(years as i32);
    let expected_total = expected_position + cash_amount;

    let actual = result.final_account_balance(AccountId(1)).unwrap();

    assert!(
        (actual - expected_total).abs() < 1.0,
        "Expected total ${expected_total:.2} (position ${expected_position:.2} + cash ${cash_amount:.2}), got ${actual:.2}"
    );
}

/// Test multiple lots of the same asset
#[test]
fn test_multiple_lots_same_asset() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let earlier_date = jiff::civil::date(2018, 6, 15);
    let later_date = jiff::civil::date(2019, 9, 20);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);
    let annual_return = 0.10;
    let years = 5;

    // Two lots of the same asset bought at different times
    let lot1_units = 100.0;
    let lot1_basis = 80.0;
    let lot2_units = 50.0;
    let lot2_basis = 55.0;

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
                positions: vec![
                    AssetLot {
                        asset_id,
                        purchase_date: earlier_date,
                        units: lot1_units,
                        cost_basis: lot1_basis,
                    },
                    AssetLot {
                        asset_id,
                        purchase_date: later_date,
                        units: lot2_units,
                        cost_basis: lot2_basis,
                    },
                ],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Both lots track units; final value = total_units * current_price
    // Current price starts at 1.0 and grows at 10% annually
    let total_units = lot1_units + lot2_units;
    let price_growth = (1.0 + annual_return).powi(years as i32);
    let expected_value = total_units * price_growth;

    let actual = result.final_account_balance(AccountId(1)).unwrap();

    assert!(
        (actual - expected_value).abs() < 1.0,
        "Expected ${expected_value:.2}, got ${actual:.2}"
    );
}

/// Test property accounts with fixed assets (separate accounts for house and car)
#[test]
fn test_property_account_appreciation() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let house_id = AssetId(1);
    let car_id = AssetId(2);
    let house_profile = ReturnProfileId(0);
    let car_profile = ReturnProfileId(1);
    let years = 10;

    let house_value = 500_000.0;
    let house_return = 0.03; // 3% appreciation
    let car_value = 30_000.0;
    let car_return = -0.15; // -15% depreciation

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([
            (house_profile, ReturnProfile::Fixed(house_return)),
            (car_profile, ReturnProfile::Fixed(car_return)),
        ]),
        asset_returns: HashMap::from([(house_id, house_profile), (car_id, car_profile)]),
        accounts: vec![
            Account {
                account_id: AccountId(1),
                flavor: AccountFlavor::Property(FixedAsset {
                    asset_id: house_id,
                    value: house_value,
                }),
            },
            Account {
                account_id: AccountId(2),
                flavor: AccountFlavor::Property(FixedAsset {
                    asset_id: car_id,
                    value: car_value,
                }),
            },
        ],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Property assets appreciate via Market using their assigned return profiles
    let house_actual = result.final_account_balance(AccountId(1)).unwrap();
    let car_actual = result.final_account_balance(AccountId(2)).unwrap();

    // Expected: house appreciates at 3%, car depreciates at -15% over 10 years
    let expected_house = house_value * (1.0 + house_return).powi(years as i32);
    let expected_car = car_value * (1.0 + car_return).powi(years as i32);

    assert!(
        (house_actual - expected_house).abs() < 1.0,
        "House expected ${expected_house:.2}, got ${house_actual:.2}"
    );
    assert!(
        (car_actual - expected_car).abs() < 1.0,
        "Car expected ${expected_car:.2}, got ${car_actual:.2}"
    );
}

/// Test liability account
#[test]
fn test_liability_account_negative_balance() {
    let start_date = jiff::civil::date(2020, 1, 1);

    let loan_principal = 250_000.0;
    let loan_rate = 0.065;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Liability(LoanDetail {
                principal: loan_principal,
                interest_rate: loan_rate,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();
    let actual = result.final_account_balance(AccountId(1)).unwrap();

    // Liability accrues interest over time, so it grows from the initial principal
    // The simulation compounds continuously (daily rate) while this formula uses annual
    // Expected: approximately $250,000 × (1.065)^5 ≈ $342,640 (shown as negative)
    let expected = -loan_principal * (1.0 + loan_rate).powi(5);
    assert!(
        (actual - expected).abs() < 200.0, // Allow for compounding method differences
        "Liability should be ${expected:.2}, got ${actual:.2}"
    );
}

/// Test tax statuses don't affect appreciation
#[test]
fn test_tax_status_same_returns() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);
    let annual_return = 0.08;
    let years = 10;
    let initial_value = 50_000.0;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile_id, ReturnProfile::Fixed(annual_return))]),
        asset_returns: HashMap::from([(asset_id, return_profile_id)]),
        accounts: vec![
            // Taxable account
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
                        units: initial_value,
                        cost_basis: initial_value,
                    }],
                    contribution_limit: None,
                }),
            },
            // Tax-deferred (401k/IRA)
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
                        units: initial_value,
                        cost_basis: initial_value,
                    }],
                    contribution_limit: None,
                }),
            },
            // Tax-free (Roth)
            Account {
                account_id: AccountId(3),
                flavor: AccountFlavor::Investment(InvestmentContainer {
                    tax_status: TaxStatus::TaxFree,
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
            },
        ],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    let expected = initial_value * (1.0 + annual_return).powi(years as i32);

    let taxable = result.final_account_balance(AccountId(1)).unwrap();
    let deferred = result.final_account_balance(AccountId(2)).unwrap();
    let tax_free = result.final_account_balance(AccountId(3)).unwrap();
    // All should have same ending value (tax status doesn't affect appreciation)
    assert!(
        (taxable - expected).abs() < 1.0,
        "Taxable expected ${expected:.2}, got ${taxable:.2}"
    );
    assert!(
        (deferred - expected).abs() < 1.0,
        "Tax-deferred expected ${expected:.2}, got ${deferred:.2}"
    );
    assert!(
        (tax_free - expected).abs() < 1.0,
        "Tax-free expected ${expected:.2}, got ${tax_free:.2}"
    );
}

/// Test empty account
#[test]
fn test_empty_investment_account() {
    let start_date = jiff::civil::date(2020, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(999),
                },
                positions: vec![],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();
    let actual = result.final_account_balance(AccountId(1)).unwrap();

    assert!(
        actual.abs() < 0.01,
        "Empty account should have $0 balance, got ${actual:.2}"
    );
}

/// Test account balance query for non-existent account
#[test]
fn test_nonexistent_account_returns_zero() {
    let start_date = jiff::civil::date(2020, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 1,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![], // No accounts
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Query non-existent account should return 0
    let balance = result.final_account_balance(AccountId(999));
    assert!(balance.is_none(), "Non-existent account should be None");
}
