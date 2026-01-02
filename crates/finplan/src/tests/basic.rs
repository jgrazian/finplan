//! Basic simulation tests
//!
//! Tests for core simulation mechanics: cash flows, returns, inflation, limits.

use rayon::result;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountId, AccountType, Asset, AssetClass, AssetId, InflationProfile, LimitPeriod,
    RepeatInterval, ReturnProfile,
};
use crate::simulation::{monte_carlo_simulate, simulate};

// #[test]
// fn test_monte_carlo_simulation() {
//     let params = SimulationParameters {
//         start_date: None,
//         duration_years: 30,
//         birth_date: None,
//         inflation_profile: InflationProfile::Fixed(0.02),
//         return_profiles: vec![ReturnProfile::Normal {
//             mean: 0.07,
//             std_dev: 0.15,
//         }],
//         events: vec![],
//         accounts: vec![Account {
//             account_id: AccountId(1),
//             assets: vec![Asset {
//                 asset_id: AssetId(1),
//                 initial_value: 10_000.0,
//                 return_profile_index: 0,
//                 asset_class: AssetClass::Investable,
//             }],
//             account_type: AccountType::Taxable,
//         }],
//         cash_flows: vec![],
//         ..Default::default()
//     };

//     const NUM_ITERATIONS: usize = 10_000;
//     let result = monte_carlo_simulate(&params, NUM_ITERATIONS);
//     assert_eq!(result.iterations.len(), NUM_ITERATIONS);

//     // Check that results are different (due to random seed)
//     let first_final = result.iterations[0].final_account_balance(AccountId(1));
//     let second_final = result.iterations[1].final_account_balance(AccountId(1));

//     assert_ne!(first_final, second_final);
// }

#[test]
fn test_simulation_basic() {
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2020, 2, 5)),
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
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Test returns compound correctly over 10 years
    let expected_final = 10_000.0 + 10_000.0 * ((1.05_f64).powf(10.0) - 1.0);
    assert!(
        (result.final_account_balance(AccountId(1)) - expected_final).abs() < 100.0,
        "Expected final balance {}, got {}",
        expected_final,
        result.final_account_balance(AccountId(1))
    );
    assert!(
        result.final_account_balance(AccountId(1))
            == result.final_asset_balance(AccountId(1), AssetId(1)),
        "Expected final account balance to equal asset balance"
    );

    // Test start date and duration
    assert_eq!(
        result.dates.first().copied(),
        Some(jiff::civil::date(2020, 2, 5))
    );
    assert_eq!(
        result.dates.last().copied(),
        Some(jiff::civil::date(2030, 2, 5))
    );
}
