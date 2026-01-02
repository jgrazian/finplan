//! Event system tests for the new Transfer/Sweep-based event effects
//!
//! These tests cover:
//! - Transfer effect (income, expenses, asset-to-asset)
//! - Sweep effect (multi-source withdrawal with tax handling)
//! - Event triggers (date, age, balance, repeating)
//! - Event control effects (pause, resume, terminate, trigger)
//! - RMD effects

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountId, AccountType, Asset, AssetClass, AssetId, BalanceThreshold, Event,
    EventEffect, EventId, EventTrigger, FlowLimits, InflationProfile, LimitPeriod, LotMethod,
    RecordKind, RepeatInterval, ReturnProfile, TransferAmount, TransferEndpoint,
    WithdrawalAmountMode, WithdrawalSources,
};
use crate::simulation::simulate;

// ============================================================================
// Transfer Effect Tests
// ============================================================================

#[test]
fn test_transfer_income_fixed() {
    // Test simple income: External -> Asset with fixed amount
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 3,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: None,
                end_condition: None,
            },
            effects: vec![EventEffect::Transfer {
                from: TransferEndpoint::External,
                to: TransferEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                amount: TransferAmount::Fixed(10_000.0),
                adjust_for_inflation: false,
                limits: None,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // 3 years of $10,000 annual income = $30,000
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 30_000.0,
        "Expected $30,000 from 3 years of income, got {}",
        final_balance
    );

    // Verify income records
    let income_records: Vec<_> = result
        .records
        .iter()
        .filter(|r| matches!(r.kind, RecordKind::Income { .. }))
        .collect();
    assert_eq!(
        income_records.len(),
        3,
        "Should have 3 income records, got {}",
        income_records.len()
    );
}

#[test]
fn test_transfer_expense_fixed() {
    // Test simple expense: Asset -> External with fixed amount
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 3,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 50_000.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: None,
                end_condition: None,
            },
            effects: vec![EventEffect::Transfer {
                from: TransferEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                to: TransferEndpoint::External,
                amount: TransferAmount::Fixed(10_000.0),
                adjust_for_inflation: false,
                limits: None,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Started with $50,000, 3 years of $10,000 expense = $20,000 remaining
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 20_000.0,
        "Expected $20,000 after expenses, got {}",
        final_balance
    );

    // Verify expense records
    let expense_records: Vec<_> = result
        .records
        .iter()
        .filter(|r| matches!(r.kind, RecordKind::Expense { .. }))
        .collect();
    assert_eq!(
        expense_records.len(),
        3,
        "Should have 3 expense records, got {}",
        expense_records.len()
    );
}

#[test]
fn test_transfer_between_accounts() {
    // Test internal transfer: Asset -> Asset (no external)
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::Taxable,
            },
            Account {
                account_id: AccountId(2),
                assets: vec![Asset {
                    asset_id: AssetId(2),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::TaxFree,
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: None,
                end_condition: None,
            },
            effects: vec![EventEffect::Transfer {
                from: TransferEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                to: TransferEndpoint::Asset {
                    account_id: AccountId(2),
                    asset_id: AssetId(2),
                },
                amount: TransferAmount::Fixed(2_000.0),
                adjust_for_inflation: false,
                limits: None,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // 2 years of $2,000 transfers
    let account1 = result.final_account_balance(AccountId(1));
    let account2 = result.final_account_balance(AccountId(2));

    assert_eq!(
        account1, 6_000.0,
        "Account 1 should have $6,000, got {}",
        account1
    );
    assert_eq!(
        account2, 4_000.0,
        "Account 2 should have $4,000, got {}",
        account2
    );
    assert_eq!(account1 + account2, 10_000.0, "Total should be conserved");
}

#[test]
fn test_transfer_with_yearly_limits() {
    // Test transfer with yearly limits (like 401k contribution limit)
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::TaxDeferred,
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Monthly,
                start_condition: None,
                end_condition: None,
            },
            effects: vec![EventEffect::Transfer {
                from: TransferEndpoint::External,
                to: TransferEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                amount: TransferAmount::Fixed(5_000.0), // $5k/month attempt
                adjust_for_inflation: false,
                limits: Some(FlowLimits {
                    limit: 23_000.0, // 2024 401k limit
                    period: LimitPeriod::Yearly,
                }),
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Each year is capped at $23,000
    // 2 years = $46,000
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 46_000.0,
        "Expected $46,000 (2 years at $23k limit), got {}",
        final_balance
    );
}

#[test]
fn test_transfer_source_balance() {
    // Test TransferAmount::SourceBalance - transfer everything from source
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 1,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 15_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::Taxable,
            },
            Account {
                account_id: AccountId(2),
                assets: vec![Asset {
                    asset_id: AssetId(2),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::TaxFree,
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Date(jiff::civil::date(2025, 6, 1)),
            effects: vec![EventEffect::Transfer {
                from: TransferEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                to: TransferEndpoint::Asset {
                    account_id: AccountId(2),
                    asset_id: AssetId(2),
                },
                amount: TransferAmount::SourceBalance,
                adjust_for_inflation: false,
                limits: None,
            }],
            once: true,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    let account1 = result.final_account_balance(AccountId(1));
    let account2 = result.final_account_balance(AccountId(2));

    assert_eq!(
        account1, 0.0,
        "Account 1 should be emptied, got {}",
        account1
    );
    assert_eq!(
        account2, 15_000.0,
        "Account 2 should have full transfer, got {}",
        account2
    );
}

// ============================================================================
// Sweep Effect Tests (Multi-source withdrawal with tax handling)
// ============================================================================

#[test]
fn test_sweep_single_source_gross() {
    // Test Sweep from single source with gross mode
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            // Source: Tax-deferred account with $100k
            Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 100_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::TaxDeferred,
            },
            // Target: Cash account
            Account {
                account_id: AccountId(2),
                assets: vec![Asset {
                    asset_id: AssetId(2),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::Taxable,
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: None,
                end_condition: None,
            },
            effects: vec![EventEffect::Sweep {
                to_account: AccountId(2),
                to_asset: AssetId(2),
                target: TransferAmount::Fixed(20_000.0),
                sources: WithdrawalSources::Single {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                amount_mode: WithdrawalAmountMode::Gross,
                lot_method: LotMethod::Fifo,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // 2 years of $20k gross withdrawals = $40k withdrawn from source
    let source_balance = result.final_account_balance(AccountId(1));
    let target_balance = result.final_account_balance(AccountId(2));

    assert_eq!(
        source_balance, 60_000.0,
        "Source should have $60k remaining, got {}",
        source_balance
    );

    // Target receives net after taxes (tax-deferred = ordinary income)
    // Simplified: 22% federal + state_rate
    assert!(
        target_balance < 40_000.0,
        "Target should receive less than gross due to taxes, got {}",
        target_balance
    );
    assert!(
        target_balance > 25_000.0,
        "Target should have reasonable net amount, got {}",
        target_balance
    );
}

#[test]
fn test_sweep_tax_free_no_taxes() {
    // Test Sweep from Roth (tax-free) - no taxes on withdrawal
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 1,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            // Source: Tax-free (Roth) account
            Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 50_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::TaxFree,
            },
            // Target: Cash account
            Account {
                account_id: AccountId(2),
                assets: vec![Asset {
                    asset_id: AssetId(2),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::Taxable,
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Date(jiff::civil::date(2025, 6, 1)),
            effects: vec![EventEffect::Sweep {
                to_account: AccountId(2),
                to_asset: AssetId(2),
                target: TransferAmount::Fixed(10_000.0),
                sources: WithdrawalSources::Single {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                amount_mode: WithdrawalAmountMode::Gross,
                lot_method: LotMethod::Fifo,
            }],
            once: true,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    let source_balance = result.final_account_balance(AccountId(1));
    let target_balance = result.final_account_balance(AccountId(2));

    // Tax-free: gross = net
    assert_eq!(source_balance, 40_000.0, "Source should have $40k");
    assert_eq!(
        target_balance, 10_000.0,
        "Target should receive full $10k (no taxes on Roth)"
    );
}

// ============================================================================
// Event Trigger Tests
// ============================================================================

#[test]
fn test_trigger_date() {
    // Test date-based trigger
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 3,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Date(jiff::civil::date(2026, 7, 1)),
            effects: vec![EventEffect::Transfer {
                from: TransferEndpoint::External,
                to: TransferEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                amount: TransferAmount::Fixed(50_000.0),
                adjust_for_inflation: false,
                limits: None,
            }],
            once: true,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Event triggers in mid-2026, adds $50k
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 50_000.0,
        "Should have $50k from date-triggered event"
    );
}

#[test]
fn test_trigger_age() {
    // Test age-based trigger for retirement
    let birth_date = jiff::civil::date(1960, 6, 15);
    let start_date = jiff::civil::date(2025, 1, 1); // Age 64

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 3,
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Age {
                years: 65,
                months: None,
            },
            effects: vec![EventEffect::Transfer {
                from: TransferEndpoint::External,
                to: TransferEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                amount: TransferAmount::Fixed(100_000.0),
                adjust_for_inflation: false,
                limits: None,
            }],
            once: true,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Person turns 65 in June 2025
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 100_000.0,
        "Should have $100k from age-triggered event"
    );
}

#[test]
fn test_trigger_account_balance_threshold() {
    // Test balance-based trigger: when account reaches $20k, add bonus
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![
            // Regular income
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(5_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // Bonus when balance >= $20k
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::AccountBalance {
                    account_id: AccountId(1),
                    threshold: BalanceThreshold::GreaterThanOrEqual(20_000.0),
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(10_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Years 0-3: $5k each = $20k
    // Year 4 (or when threshold hit): Bonus $10k triggered
    // Year 4: +$5k more
    // Total: $5k * 5 + $10k bonus = $35k
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 35_000.0,
        "Expected $35k after bonus triggers, got {}",
        final_balance
    );
}

#[test]
fn test_trigger_repeating_with_end_condition() {
    // Test repeating event that stops when balance reaches threshold
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 10,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: None,
                end_condition: Some(Box::new(EventTrigger::AccountBalance {
                    account_id: AccountId(1),
                    threshold: BalanceThreshold::GreaterThanOrEqual(30_000.0),
                })),
            },
            effects: vec![EventEffect::Transfer {
                from: TransferEndpoint::External,
                to: TransferEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
                amount: TransferAmount::Fixed(10_000.0),
                adjust_for_inflation: false,
                limits: None,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Contributions stop when >= $30k
    // Year 0: $10k
    // Year 1: $20k
    // Year 2: $30k -> end condition met
    // Should NOT continue adding more
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 30_000.0,
        "Should stop at $30k due to end_condition, got {}",
        final_balance
    );
}

// ============================================================================
// Event Control Effects Tests
// ============================================================================

#[test]
fn test_pause_and_resume_event() {
    // Test pausing and resuming a repeating event
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![
            // Repeating income
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(10_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // Pause at year 2
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Date(jiff::civil::date(2027, 1, 1)),
                effects: vec![EventEffect::PauseEvent(EventId(1))],
                once: true,
            },
            // Resume at year 4
            Event {
                event_id: EventId(3),
                trigger: EventTrigger::Date(jiff::civil::date(2029, 1, 1)),
                effects: vec![EventEffect::ResumeEvent(EventId(1))],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Year 0 (2025): +$10k
    // Year 1 (2026): +$10k
    // Year 2 (2027): Paused (no income)
    // Year 3 (2028): Still paused
    // Year 4 (2029): Resumed +$10k
    // Total: $30k
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 30_000.0,
        "Expected $30k after pause/resume, got {}",
        final_balance
    );
}

#[test]
fn test_trigger_event_chaining() {
    // Test TriggerEvent effect for chaining events
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![
            // Primary event triggers secondary
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Date(jiff::civil::date(2025, 6, 1)),
                effects: vec![
                    EventEffect::Transfer {
                        from: TransferEndpoint::External,
                        to: TransferEndpoint::Asset {
                            account_id: AccountId(1),
                            asset_id: AssetId(1),
                        },
                        amount: TransferAmount::Fixed(10_000.0),
                        adjust_for_inflation: false,
                        limits: None,
                    },
                    EventEffect::TriggerEvent(EventId(2)),
                ],
                once: true,
            },
            // Secondary event (Manual trigger)
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Manual,
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(5_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Primary: $10k + chains to secondary: $5k = $15k
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 15_000.0,
        "Expected $15k from chained events, got {}",
        final_balance
    );
}

#[test]
fn test_terminate_event() {
    // Test permanently terminating an event
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![
            // Repeating income
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(10_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // Terminate after 2 years
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Date(jiff::civil::date(2027, 1, 1)),
                effects: vec![EventEffect::TerminateEvent(EventId(1))],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Years 0, 1: $20k total, then terminated
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 20_000.0,
        "Expected $20k before termination, got {}",
        final_balance
    );
}

// ============================================================================
// Compound Trigger Tests
// ============================================================================

#[test]
fn test_trigger_and_compound() {
    // Test And trigger - both conditions must be true
    let birth_date = jiff::civil::date(1960, 6, 15);
    let start_date = jiff::civil::date(2025, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5,
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![
            // Build up balance
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(5_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // Bonus when BOTH: age >= 66 AND balance >= $10k
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::And(vec![
                    EventTrigger::Age {
                        years: 66,
                        months: None,
                    },
                    EventTrigger::AccountBalance {
                        account_id: AccountId(1),
                        threshold: BalanceThreshold::GreaterThanOrEqual(10_000.0),
                    },
                ]),
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(20_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Age 66 in June 2026, Balance reaches $10k after Year 1 (2026)
    // Both conditions met in 2026 -> bonus $20k
    // 5 years * $5k + $20k bonus = $45k
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 45_000.0,
        "Expected $45k with compound trigger, got {}",
        final_balance
    );
}

#[test]
fn test_trigger_or_compound() {
    // Test Or trigger - either condition triggers
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 0.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![
            // Build up balance
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(5_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // Bonus when EITHER: date 2028 OR balance >= $100k
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Or(vec![
                    EventTrigger::Date(jiff::civil::date(2028, 1, 1)),
                    EventTrigger::AccountBalance {
                        account_id: AccountId(1),
                        threshold: BalanceThreshold::GreaterThanOrEqual(100_000.0),
                    },
                ]),
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(10_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Date 2028 comes before $100k balance
    // Years 0-2: $15k, then bonus $10k at 2028
    // Years 3-4: +$10k more
    // Total: $5k * 5 + $10k = $35k
    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        final_balance, 35_000.0,
        "Expected $35k with Or trigger, got {}",
        final_balance
    );
}

// ============================================================================
// Investment Returns Tests
// ============================================================================

#[test]
fn test_investment_returns_compound() {
    // Test that investment returns compound correctly
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 10,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::Fixed(0.07)], // 7% annual return
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 100_000.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
                initial_cost_basis: None,
            }],
            account_type: AccountType::Taxable,
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // $100k * 1.07^10 ≈ $196,715
    let expected = 100_000.0 * (1.07_f64).powi(10);
    let final_balance = result.final_account_balance(AccountId(1));

    assert!(
        (final_balance - expected).abs() < 1_000.0,
        "Expected ~${:.0}, got ${:.0}",
        expected,
        final_balance
    );
}

// ============================================================================
// Complex Scenario Tests
// ============================================================================

#[test]
fn test_retirement_scenario() {
    // Full retirement scenario:
    // - Working phase: salary income until age 65
    // - Retirement phase: withdraw from 401k

    let birth_date = jiff::civil::date(1960, 1, 1);
    let start_date = jiff::civil::date(2023, 1, 1); // Age 63

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5, // Through age 67
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None], // Simplified - no returns
        accounts: vec![
            // 401k
            Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 500_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::TaxDeferred,
            },
            // Cash account for spending
            Account {
                account_id: AccountId(2),
                assets: vec![Asset {
                    asset_id: AssetId(2),
                    initial_value: 20_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::Taxable,
            },
        ],
        events: vec![
            // Salary (ends at retirement)
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: Some(Box::new(EventTrigger::Age {
                        years: 65,
                        months: None,
                    })),
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(2),
                        asset_id: AssetId(2),
                    },
                    amount: TransferAmount::Fixed(100_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // Annual expenses (always)
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::Asset {
                        account_id: AccountId(2),
                        asset_id: AssetId(2),
                    },
                    to: TransferEndpoint::External,
                    amount: TransferAmount::Fixed(60_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // Retirement withdrawals (start at 65)
            Event {
                event_id: EventId(3),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: Some(Box::new(EventTrigger::Age {
                        years: 65,
                        months: None,
                    })),
                    end_condition: None,
                },
                effects: vec![EventEffect::Sweep {
                    to_account: AccountId(2),
                    to_asset: AssetId(2),
                    target: TransferAmount::Fixed(60_000.0),
                    sources: WithdrawalSources::Single {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount_mode: WithdrawalAmountMode::Gross,
                    lot_method: LotMethod::Fifo,
                }],
                once: false,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Working years (63, 64): +$100k income, -$60k expenses = +$40k/year
    // Retirement years (65, 66, 67): -$60k expenses, +$60k withdrawal from 401k

    let final_401k = result.final_account_balance(AccountId(1));
    let final_cash = result.final_account_balance(AccountId(2));

    // 401k should have 3 years of $60k withdrawals = $180k withdrawn
    // Starting $500k - $180k = $320k
    assert!(
        (final_401k - 320_000.0).abs() < 1_000.0,
        "401k should have ~$320k, got {}",
        final_401k
    );

    // Cash: Starting $20k + 2*$100k salary - 5*$60k expenses + 3*$60k withdrawals (net after tax)
    // The withdrawals are taxed, so cash won't equal simple arithmetic
    assert!(
        final_cash > 0.0,
        "Cash should be positive, got {}",
        final_cash
    );
}

#[test]
fn test_debt_payoff_then_save() {
    // Scenario: Pay off debt, then redirect money to savings
    let params = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            // Debt account (negative balance)
            Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: -10_000.0, // $10k debt
                    return_profile_index: 0,
                    asset_class: AssetClass::Liability,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::Illiquid,
            },
            // Savings account
            Account {
                account_id: AccountId(2),
                assets: vec![Asset {
                    asset_id: AssetId(2),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
                account_type: AccountType::Taxable,
            },
        ],
        events: vec![
            // Debt payment
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: Some(Box::new(EventTrigger::AccountBalance {
                        account_id: AccountId(1),
                        threshold: BalanceThreshold::GreaterThanOrEqual(0.0),
                    })),
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                    amount: TransferAmount::Fixed(5_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // Savings (starts when debt is paid)
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: Some(Box::new(EventTrigger::AccountBalance {
                        account_id: AccountId(1),
                        threshold: BalanceThreshold::GreaterThanOrEqual(0.0),
                    })),
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: AccountId(2),
                        asset_id: AssetId(2),
                    },
                    amount: TransferAmount::Fixed(5_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Year 0: Debt -10k + 5k = -5k
    // Year 1: Debt -5k + 5k = 0 -> debt paid, start savings +$5k
    // Year 2: Savings +$5k = $10k
    // Year 3: Savings +$5k = $15k
    // Year 4: Savings +$5k = $20k

    let final_debt = result.final_account_balance(AccountId(1));
    let final_savings = result.final_account_balance(AccountId(2));

    assert_eq!(
        final_debt, 0.0,
        "Debt should be paid off, got {}",
        final_debt
    );
    assert_eq!(
        final_savings, 20_000.0,
        "Savings should be $20k, got {}",
        final_savings
    );
}
