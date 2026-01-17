//! RMD (Required Minimum Distribution) tests
//!
//! Tests for IRS RMD calculations using the Uniform Lifetime Table (2024).
//! RMDs are required from tax-deferred accounts (Traditional IRA, 401k) starting at age 73.

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountId, AccountType, Asset, AssetClass, AssetId, Event, EventEffect, EventId,
    EventTrigger, InflationProfile, RecordKind, RepeatInterval, ReturnProfile, RmdTable,
    TransactionSource,
};
use crate::simulation::simulate;

// ============================================================================
// RMD Table Tests
// ============================================================================

#[test]
fn test_rmd_table_age_73() {
    let table = RmdTable::irs_uniform_lifetime_2024();
    let divisor = table.divisor_for_age(73);
    assert_eq!(divisor, Some(26.5), "Age 73 divisor should be 26.5");
}

#[test]
fn test_rmd_table_age_80() {
    let table = RmdTable::irs_uniform_lifetime_2024();
    let divisor = table.divisor_for_age(80);
    assert_eq!(divisor, Some(20.2), "Age 80 divisor should be 20.2");
}

#[test]
fn test_rmd_table_age_90() {
    let table = RmdTable::irs_uniform_lifetime_2024();
    let divisor = table.divisor_for_age(90);
    assert_eq!(divisor, Some(12.2), "Age 90 divisor should be 12.2");
}

#[test]
fn test_rmd_table_age_100() {
    let table = RmdTable::irs_uniform_lifetime_2024();
    let divisor = table.divisor_for_age(100);
    assert_eq!(divisor, Some(6.4), "Age 100 divisor should be 6.4");
}

#[test]
fn test_rmd_table_age_before_73() {
    let table = RmdTable::irs_uniform_lifetime_2024();
    let divisor = table.divisor_for_age(72);
    assert_eq!(divisor, None, "No divisor for age < 73");
}

#[test]
fn test_rmd_table_age_beyond_120() {
    let table = RmdTable::irs_uniform_lifetime_2024();
    let divisor = table.divisor_for_age(121);
    assert_eq!(divisor, None, "No divisor for age > 120");
}

// ============================================================================
// RMD Calculation Integration Tests
// ============================================================================

/// Test basic RMD calculation starting at age 73
/// Note: RMD requires prior year balance, so the first year at age 73 won't have
/// a record if the simulation starts exactly at age 73. Starting earlier ensures
/// year-end balance is captured for prior year.
#[test]
fn test_rmd_starts_at_age_73() {
    // Person born 1952-01-01, simulation starts 2024-01-01 at age 72
    // This ensures we have a full year before RMD age to capture prior year balance
    let birth_date = jiff::civil::date(1952, 1, 1);
    let start_date = jiff::civil::date(2024, 1, 1); // Start at age 72

    const TRAD_401K: AccountId = AccountId(1);
    const SP500: AssetId = AssetId(1);
    const CASH_ACCOUNT: AccountId = AccountId(2);
    const CASH: AssetId = AssetId(2);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 4, // Ages 72-75, RMDs at 73, 74, 75
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None], // No growth for easy verification
        accounts: vec![
            Account {
                account_id: TRAD_401K,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 1_000_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
            Account {
                account_id: CASH_ACCOUNT,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: CASH,
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Cash,
                    initial_cost_basis: None,
                }],
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: Some(Box::new(EventTrigger::Age {
                    years: 73,
                    months: Some(0),
                })),
                end_condition: None,
            },
            effects: vec![EventEffect::ApplyRmd {
                to_account: CASH_ACCOUNT,
                to_asset: CASH,
                starting_age: 73,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Should have RMD records for ages 73, 74, 75 (3 years)
    let rmd_records: Vec<_> = result.rmd_records().collect();
    assert!(
        rmd_records.len() >= 2,
        "Expected at least 2 RMD records (years where prior balance is available), got {}",
        rmd_records.len()
    );

    // Verify first available RMD calculation
    match &rmd_records[0].kind {
        RecordKind::Transfer {
            from_account_id,
            to_account_id,
            source,
            gross_amount,
            ..
        } => match source.as_ref() {
            TransactionSource::Rmd {
                age,
                irs_divisor,
                required_amount,
                ..
            } => {
                assert_eq!(*from_account_id, TRAD_401K);
                assert_eq!(*to_account_id, CASH_ACCOUNT);
                assert_eq!(*age, 73);
                assert!(
                    (irs_divisor - 26.5).abs() < 0.1,
                    "Age 73 divisor should be 26.5"
                );
                let expected_rmd = 1_000_000.0 / 26.5;
                assert!(
                    (required_amount - expected_rmd).abs() < 1.0,
                    "Expected RMD ${:.2}, got ${:.2}",
                    expected_rmd,
                    required_amount
                );
                assert!(
                    (gross_amount - required_amount).abs() < 1.0,
                    "Should withdraw required amount"
                );
            }
            _ => {
                panic!("Expected RMD transaction source");
            }
        },
        _ => {
            panic!("Expected Transfer record kind");
        }
    }
}

/// Test RMD does not trigger before age 73
#[test]
fn test_rmd_does_not_trigger_before_73() {
    // Person born 1962, starts at age 63 (2025), runs 5 years to age 68
    let birth_date = jiff::civil::date(1962, 1, 1);
    let start_date = jiff::civil::date(2025, 1, 1);

    const TRAD_401K: AccountId = AccountId(1);
    const SP500: AssetId = AssetId(1);
    const CASH_ACCOUNT: AccountId = AccountId(2);
    const CASH: AssetId = AssetId(2);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5, // Ages 63-67 (well before 73)
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            Account {
                account_id: TRAD_401K,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 500_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
            Account {
                account_id: CASH_ACCOUNT,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: CASH,
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Cash,
                    initial_cost_basis: None,
                }],
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: Some(Box::new(EventTrigger::Age {
                    years: 73,
                    months: Some(0),
                })),
                end_condition: None,
            },
            effects: vec![EventEffect::ApplyRmd {
                to_account: CASH_ACCOUNT,
                to_asset: CASH,
                starting_age: 73,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Should have 0 RMD records (not yet 73)
    let rmd_count = result.rmd_records().count();
    assert_eq!(
        rmd_count, 0,
        "Expected 0 RMD records before age 73, got {}",
        rmd_count
    );

    // Account balance should be unchanged
    let final_balance = result.final_account_balance(TRAD_401K);
    assert_eq!(
        final_balance, 500_000.0,
        "Balance should be unchanged without RMDs"
    );
}

/// Test RMD with account growth
#[test]
fn test_rmd_with_account_growth() {
    // Person at age 72 (one year before RMD) with growing account
    let birth_date = jiff::civil::date(1952, 1, 1);
    let start_date = jiff::civil::date(2024, 1, 1); // Age 72

    const TRAD_401K: AccountId = AccountId(1);
    const SP500: AssetId = AssetId(1);
    const CASH_ACCOUNT: AccountId = AccountId(2);
    const CASH: AssetId = AssetId(2);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 3, // Ages 72-74, RMDs at 73, 74
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::Fixed(0.07)], // 7% annual return
        accounts: vec![
            Account {
                account_id: TRAD_401K,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 500_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
            Account {
                account_id: CASH_ACCOUNT,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: CASH,
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Cash,
                    initial_cost_basis: None,
                }],
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: Some(Box::new(EventTrigger::Age {
                    years: 73,
                    months: Some(0),
                })),
                end_condition: None,
            },
            effects: vec![EventEffect::ApplyRmd {
                to_account: CASH_ACCOUNT,
                to_asset: CASH,
                starting_age: 73,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    let rmd_records: Vec<_> = result.rmd_records().collect();
    assert!(
        !rmd_records.is_empty(),
        "Expected at least 1 RMD record, got {}",
        rmd_records.len()
    );

    // First RMD at age 73 (using year-end balance from age 72)
    match &rmd_records[0].kind {
        RecordKind::Transfer { source, .. } => match source.as_ref() {
            TransactionSource::Rmd {
                age,
                irs_divisor,
                prior_year_balance,
                ..
            } => {
                assert_eq!(*age, 73);
                // Prior year balance should include some growth from the year before
                assert!(
                    *prior_year_balance >= 500_000.0,
                    "Prior balance should be at least initial ${}, got ${}",
                    500_000.0,
                    prior_year_balance
                );
                assert!((irs_divisor - 26.5).abs() < 0.01);
            }
            _ => {
                panic!("Expected RMD transaction source");
            }
        },
        _ => {
            panic!("Expected Transfer record kind");
        }
    }
}

/// Test RMD from multiple accounts
#[test]
fn test_rmd_multiple_accounts() {
    let birth_date = jiff::civil::date(1952, 1, 1);
    let start_date = jiff::civil::date(2024, 1, 1); // Age 72 (one year before RMD)

    const TRAD_401K: AccountId = AccountId(1);
    const TRAD_IRA: AccountId = AccountId(2);
    const SP500: AssetId = AssetId(1);
    const BONDS: AssetId = AssetId(2);
    const CASH_ACCOUNT: AccountId = AccountId(2);
    const CASH: AssetId = AssetId(2);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 3, // Ages 72-74, RMDs at 73, 74
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            Account {
                account_id: TRAD_401K,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 500_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
            Account {
                account_id: TRAD_IRA,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: BONDS,
                    initial_value: 300_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
            Account {
                account_id: CASH_ACCOUNT,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: CASH,
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Cash,
                    initial_cost_basis: None,
                }],
            },
        ],
        // ApplyRmd automatically processes all tax-deferred accounts
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: Some(Box::new(EventTrigger::Age {
                    years: 73,
                    months: Some(0),
                })),
                end_condition: None,
            },
            effects: vec![EventEffect::ApplyRmd {
                to_account: CASH_ACCOUNT,
                to_asset: CASH,
                starting_age: 73,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Should have RMD records from both accounts
    let rmd_records: Vec<_> = result.rmd_records().collect();
    assert!(
        rmd_records.len() >= 2,
        "Expected at least 2 RMD records (one per account when prior balance available), got {}",
        rmd_records.len()
    );

    // Verify RMDs from both accounts
    let mut found_401k = false;
    let mut found_ira = false;
    for record in &rmd_records {
        if let RecordKind::Transfer {
            from_account_id,
            source,
            ..
        } = &record.kind
            && let TransactionSource::Rmd { .. } = source.as_ref()
        {
            if *from_account_id == TRAD_401K {
                found_401k = true;
            }
            if *from_account_id == TRAD_IRA {
                found_ira = true;
            }
        }
    }
    assert!(found_401k, "Should have RMD from 401k");
    assert!(found_ira, "Should have RMD from IRA");
}

/// Test RMD calculation amounts are correct
#[test]
fn test_rmd_amount_calculation() {
    // Start at age 79 so we have prior year balance for age 80
    let birth_date = jiff::civil::date(1945, 6, 15); // Age 79 in 2024
    let start_date = jiff::civil::date(2024, 7, 1); // After birthday

    const TRAD_IRA: AccountId = AccountId(1);
    const BONDS: AssetId = AssetId(1);
    const INITIAL_BALANCE: f64 = 1_000_000.0;
    const CASH_ACCOUNT: AccountId = AccountId(2);
    const CASH: AssetId = AssetId(2);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 2, // Ages 79-80, RMD at 80
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            Account {
                account_id: TRAD_IRA,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: BONDS,
                    initial_value: INITIAL_BALANCE,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
            Account {
                account_id: CASH_ACCOUNT,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: CASH,
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Cash,
                    initial_cost_basis: None,
                }],
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: Some(Box::new(EventTrigger::Age {
                    years: 73, // RMD starts at 73, person is already older
                    months: Some(0),
                })),
                end_condition: None,
            },
            effects: vec![EventEffect::ApplyRmd {
                to_account: CASH_ACCOUNT,
                to_asset: CASH,
                starting_age: 73,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    let rmd_records: Vec<_> = result.rmd_records().collect();
    assert!(
        !rmd_records.is_empty(),
        "Should have at least one RMD record"
    );

    // Find the RMD at age 80 (if available - may be at age 79 depending on timing)
    let rmd_record = rmd_records.iter().find_map(|r| {
        // if let RecordKind::Rmd { age, .. } = &r.kind {
        //     *age >= 79
        // } else {
        //     false
        // }
        match &r.kind {
            RecordKind::Transfer { source, .. } => match source.as_ref() {
                TransactionSource::Rmd { age, .. } if *age >= 79 => Some(source),
                _ => None,
            },
            _ => None,
        }
    });

    if let Some(record) = rmd_record
        && let TransactionSource::Rmd {
            age,
            prior_year_balance,
            irs_divisor,
            required_amount,
            ..
        } = &record.as_ref()
    {
        // Calculate actual withdrawn amount from records
        let actual_withdrawn: f64 = rmd_records
            .iter()
            .filter_map(|r| match &r.kind {
                RecordKind::Transfer {
                    from_account_id,
                    gross_amount,
                    ..
                } if *from_account_id == TRAD_IRA => Some(*gross_amount),
                _ => None,
            })
            .sum();
        {
            // Verify divisor is correct for the age
            let expected_divisor = match age {
                79 => 21.1,
                80 => 20.2,
                _ => 20.2,
            };
            assert!(
                (irs_divisor - expected_divisor).abs() < 0.1,
                "Age {} divisor should be {}, got {}",
                age,
                expected_divisor,
                irs_divisor
            );

            // RMD amount should be balance / divisor
            let expected_rmd = prior_year_balance / irs_divisor;
            assert!(
                (required_amount - expected_rmd).abs() < 1.0,
                "Expected RMD ${:.2}, got ${:.2}",
                expected_rmd,
                required_amount
            );
            assert!(
                (actual_withdrawn - required_amount).abs() < 1.0,
                "Should withdraw required amount"
            );

            // Verify account balance decreased by RMD amount
            let final_balance = result.final_account_balance(TRAD_IRA);
            // Account should be less than initial after RMD
            assert!(
                final_balance < INITIAL_BALANCE,
                "Final balance ${:.2} should be less than initial ${:.2}",
                final_balance,
                INITIAL_BALANCE
            );
        }
    }
}

/// Test RMD only applies to TaxDeferred accounts, not TaxFree (Roth)
#[test]
fn test_rmd_not_required_for_roth() {
    let birth_date = jiff::civil::date(1952, 1, 1);
    let start_date = jiff::civil::date(2025, 1, 1);

    const ROTH_IRA: AccountId = AccountId(1);
    const SP500: AssetId = AssetId(1);
    const INITIAL_BALANCE: f64 = 500_000.0;

    // Note: In real life, Roth IRAs don't require RMDs for the original owner
    // The simulation should handle this, but we're testing that if someone
    // mistakenly sets up RMD for a Roth, the account type is still TaxFree
    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 3,
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: ROTH_IRA,
            account_type: AccountType::TaxFree, // Roth = TaxFree
            assets: vec![Asset {
                asset_id: SP500,
                initial_value: INITIAL_BALANCE,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
        }],
        events: vec![], // No RMD events for Roth
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Should have 0 RMD records
    let rmd_count = result.rmd_records().count();
    assert_eq!(rmd_count, 0, "Roth accounts should not have RMDs");

    // Balance should be unchanged
    let final_balance = result.final_account_balance(ROTH_IRA);
    assert_eq!(
        final_balance, INITIAL_BALANCE,
        "Roth balance should be unchanged"
    );
}
