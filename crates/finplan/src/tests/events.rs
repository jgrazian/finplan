//! Event system tests
//!
//! Tests for event triggers, effects, and chaining.

use crate::accounts::{Account, AccountType, Asset, AssetClass};
use crate::cash_flows::{CashFlow, CashFlowDirection, CashFlowLimits, CashFlowState, LimitPeriod, RepeatInterval};
use crate::config::SimulationParameters;
use crate::events::{Event, EventEffect, EventTrigger, TriggerOffset};
use crate::ids::{AccountId, AssetId, CashFlowId, EventId, SpendingTargetId};
use crate::profiles::{InflationProfile, ReturnProfile};
use crate::simulation::simulate;
use crate::spending::{SpendingTarget, SpendingTargetState, WithdrawalStrategy};
use jiff::ToSpan;

#[test]
fn test_event_trigger_balance() {
    // Event-based: Event triggers when balance > 5000, which activates a bonus cash flow
    let params = SimulationParameters {
        start_date: None,
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::AccountBalance {
                account_id: AccountId(1),
                threshold: 5000.0,
                above: true,
            },
            effects: vec![EventEffect::ActivateCashFlow(CashFlowId(2))],
            once: true,
        }],
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
        cash_flows: vec![
            // Base income: 2000/year - starts active
            CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 2000.0,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Active,
            },
            // Bonus starts when RichEnough event triggers - starts pending
            CashFlow {
                cash_flow_id: CashFlowId(2),
                amount: 10000.0,
                repeats: RepeatInterval::Never, // One time bonus
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Pending,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);
    let final_balance = result.final_account_balance(AccountId(1));

    // Year 0: +2000 -> Bal 2000
    // Year 1: +2000 -> Bal 4000
    // Year 2: +2000 -> Bal 6000. Trigger "RichEnough" (Threshold 5000).
    // Bonus +10000 -> Bal 16000.
    // Year 3: +2000 -> Bal 18000.
    // Year 4: +2000 -> Bal 20000.

    assert_eq!(final_balance, 20000.0);
}

#[test]
fn test_event_date_trigger() {
    
    
    let start_date = jiff::civil::date(2025, 1, 1);
    let params = SimulationParameters {
        start_date: Some(start_date),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Date(start_date.saturating_add(2.years())),
            effects: vec![EventEffect::ActivateCashFlow(CashFlowId(1))],
            once: true,
        }],
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
            repeats: RepeatInterval::Monthly,
            cash_flow_limits: Some(CashFlowLimits {
                limit: 5000.0,
                limit_period: LimitPeriod::Yearly,
            }),
            adjust_for_inflation: false,
            direction: CashFlowDirection::Income {
                target_account_id: AccountId(1),
                target_asset_id: AssetId(1),
            },
            state: CashFlowState::Pending, // Starts pending, activated by event
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // StartSaving triggers at Year 2 (2027-01-01).
    // Year 0 (2025): 0
    // Year 1 (2026): 0
    // Year 2 (2027): Start. Monthly 1000. Limit 5000/year.
    // Year 3 (2028): 5000.
    // Year 4 (2029): 5000.
    // Total: 15000.

    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(final_balance, 15000.0);
}

#[test]
fn test_cross_account_events() {
    
    
    // Test: Debt payoff triggers savings to start
    let params = SimulationParameters {
        start_date: None,
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::AccountBalance {
                account_id: AccountId(1), // Debt account
                threshold: 0.0,
                above: true, // When balance >= 0 (debt paid off)
            },
            effects: vec![
                EventEffect::TerminateCashFlow(CashFlowId(1)), // Stop debt payments
                EventEffect::ActivateCashFlow(CashFlowId(2)),  // Start savings
            ],
            once: true,
        }],
        accounts: vec![
            Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: -2000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Liability,
                }],
                account_type: AccountType::Illiquid,
            },
            Account {
                account_id: AccountId(2),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            },
        ],
        cash_flows: vec![
            // Debt payment - starts active
            CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1000.0,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Active,
            },
            // Savings - starts pending, activated when debt is paid
            CashFlow {
                cash_flow_id: CashFlowId(2),
                amount: 1000.0,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(2),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Pending,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Debt Account:
    // Year 0: -2000 + 1000 = -1000
    // Year 1: -1000 + 1000 = 0. Trigger "DebtPaid".
    // Payment stops.
    // Final Debt Balance: 0.

    // Savings Account:
    // Year 1: +1000 -> Bal 1000 (event triggered, cashflow activated)
    // Year 2: +1000 -> Bal 2000.
    // Year 3: +1000 -> Bal 3000.
    // Year 4: +1000 -> Bal 4000.

    let final_debt = result.final_account_balance(AccountId(1));
    let final_savings = result.final_account_balance(AccountId(2));

    assert_eq!(final_debt, 0.0, "Debt should be paid off");
    assert_eq!(
        final_savings, 4000.0,
        "Savings should accumulate after debt is paid"
    );
}

#[test]
fn test_age_based_event() {
    
    use crate::tax_config::TaxConfig;
    
    let birth_date = jiff::civil::date(1960, 6, 15);
    let start_date = jiff::civil::date(2025, 1, 1); // Person is 64

    let params = SimulationParameters {
        start_date: Some(start_date),
        duration_years: 5,
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Age {
                years: 65,
                months: None,
            },
            effects: vec![
                EventEffect::TerminateCashFlow(CashFlowId(1)), // Stop salary
                EventEffect::ActivateSpendingTarget(SpendingTargetId(1)), // Start retirement withdrawals
            ],
            once: true,
        }],
        accounts: vec![Account {
            account_id: AccountId(1),
            account_type: AccountType::TaxDeferred,
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 500_000.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
        }],
        cash_flows: vec![CashFlow {
            cash_flow_id: CashFlowId(1),
            amount: 50_000.0,
            repeats: RepeatInterval::Yearly,
            cash_flow_limits: None,
            adjust_for_inflation: false,
            direction: CashFlowDirection::Income {
                target_account_id: AccountId(1),
                target_asset_id: AssetId(1),
            },
            state: CashFlowState::Active,
        }],
        spending_targets: vec![SpendingTarget {
            spending_target_id: SpendingTargetId(1),
            amount: 40_000.0,
            net_amount_mode: false,
            repeats: RepeatInterval::Yearly,
            adjust_for_inflation: false,
            withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
            exclude_accounts: vec![],
            state: SpendingTargetState::Pending, // Starts pending
        }],
        tax_config: TaxConfig::default(),
    };

    let result = simulate(&params, 42);

    // Person turns 65 in June 2025
    // Year 0 (2025): Salary +50k, then retirement starts -> -40k. Net: +10k
    // Year 1 (2026): -40k (salary stopped)
    // Year 2 (2027): -40k
    // Year 3 (2028): -40k
    // Year 4 (2029): -40k

    // Starting: 500k + 50k (year 0 salary) = 550k
    // Withdrawals: 5 * 40k = 200k
    // Final: 550k - 200k = 350k

    let final_balance = result.final_account_balance(AccountId(1));

    // Verify retirement event was triggered
    assert!(
        result.event_was_triggered(EventId(1)),
        "Retirement event should have triggered"
    );

    assert!(
        (final_balance - 350_000.0).abs() < 1000.0,
        "Expected ~350,000, got {}",
        final_balance
    );
}

#[test]
fn test_event_chaining() {
    
    
    // Test that TriggerEvent effect works for chaining events
    let params = SimulationParameters {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 3,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Date(jiff::civil::date(2026, 1, 1)),
                effects: vec![
                    EventEffect::ActivateCashFlow(CashFlowId(1)),
                    EventEffect::TriggerEvent(EventId(2)), // Chain to secondary
                ],
                once: true,
            },
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Manual, // Only triggered via TriggerEvent
                effects: vec![EventEffect::ActivateCashFlow(CashFlowId(2))],
                once: true,
            },
        ],
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
        cash_flows: vec![
            CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1000.0,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Pending,
            },
            CashFlow {
                cash_flow_id: CashFlowId(2),
                amount: 500.0,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Pending,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Both events should have triggered
    assert!(
        result.event_was_triggered(EventId(1)),
        "Primary event should trigger"
    );
    assert!(
        result.event_was_triggered(EventId(2)),
        "Secondary event should be chained"
    );

    // Year 0 (2025): Nothing (events not triggered yet)
    // Year 1 (2026): Primary triggers -> Flow1 +1000, Flow2 +500 = 1500
    // Year 2 (2027): Flow1 +1000, Flow2 +500 = 1500

    let final_balance = result.final_account_balance(AccountId(1));
    assert_eq!(final_balance, 3000.0, "Should have 3000 from chained flows");
}

#[test]
fn test_repeating_event_transfer() {
    
    
    // Test repeating event that transfers $100/month between accounts
    let params = SimulationParameters {
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
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
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
                }],
                account_type: AccountType::TaxFree,
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Monthly,
                start_condition: None, // Start immediately
            },
            effects: vec![EventEffect::TransferAsset {
                from_account: AccountId(1),
                to_account: AccountId(2),
                from_asset_id: AssetId(1),
                to_asset_id: AssetId(2),
                amount: Some(100.0),
            }],
            once: false,
        }],
        cash_flows: vec![],
        spending_targets: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Should have triggered the repeating event
    assert!(
        result.event_was_triggered(EventId(1)),
        "Repeating event should trigger"
    );

    // Monthly transfers for 1 year (13 occurrences: Jan 1 start + 12 months)
    let account1_balance = result.final_account_balance(AccountId(1));
    let account2_balance = result.final_account_balance(AccountId(2));

    // The exact count depends on simulation timing, but should be ~12-13 transfers
    assert!(
        (1200.0..=1400.0).contains(&account2_balance),
        "Account 2 should have 12-14 transfers worth, got {}",
        account2_balance
    );
    assert_eq!(
        account1_balance + account2_balance,
        10_000.0,
        "Total should still be 10000"
    );

    // Check transfer history has reasonable count
    assert!(
        result.transfer_history.len() >= 12 && result.transfer_history.len() <= 14,
        "Should have 12-14 transfer records, got {}",
        result.transfer_history.len()
    );
}

#[test]
fn test_repeating_event_with_start_condition() {
    
    
    // Test repeating event that only starts after age 65
    let params = SimulationParameters {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 3,
        birth_date: Some(jiff::civil::date(1960, 6, 15)), // Age 64.5 at start
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 100_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::TaxDeferred,
            },
            Account {
                account_id: AccountId(2),
                assets: vec![Asset {
                    asset_id: AssetId(2),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::TaxFree,
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: Some(Box::new(EventTrigger::Age {
                    years: 65,
                    months: None,
                })),
            },
            effects: vec![EventEffect::TransferAsset {
                from_account: AccountId(1),
                to_account: AccountId(2),
                from_asset_id: AssetId(1),
                to_asset_id: AssetId(2),
                amount: Some(10_000.0), // Roth conversion
            }],
            once: false,
        }],
        cash_flows: vec![],
        spending_targets: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Event should trigger (start_condition met mid-2025)
    assert!(
        result.event_was_triggered(EventId(1)),
        "Repeating event should trigger after age 65"
    );

    // Age 65 is June 2025, then yearly transfers through end of 2027
    // The exact number depends on when condition is checked
    let account2_balance = result.final_account_balance(AccountId(2));
    let account1_balance = result.final_account_balance(AccountId(1));

    // Verify transfers happened
    assert!(
        account2_balance >= 20_000.0,
        "Account 2 should have at least 2 transfers worth (got {})",
        account2_balance
    );

    // Verify conservation of value
    assert_eq!(
        account1_balance + account2_balance,
        100_000.0,
        "Total should still be 100000"
    );
}
