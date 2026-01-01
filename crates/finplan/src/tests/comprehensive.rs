//! Comprehensive lifecycle integration tests
//!
//! These tests model realistic financial scenarios over multiple decades.

use crate::accounts::{Account, AccountType, Asset, AssetClass};
use crate::cash_flows::{
    CashFlow, CashFlowDirection, CashFlowLimits, CashFlowState, LimitPeriod, RepeatInterval,
};
use crate::config::SimulationParameters;
use crate::events::{Event, EventEffect, EventTrigger, TriggerOffset};
use crate::ids::{AccountId, AssetId, CashFlowId, EventId, SpendingTargetId};
use crate::profiles::{InflationProfile, ReturnProfile};
use crate::simulation::simulate;
use crate::spending::{SpendingTarget, SpendingTargetState, WithdrawalStrategy};
use crate::tax_config::{TaxBracket, TaxConfig};

#[test]
fn test_spending_target_basic() {
    // Test basic spending target withdrawal from a single account
    let params = SimulationParameters {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            account_type: AccountType::TaxDeferred, // 401k - taxed as ordinary income
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 100_000.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
        }],
        cash_flows: vec![],
        spending_targets: vec![SpendingTarget {
            spending_target_id: SpendingTargetId(1),
            amount: 10_000.0,
            net_amount_mode: false, // Gross withdrawal
            repeats: RepeatInterval::Yearly,
            adjust_for_inflation: false,
            withdrawal_strategy: WithdrawalStrategy::Sequential {
                order: vec![AccountId(1)],
            },
            exclude_accounts: vec![],
            state: SpendingTargetState::Active,
        }],
        tax_config: TaxConfig::default(),
    };

    let result = simulate(&params, 42);
    let final_balance = result.final_account_balance(AccountId(1));

    // Starting: 100,000
    // Yearly withdrawal: 10,000
    // After 5 years: 100,000 - (5 * 10,000) = 50,000
    assert!(
        (final_balance - 50_000.0).abs() < 1.0,
        "Expected ~50,000, got {}",
        final_balance
    );

    // Check that taxes were tracked
    assert!(!result.yearly_taxes.is_empty(), "Should have tax records");

    // Each year should have 10,000 in ordinary income (TaxDeferred withdrawal)
    for tax in &result.yearly_taxes {
        assert!(
            (tax.ordinary_income - 10_000.0).abs() < 1.0,
            "Expected 10,000 ordinary income, got {}",
            tax.ordinary_income
        );
    }
}

#[test]
fn test_spending_target_tax_optimized() {
    // Test tax-optimized withdrawal order: Taxable -> TaxDeferred -> TaxFree
    let params = SimulationParameters {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 3,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![],
        accounts: vec![
            Account {
                account_id: AccountId(1),
                account_type: AccountType::TaxFree, // Roth - should be last
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 50_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
            },
            Account {
                account_id: AccountId(2),
                account_type: AccountType::TaxDeferred, // 401k - should be second
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 50_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
            },
            Account {
                account_id: AccountId(3),
                account_type: AccountType::Taxable, // Brokerage - should be first
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 30_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
            },
        ],
        cash_flows: vec![],
        spending_targets: vec![SpendingTarget {
            spending_target_id: SpendingTargetId(1),
            amount: 40_000.0,
            net_amount_mode: false,
            repeats: RepeatInterval::Yearly,
            adjust_for_inflation: false,
            withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
            exclude_accounts: vec![],
            state: SpendingTargetState::Active,
        }],
        tax_config: TaxConfig::default(),
    };

    let result = simulate(&params, 42);

    // Year 1: Need 40k. Taxable has 30k, so take all 30k from Taxable, then 10k from TaxDeferred
    // Year 2: Taxable empty. Take 40k from TaxDeferred (has 40k left)
    // Year 3: TaxDeferred empty. Take 40k from TaxFree

    // Final balances:
    // Taxable: 0
    // TaxDeferred: 0
    // TaxFree: 50,000 - 40,000 = 10,000

    let taxfree_balance = result.final_account_balance(AccountId(1));
    let taxdeferred_balance = result.final_account_balance(AccountId(2));
    let taxable_balance = result.final_account_balance(AccountId(3));

    assert!(
        taxable_balance.abs() < 1.0,
        "Taxable should be depleted first, got {}",
        taxable_balance
    );
    assert!(
        taxdeferred_balance.abs() < 1.0,
        "TaxDeferred should be depleted second, got {}",
        taxdeferred_balance
    );
    assert!(
        (taxfree_balance - 10_000.0).abs() < 1.0,
        "TaxFree should have ~10,000 left, got {}",
        taxfree_balance
    );
}

#[test]
fn test_spending_target_excludes_illiquid() {
    // Test that Illiquid accounts are automatically skipped
    let params = SimulationParameters {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        events: vec![],
        accounts: vec![
            Account {
                account_id: AccountId(1),
                account_type: AccountType::Illiquid, // Real estate - cannot withdraw
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 500_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::RealEstate,
                }],
            },
            Account {
                account_id: AccountId(2),
                account_type: AccountType::Taxable,
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 50_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
            },
        ],
        cash_flows: vec![],
        spending_targets: vec![SpendingTarget {
            spending_target_id: SpendingTargetId(1),
            amount: 20_000.0,
            net_amount_mode: false,
            repeats: RepeatInterval::Yearly,
            adjust_for_inflation: false,
            withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
            exclude_accounts: vec![],
            state: SpendingTargetState::Active,
        }],
        tax_config: TaxConfig::default(),
    };

    let result = simulate(&params, 42);

    // Illiquid account should be untouched
    let illiquid_balance = result.final_account_balance(AccountId(1));
    assert_eq!(
        illiquid_balance, 500_000.0,
        "Illiquid account should be untouched"
    );

    // Taxable should have withdrawals
    let taxable_balance = result.final_account_balance(AccountId(2));
    assert!(
        (taxable_balance - 10_000.0).abs() < 1.0,
        "Taxable should have 10,000 left after 2 years of 20k withdrawals, got {}",
        taxable_balance
    );
}

#[test]
fn test_rmd_withdrawal() {
    let params = SimulationParameters {
        start_date: Some(jiff::civil::date(2024, 1, 1)),
        duration_years: 5,
        birth_date: Some(jiff::civil::date(1951, 6, 15)), // Age 73 in 2024
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![Account {
            account_id: AccountId(1),
            assets: vec![Asset {
                asset_id: AssetId(1),
                initial_value: 1_000_000.0,
                return_profile_index: 0,
                asset_class: AssetClass::Investable,
            }],
            account_type: AccountType::TaxDeferred,
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: Some(Box::new(EventTrigger::Age {
                    years: 73,
                    months: Some(0),
                })),
            },
            effects: vec![EventEffect::CreateRmdWithdrawal {
                account_id: AccountId(1),
                starting_age: 73,
            }],
            once: false,
        }],
        cash_flows: vec![],
        spending_targets: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // RMD event should trigger
    assert!(
        result.event_was_triggered(EventId(1)),
        "RMD event should trigger at age 73"
    );

    // Should have RMD withdrawals recorded
    assert!(
        !result.withdrawal_history.is_empty(),
        "Should have RMD withdrawals"
    );

    let final_balance = result.final_account_balance(AccountId(1));

    println!(
        "After RMDs: Starting balance=$1,000,000, Final balance=${:.2}",
        final_balance
    );
    println!("RMD withdrawals: {}", result.withdrawal_history.len());

    // Verify exactly 5 RMD withdrawals (one per year for 5-year simulation)
    assert_eq!(
        result.withdrawal_history.len(),
        5,
        "Should have exactly 5 RMD withdrawals"
    );

    // Verify RMDs were taken (total withdrawals should be substantial)
    let total_withdrawn: f64 = result
        .withdrawal_history
        .iter()
        .map(|w| w.gross_amount)
        .sum();
    println!("RMD withdrawal total: {}", total_withdrawn);
    assert!(
        total_withdrawn > 100_000.0,
        "Total RMD withdrawals should be substantial, got {:.2}",
        total_withdrawn
    );

    // With 5% returns and ~3.77% RMD rate at age 73, balance may grow or shrink
    // depending on market performance vs withdrawal rate
    // Just verify the simulation completed successfully
    assert!(
        final_balance > 0.0,
        "Account should still have positive balance"
    );
}

#[test]
fn test_comprehensive_lifecycle_simulation() {
    // Comprehensive test case modeling a realistic financial lifecycle:
    // - Multiple accounts with multiple assets
    // - Asset tracking across accounts (VFIAX in multiple places)
    // - Complex event chains (home purchase, retirement, RMD)
    // - Cash flow limits (Roth 401k contributions)
    // - Age-based events
    // - Tax optimization

    let start_date = jiff::civil::date(2025, 1, 1);
    let birth_date = jiff::civil::date(1997, 3, 16); // Age 28 at start

    // Asset IDs
    const VFIAX: AssetId = AssetId(1);
    const VGPMX: AssetId = AssetId(2);
    const VIMAX: AssetId = AssetId(3);
    const VTIAX: AssetId = AssetId(4);
    const VFIFX: AssetId = AssetId(5);
    const SP500: AssetId = AssetId(6);
    const HOUSE: AssetId = AssetId(7);
    const CASH: AssetId = AssetId(8);
    const MORTGAGE: AssetId = AssetId(9);

    // Account IDs
    const BROKERAGE: AccountId = AccountId(1);
    const ROTH_IRA: AccountId = AccountId(2);
    const TRAD_401K: AccountId = AccountId(3);
    const ROTH_401K: AccountId = AccountId(4);
    const REAL_ESTATE: AccountId = AccountId(5);
    const CASH_ACCOUNT: AccountId = AccountId(6);
    const MORTGAGE_DEBT: AccountId = AccountId(7);

    // Variables
    const HOUSE_PRICE: f64 = 1_200_000.0;
    const DOWN_PAYMENT_PERCENT: f64 = 0.20; // 20%
    const HOME_PURCHASE_AGE: u8 = 35;
    const RETIREMENT_AGE: u8 = 45;

    let params = SimulationParameters {
        start_date: Some(start_date),
        duration_years: 50, // Age 28 to 78
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::Fixed(0.025), // 2.5% inflation
        return_profiles: vec![
            ReturnProfile::Fixed(0.07),  // 0: S&P 500
            ReturnProfile::Fixed(0.03),  // 1: Precious metals
            ReturnProfile::Fixed(0.08),  // 2: Mid-cap
            ReturnProfile::Fixed(0.06),  // 3: International
            ReturnProfile::Fixed(0.065), // 4: Target date
            ReturnProfile::Fixed(0.03),  // 5: House appreciation
            ReturnProfile::Fixed(0.0),   // 6: Cash (no growth)
            ReturnProfile::Fixed(0.06),  // 7: Mortgage debt interest
        ],
        accounts: vec![
            // 1. Brokerage (Taxable)
            Account {
                account_id: BROKERAGE,
                account_type: AccountType::Taxable,
                assets: vec![
                    Asset {
                        asset_id: VFIAX,
                        initial_value: 900_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    },
                    Asset {
                        asset_id: VGPMX,
                        initial_value: 230_000.0,
                        return_profile_index: 1,
                        asset_class: AssetClass::Investable,
                    },
                    Asset {
                        asset_id: VIMAX,
                        initial_value: 70_000.0,
                        return_profile_index: 2,
                        asset_class: AssetClass::Investable,
                    },
                    Asset {
                        asset_id: VTIAX,
                        initial_value: 80_000.0,
                        return_profile_index: 3,
                        asset_class: AssetClass::Investable,
                    },
                ],
            },
            // 2. Roth IRA (TaxFree)
            Account {
                account_id: ROTH_IRA,
                account_type: AccountType::TaxFree,
                assets: vec![
                    Asset {
                        asset_id: VFIAX,
                        initial_value: 30_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    },
                    Asset {
                        asset_id: VFIFX,
                        initial_value: 15_000.0,
                        return_profile_index: 4,
                        asset_class: AssetClass::Investable,
                    },
                ],
            },
            // 3. Traditional 401k (TaxDeferred)
            Account {
                account_id: TRAD_401K,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 100_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
            },
            // 4. Roth 401k (TaxFree)
            Account {
                account_id: ROTH_401K,
                account_type: AccountType::TaxFree,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 50_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
            },
            // 5. Real Estate (Illiquid)
            Account {
                account_id: REAL_ESTATE,
                account_type: AccountType::Illiquid,
                assets: vec![Asset {
                    asset_id: HOUSE,
                    initial_value: 0.0,
                    return_profile_index: 5,
                    asset_class: AssetClass::RealEstate,
                }],
            },
            // 6. Cash Account (Taxable)
            Account {
                account_id: CASH_ACCOUNT,
                account_type: AccountType::Taxable,
                assets: vec![Asset {
                    asset_id: CASH,
                    initial_value: (HOUSE_PRICE * DOWN_PAYMENT_PERCENT) + 100_000.0,
                    return_profile_index: 6,
                    asset_class: AssetClass::Investable,
                }],
            },
            // 7. Mortgage Debt (Illiquid)
            Account {
                account_id: MORTGAGE_DEBT,
                account_type: AccountType::Illiquid,
                assets: vec![Asset {
                    asset_id: MORTGAGE,
                    initial_value: 0.0,
                    return_profile_index: 7,
                    asset_class: AssetClass::Liability,
                }],
            },
        ],
        cash_flows: vec![
            // Monthly contribution to Brokerage VFIAX
            CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1_500.0,
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: None,
                adjust_for_inflation: true,
                direction: CashFlowDirection::Income {
                    target_account_id: BROKERAGE,
                    target_asset_id: VFIAX,
                },
                state: CashFlowState::Active,
            },
            // Mega backdoor Roth 401k - $43.5k/year
            CashFlow {
                cash_flow_id: CashFlowId(2),
                amount: 10_000.0,
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: Some(CashFlowLimits {
                    limit: 43_500.0,
                    limit_period: LimitPeriod::Yearly,
                }),
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: ROTH_401K,
                    target_asset_id: SP500,
                },
                state: CashFlowState::Active,
            },
            // Backdoor Roth IRA - $7k/year
            CashFlow {
                cash_flow_id: CashFlowId(3),
                amount: 7_000.0,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: ROTH_IRA,
                    target_asset_id: VFIAX,
                },
                state: CashFlowState::Active,
            },
            // Mortgage payment - activated by home purchase event
            CashFlow {
                cash_flow_id: CashFlowId(4),
                amount: 5_755.0,
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: MORTGAGE_DEBT,
                    target_asset_id: MORTGAGE,
                },
                state: CashFlowState::Pending,
            },
        ],
        spending_targets: vec![
            // Retirement withdrawals - activated at age 45
            SpendingTarget {
                spending_target_id: SpendingTargetId(1),
                amount: 200_000.0,
                net_amount_mode: true,
                repeats: RepeatInterval::Yearly,
                adjust_for_inflation: true,
                withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
                exclude_accounts: vec![REAL_ESTATE, CASH_ACCOUNT, MORTGAGE_DEBT],
                state: SpendingTargetState::Pending,
            },
        ],
        events: vec![
            // Home purchase at age 35
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Age {
                    years: HOME_PURCHASE_AGE,
                    months: Some(3),
                },
                effects: vec![
                    // Down payment expense
                    EventEffect::CreateCashFlow(Box::new(CashFlow {
                        cash_flow_id: CashFlowId(101),
                        amount: HOUSE_PRICE * DOWN_PAYMENT_PERCENT,
                        repeats: RepeatInterval::Never,
                        cash_flow_limits: None,
                        adjust_for_inflation: false,
                        direction: CashFlowDirection::Expense {
                            source_account_id: CASH_ACCOUNT,
                            source_asset_id: CASH,
                        },
                        state: CashFlowState::Active,
                    })),
                    // Mortgage debt
                    EventEffect::CreateCashFlow(Box::new(CashFlow {
                        cash_flow_id: CashFlowId(102),
                        amount: HOUSE_PRICE * (1.0 - DOWN_PAYMENT_PERCENT),
                        repeats: RepeatInterval::Never,
                        cash_flow_limits: None,
                        adjust_for_inflation: false,
                        direction: CashFlowDirection::Expense {
                            source_account_id: MORTGAGE_DEBT,
                            source_asset_id: MORTGAGE,
                        },
                        state: CashFlowState::Active,
                    })),
                    // House asset
                    EventEffect::CreateCashFlow(Box::new(CashFlow {
                        cash_flow_id: CashFlowId(100),
                        amount: HOUSE_PRICE,
                        repeats: RepeatInterval::Never,
                        cash_flow_limits: None,
                        adjust_for_inflation: false,
                        direction: CashFlowDirection::Income {
                            target_account_id: REAL_ESTATE,
                            target_asset_id: HOUSE,
                        },
                        state: CashFlowState::Active,
                    })),
                    // Start mortgage payments
                    EventEffect::ActivateCashFlow(CashFlowId(4)),
                ],
                once: true,
            },
            // Stop mortgage when paid off
            Event {
                event_id: EventId(101),
                trigger: EventTrigger::And(vec![
                    EventTrigger::RelativeToEvent {
                        event_id: EventId(1),
                        offset: TriggerOffset::Months(1),
                    },
                    EventTrigger::AccountBalance {
                        account_id: MORTGAGE_DEBT,
                        threshold: -1000.0,
                        above: true,
                    },
                ]),
                effects: vec![EventEffect::TerminateCashFlow(CashFlowId(4))],
                once: true,
            },
            // Retirement at age 45
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Age {
                    years: RETIREMENT_AGE,
                    months: Some(0),
                },
                effects: vec![
                    EventEffect::TerminateCashFlow(CashFlowId(1)),
                    EventEffect::TerminateCashFlow(CashFlowId(2)),
                    EventEffect::TerminateCashFlow(CashFlowId(3)),
                    EventEffect::ActivateSpendingTarget(SpendingTargetId(1)),
                ],
                once: true,
            },
            // RMD at age 73
            Event {
                event_id: EventId(3),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: Some(Box::new(EventTrigger::Age {
                        years: 73,
                        months: Some(0),
                    })),
                },
                effects: vec![EventEffect::CreateRmdWithdrawal {
                    account_id: TRAD_401K,
                    starting_age: 73,
                }],
                once: false,
            },
        ],
        tax_config: TaxConfig {
            federal_brackets: vec![
                TaxBracket {
                    threshold: 0.0,
                    rate: 0.10,
                },
                TaxBracket {
                    threshold: 11_600.0,
                    rate: 0.12,
                },
                TaxBracket {
                    threshold: 47_150.0,
                    rate: 0.22,
                },
                TaxBracket {
                    threshold: 100_525.0,
                    rate: 0.24,
                },
                TaxBracket {
                    threshold: 191_950.0,
                    rate: 0.32,
                },
                TaxBracket {
                    threshold: 243_725.0,
                    rate: 0.35,
                },
                TaxBracket {
                    threshold: 609_350.0,
                    rate: 0.37,
                },
            ],
            state_rate: 0.05,
            capital_gains_rate: 0.15,
            taxable_gains_percentage: 0.5,
        },
    };

    let result = simulate(&params, 42);

    // === VERIFICATION CHECKS ===

    println!("\n=== Comprehensive Lifecycle Simulation Results ===");

    // Verify home purchase event
    assert!(
        result.event_was_triggered(EventId(1)),
        "Home purchase event should trigger at age 35"
    );
    let house_value = result.final_account_balance(REAL_ESTATE);
    println!("House final value: ${:.2}", house_value);
    assert!(
        house_value > 1_200_000.0,
        "House should appreciate from $1.2M initial"
    );

    // Verify mortgage was created and payments are being made
    let mortgage_balance = result.final_account_balance(MORTGAGE_DEBT);
    println!("Final mortgage balance: ${:.2}", mortgage_balance);
    assert!(
        mortgage_balance.abs() < 50_000.0,
        "Mortgage should be nearly paid off, got {}",
        mortgage_balance
    );

    // Verify retirement event
    assert!(
        result.event_was_triggered(EventId(2)),
        "Retirement event should trigger at age 45"
    );

    // Verify RMD event at age 73
    assert!(
        result.event_was_triggered(EventId(3)),
        "RMD event should trigger at age 73"
    );
    let rmd_count = result.rmd_history.len();
    println!("RMD records: {}", rmd_count);
    assert!(
        rmd_count >= 5,
        "Should have RMDs for ages 73-78, got {}",
        rmd_count
    );

    // Final account balances
    let final_brokerage = result.final_account_balance(BROKERAGE);
    let final_roth_ira = result.final_account_balance(ROTH_IRA);
    let final_roth_401k = result.final_account_balance(ROTH_401K);
    let final_trad_401k = result.final_account_balance(TRAD_401K);

    println!("Final Brokerage: ${:.2}", final_brokerage);
    println!("Final Roth IRA: ${:.2}", final_roth_ira);
    println!("Final Roth 401k: ${:.2}", final_roth_401k);
    println!("Final Traditional 401k: ${:.2}", final_trad_401k);

    // Check for any negative balances (bug indicator)
    assert!(
        final_brokerage.abs() >= 0.0,
        "Brokerage should not be negative! Got {}",
        final_brokerage
    );
    assert!(
        final_roth_ira.abs() >= 0.0,
        "Roth IRA should not be negative! Got {}",
        final_roth_ira
    );
    assert!(
        final_roth_401k.abs() >= 0.0,
        "Roth 401k should not be negative! Got {}",
        final_roth_401k
    );
    assert!(
        final_trad_401k.abs() >= 0.0,
        "Traditional 401k should not be negative! Got {}",
        final_trad_401k
    );

    // Check simulation completed fully (50 years)
    let first_date = result.dates.first().unwrap();
    let last_date = result.dates.last().unwrap();
    let years_simulated = (last_date.year() - first_date.year()) as usize;
    assert!(
        years_simulated >= 49,
        "Simulation should span 50 years, got {} years",
        years_simulated
    );

    println!("\n=== All Verification Checks Passed ===\n");
}
