//! Basic simulation tests
//!
//! Tests for core simulation mechanics: cash flows, returns, inflation, limits.

use crate::accounts::{Account, AccountType, Asset, AssetClass};
use crate::cash_flows::{
    CashFlow, CashFlowDirection, CashFlowLimits, CashFlowState, LimitPeriod, RepeatInterval,
};
use crate::config::SimulationParameters;
use crate::ids::{AccountId, AssetId, CashFlowId};
use crate::profiles::{InflationProfile, ReturnProfile};
use crate::simulation::{monte_carlo_simulate, simulate};

#[test]
fn test_monte_carlo_simulation() {
    let params = SimulationParameters {
        start_date: None,
        duration_years: 30,
        birth_date: None,
        inflation_profile: InflationProfile::Fixed(0.02),
        return_profiles: vec![ReturnProfile::Normal {
            mean: 0.07,
            std_dev: 0.15,
        }],
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 10_000.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
            account_type: AccountType::Taxable,
        }],
        cash_flows: vec![],
        ..Default::default()
    };

    const NUM_ITERATIONS: usize = 10_000;
    let result = monte_carlo_simulate(&params, NUM_ITERATIONS);
    assert_eq!(result.iterations.len(), NUM_ITERATIONS);

    // Check that results are different (due to random seed)
    let first_final = result.iterations[0].final_account_balance(AccountId(1));
    let second_final = result.iterations[1].final_account_balance(AccountId(1));

    assert_ne!(first_final, second_final);
}

#[test]
fn test_simulation_basic() {
    let params = SimulationParameters {
        start_date: None,
        duration_years: 10,
        birth_date: None,
        inflation_profile: InflationProfile::Fixed(0.02),
        return_profiles: vec![ReturnProfile::Fixed(0.05)],
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 10_000.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
            account_type: AccountType::Taxable,
        }],
        cash_flows: vec![CashFlow {
            cash_flow_id: CashFlowId(1),
            amount: 100.0,
            repeats: RepeatInterval::Monthly,
            cash_flow_limits: None,
            adjust_for_inflation: false,
            direction: CashFlowDirection::Income {
                target_account_id: AccountId(1),
                target_asset_id: AssetId(1),
            },
            state: CashFlowState::Active,
        }],
        ..Default::default()
    };

    let _result = simulate(&params, 42);
}

#[test]
fn test_cashflow_limits() {
    let params = SimulationParameters {
        start_date: Some(jiff::civil::date(2022, 1, 1)),
        duration_years: 10,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 10_000.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
            account_type: AccountType::Taxable,
        }],
        cash_flows: vec![CashFlow {
            cash_flow_id: CashFlowId(1),
            amount: 100.0,
            repeats: RepeatInterval::Monthly,
            cash_flow_limits: Some(CashFlowLimits {
                limit: 1_000.0,
                limit_period: LimitPeriod::Yearly,
            }),
            adjust_for_inflation: false,
            direction: CashFlowDirection::Income {
                target_account_id: AccountId(1),
                target_asset_id: AssetId(1),
            },
            state: CashFlowState::Active,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Initial: 10,000
    // Contribution: 100/month -> 1200/year.
    // Limit: 1000/year.
    // Expected annual contribution: 1000.
    // Duration: 10 years.
    // Total added: 10 * 1000 = 10,000.
    // Final Balance: 10,000 + 10,000 = 20,000.

    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 20_000.0,
        "Balance should be capped by yearly limits"
    );
}

#[test]
fn test_simulation_start_date() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let params = SimulationParameters {
        start_date: Some(start_date),
        duration_years: 1,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 10_000.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
            account_type: AccountType::Taxable,
        }],
        cash_flows: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Check that the first snapshot date matches the start date
    assert_eq!(result.dates[0], start_date);
}

#[test]
fn test_inflation_adjustment() {
    let params = SimulationParameters {
        start_date: None,
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::Fixed(0.10), // 10% inflation
        return_profiles: vec![ReturnProfile::None],
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
            account_type: AccountType::Taxable,
        }],
        cash_flows: vec![CashFlow {
            cash_flow_id: CashFlowId(1),
            amount: 100.0,
            repeats: RepeatInterval::Yearly,
            cash_flow_limits: None,
            adjust_for_inflation: true,
            direction: CashFlowDirection::Income {
                target_account_id: AccountId(1),
                target_asset_id: AssetId(1),
            },
            state: CashFlowState::Active,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Year 0: 100.0
    // Year 1: 100.0 * 1.10 = 110.0
    // Total: 210.0

    let final_balance = result.final_account_balance(AccountId(1));
    // Floating point comparison
    assert!(
        (final_balance - 210.0).abs() < 1e-6,
        "Expected 210.0, got {}",
        final_balance
    );
}

#[test]
fn test_lifetime_limit() {
    let params = SimulationParameters {
        start_date: None,
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
            account_type: AccountType::Taxable,
        }],
        cash_flows: vec![CashFlow {
            cash_flow_id: CashFlowId(1),
            amount: 1000.0,
            repeats: RepeatInterval::Yearly,
            cash_flow_limits: Some(CashFlowLimits {
                limit: 2500.0,
                limit_period: LimitPeriod::Lifetime,
            }),
            adjust_for_inflation: false,
            direction: CashFlowDirection::Income {
                target_account_id: AccountId(1),
                target_asset_id: AssetId(1),
            },
            state: CashFlowState::Active,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(final_balance, 2500.0);
}

#[test]
fn test_interest_accrual() {
    let params = SimulationParameters {
        start_date: None,
        duration_years: 1,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::Fixed(0.10)],
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
            account_type: AccountType::Taxable,
        }],
        cash_flows: vec![CashFlow {
            cash_flow_id: CashFlowId(1),
            amount: 1000.0,
            repeats: RepeatInterval::Never,
            cash_flow_limits: None,
            adjust_for_inflation: false,
            direction: CashFlowDirection::Income {
                target_account_id: AccountId(1),
                target_asset_id: AssetId(1),
            },
            state: CashFlowState::Active,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);
    let final_balance = result.final_account_balance(AccountId(1));

    // 1000 invested immediately. 10% return. 1 year.
    // Should be 1100.
    assert!(
        (final_balance - 1100.0).abs() < 1.0,
        "Expected 1100.0, got {}",
        final_balance
    );
}
