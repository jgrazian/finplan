//! Comprehensive lifecycle integration tests
//!
//! These tests model realistic financial scenarios over multiple decades
//! using the new event-based system (Transfer, Sweep, etc.).

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountId, AccountType, Asset, AssetClass, AssetId, Event, EventEffect, EventId,
    EventTrigger, FlowLimits, InflationProfile, LimitPeriod, LotMethod, RecordKind, RepeatInterval,
    ReturnProfile, TaxBracket, TaxConfig, TransactionSource, TransferAmount, TransferEndpoint,
    WithdrawalAmountMode, WithdrawalOrder, WithdrawalSources,
};
use crate::simulation::simulate;

/// Comprehensive lifecycle simulation: accumulation, home purchase, retirement, RMD
///
/// This test models a realistic 50-year financial lifecycle:
/// - Multiple accounts with multiple assets
/// - Age-based events (home purchase at 35, retirement at 45)
/// - Repeating contributions with limits
/// - Complex withdrawal strategy in retirement
/// - RMD calculations starting at age 73
#[test]
fn test_comprehensive_lifecycle_simulation() {
    let start_date = jiff::civil::date(2025, 1, 1);
    let birth_date = jiff::civil::date(1997, 3, 16); // Age 28 at start

    // === Asset IDs ===
    const VFIAX: AssetId = AssetId(1); // S&P 500 fund
    const VGPMX: AssetId = AssetId(2); // Precious metals
    const VIMAX: AssetId = AssetId(3); // Mid-cap
    const VTIAX: AssetId = AssetId(4); // International
    const VFIFX: AssetId = AssetId(5); // Target date
    const SP500: AssetId = AssetId(6); // 401k S&P 500
    const HOUSE: AssetId = AssetId(7); // Real estate
    const CASH: AssetId = AssetId(8); // Cash

    // === Account IDs ===
    const BROKERAGE: AccountId = AccountId(1);
    const ROTH_IRA: AccountId = AccountId(2);
    const TRAD_401K: AccountId = AccountId(3);
    const ROTH_401K: AccountId = AccountId(4);
    const REAL_ESTATE: AccountId = AccountId(5);
    const CASH_ACCOUNT: AccountId = AccountId(6);

    // === Event IDs ===
    const EVENT_BROKERAGE_CONTRIBUTION: EventId = EventId(1);
    const EVENT_ROTH_401K_CONTRIBUTION: EventId = EventId(2);
    const EVENT_ROTH_IRA_CONTRIBUTION: EventId = EventId(3);
    const EVENT_HOME_PURCHASE: EventId = EventId(10);
    const EVENT_RETIREMENT: EventId = EventId(20);
    const EVENT_RETIREMENT_SPENDING: EventId = EventId(21);
    const EVENT_RMD: EventId = EventId(30);

    // === Variables ===
    const HOUSE_PRICE: f64 = 1_200_000.0;
    const DOWN_PAYMENT_PERCENT: f64 = 0.20;
    const HOME_PURCHASE_AGE: u8 = 35;
    const RETIREMENT_AGE: u8 = 40;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 65, // Age 28 to 93
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
                        initial_cost_basis: Some(700_000.0),
                    },
                    Asset {
                        asset_id: VGPMX,
                        initial_value: 230_000.0,
                        return_profile_index: 1,
                        asset_class: AssetClass::Investable,
                        initial_cost_basis: Some(200_000.0),
                    },
                    Asset {
                        asset_id: VIMAX,
                        initial_value: 70_000.0,
                        return_profile_index: 2,
                        asset_class: AssetClass::Investable,
                        initial_cost_basis: Some(50_000.0),
                    },
                    Asset {
                        asset_id: VTIAX,
                        initial_value: 80_000.0,
                        return_profile_index: 3,
                        asset_class: AssetClass::Investable,
                        initial_cost_basis: Some(70_000.0),
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
                        initial_cost_basis: None,
                    },
                    Asset {
                        asset_id: VFIFX,
                        initial_value: 15_000.0,
                        return_profile_index: 4,
                        asset_class: AssetClass::Investable,
                        initial_cost_basis: None,
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
                    initial_cost_basis: None,
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
                    initial_cost_basis: None,
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
                    initial_cost_basis: None,
                }],
            },
            // 6. Cash Account (for down payment and emergency fund)
            Account {
                account_id: CASH_ACCOUNT,
                account_type: AccountType::Taxable,
                assets: vec![Asset {
                    asset_id: CASH,
                    initial_value: (HOUSE_PRICE * DOWN_PAYMENT_PERCENT) + 100_000.0,
                    return_profile_index: 6,
                    asset_class: AssetClass::Cash,
                    initial_cost_basis: None,
                }],
            },
        ],
        events: vec![
            // === ACCUMULATION PHASE EVENTS ===

            // Monthly contribution to Brokerage VFIAX
            Event {
                event_id: EVENT_BROKERAGE_CONTRIBUTION,
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Monthly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: BROKERAGE,
                        asset_id: VFIAX,
                    },
                    amount: TransferAmount::Fixed(1_500.0),
                    adjust_for_inflation: true,
                    limits: None,
                }],
                once: false,
            },
            // Mega backdoor Roth 401k - $43.5k/year limit
            Event {
                event_id: EVENT_ROTH_401K_CONTRIBUTION,
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Monthly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: ROTH_401K,
                        asset_id: SP500,
                    },
                    amount: TransferAmount::Fixed(10_000.0), // $10k/month attempt
                    adjust_for_inflation: false,
                    limits: Some(FlowLimits {
                        limit: 43_500.0, // Yearly limit
                        period: LimitPeriod::Yearly,
                    }),
                }],
                once: false,
            },
            // Backdoor Roth IRA - $7k/year
            Event {
                event_id: EVENT_ROTH_IRA_CONTRIBUTION,
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: ROTH_IRA,
                        asset_id: VFIAX,
                    },
                    amount: TransferAmount::Fixed(7_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // === HOME PURCHASE EVENT (Age 35) ===
            Event {
                event_id: EVENT_HOME_PURCHASE,
                trigger: EventTrigger::Age {
                    years: HOME_PURCHASE_AGE,
                    months: Some(3),
                },
                effects: vec![
                    // Down payment from cash account
                    EventEffect::Transfer {
                        from: TransferEndpoint::Asset {
                            account_id: CASH_ACCOUNT,
                            asset_id: CASH,
                        },
                        to: TransferEndpoint::External, // Goes to seller
                        amount: TransferAmount::Fixed(HOUSE_PRICE * DOWN_PAYMENT_PERCENT),
                        adjust_for_inflation: false,
                        limits: None,
                    },
                    // House asset value (full price, we get the asset)
                    EventEffect::Transfer {
                        from: TransferEndpoint::External,
                        to: TransferEndpoint::Asset {
                            account_id: REAL_ESTATE,
                            asset_id: HOUSE,
                        },
                        amount: TransferAmount::Fixed(HOUSE_PRICE),
                        adjust_for_inflation: false,
                        limits: None,
                    },
                ],
                once: true,
            },
            // === RETIREMENT EVENT (Age 45) ===
            Event {
                event_id: EVENT_RETIREMENT,
                trigger: EventTrigger::Age {
                    years: RETIREMENT_AGE,
                    months: Some(0),
                },
                effects: vec![
                    // Stop contributions
                    EventEffect::TerminateEvent(EVENT_BROKERAGE_CONTRIBUTION),
                    EventEffect::TerminateEvent(EVENT_ROTH_401K_CONTRIBUTION),
                    EventEffect::TerminateEvent(EVENT_ROTH_IRA_CONTRIBUTION),
                ],
                once: true,
            },
            // Retirement spending - yearly withdrawal
            Event {
                event_id: EVENT_RETIREMENT_SPENDING,
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: Some(Box::new(EventTrigger::Age {
                        years: RETIREMENT_AGE,
                        months: Some(0),
                    })),
                    end_condition: None,
                },
                effects: vec![EventEffect::Sweep {
                    to_account: CASH_ACCOUNT,
                    to_asset: CASH,
                    target: TransferAmount::Fixed(200_000.0),
                    sources: WithdrawalSources::Strategy {
                        order: WithdrawalOrder::TaxEfficientEarly,
                        exclude_accounts: vec![REAL_ESTATE, CASH_ACCOUNT],
                    },
                    amount_mode: WithdrawalAmountMode::Net,
                    lot_method: LotMethod::HighestCost,
                }],
                once: false,
            },
            // === RMD EVENT (Age 73) ===
            // ApplyRmd automatically processes all tax-deferred accounts
            Event {
                event_id: EVENT_RMD,
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
        },
    };

    let result = simulate(&params, 42);

    // === VERIFICATION CHECKS ===
    println!("\n=== Comprehensive Lifecycle Simulation Results ===");

    // 1. Verify home purchase event triggered
    assert!(
        result.event_was_triggered(EVENT_HOME_PURCHASE),
        "Home purchase event should trigger at age 35"
    );
    let house_value = result.final_account_balance(REAL_ESTATE);
    println!("House final value: ${:.2}", house_value);
    assert!(
        house_value > 1_200_000.0,
        "House should appreciate from $1.2M initial, got {}",
        house_value
    );

    // 2. Verify retirement event triggered
    assert!(
        result.event_was_triggered(EVENT_RETIREMENT),
        "Retirement event should trigger at age 45"
    );

    // // 3. Verify RMD event triggered (age 73)
    // assert!(
    //     result.event_was_triggered(EVENT_RMD),
    //     "RMD event should trigger at age 73"
    // );
    // let rmd_count = result.rmd_records().count();
    // println!("RMD records: {}", rmd_count);
    // assert!(
    //     rmd_count >= 5,
    //     "Should have RMDs for ages 73-78 (5+ years), got {}",
    //     rmd_count
    // );

    // 4. Final account balances
    let final_brokerage = result.final_account_balance(BROKERAGE);
    let final_roth_ira = result.final_account_balance(ROTH_IRA);
    let final_roth_401k = result.final_account_balance(ROTH_401K);
    let final_trad_401k = result.final_account_balance(TRAD_401K);

    println!("Final Brokerage: ${:.2}", final_brokerage);
    println!("Final Roth IRA: ${:.2}", final_roth_ira);
    println!("Final Roth 401k: ${:.2}", final_roth_401k);
    println!("Final Traditional 401k: ${:.2}", final_trad_401k);

    // 5. Check simulation duration (50 years from 2025 to 2075)
    let first_date = result.dates.first().unwrap();
    let last_date = result.dates.last().unwrap();
    let years_simulated = (last_date.year() - first_date.year()) as usize;
    assert!(
        years_simulated >= 49,
        "Simulation should span ~50 years, got {} years",
        years_simulated
    );

    println!("\n=== All Verification Checks Passed ===\n");
}

/// Test accumulation phase only (simpler scenario)
#[test]
fn test_accumulation_phase() {
    let start_date = jiff::civil::date(2025, 1, 1);
    let birth_date = jiff::civil::date(1995, 6, 15); // Age 30 at start

    const BROKERAGE: AccountId = AccountId(1);
    const ROTH_401K: AccountId = AccountId(2);
    const SP500: AssetId = AssetId(1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 10, // Ages 30-39
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::Fixed(0.02),
        return_profiles: vec![ReturnProfile::Fixed(0.07)],
        accounts: vec![
            Account {
                account_id: BROKERAGE,
                account_type: AccountType::Taxable,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 50_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: Some(40_000.0),
                }],
            },
            Account {
                account_id: ROTH_401K,
                account_type: AccountType::TaxFree,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 25_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
        ],
        events: vec![
            // Yearly brokerage contribution
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
                        account_id: BROKERAGE,
                        asset_id: SP500,
                    },
                    amount: TransferAmount::Fixed(12_000.0),
                    adjust_for_inflation: true,
                    limits: None,
                }],
                once: false,
            },
            // Roth 401k contribution with yearly limit
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Monthly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: ROTH_401K,
                        asset_id: SP500,
                    },
                    amount: TransferAmount::Fixed(5_000.0), // $5k/month
                    adjust_for_inflation: false,
                    limits: Some(FlowLimits {
                        limit: 23_000.0, // 2024 401k limit
                        period: LimitPeriod::Yearly,
                    }),
                }],
                once: false,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Both accounts should have grown significantly
    let final_brokerage = result.final_account_balance(BROKERAGE);
    let final_roth = result.final_account_balance(ROTH_401K);

    println!("After 10 years:");
    println!("  Brokerage: ${:.2}", final_brokerage);
    println!("  Roth 401k: ${:.2}", final_roth);

    // Brokerage: $50k initial + ~$12k*10 contributions + growth
    assert!(
        final_brokerage > 200_000.0,
        "Brokerage should grow significantly, got {}",
        final_brokerage
    );

    // Roth 401k: $25k initial + $23k*10 contributions + growth
    assert!(
        final_roth > 300_000.0,
        "Roth 401k should grow significantly, got {}",
        final_roth
    );
}

/// Test retirement withdrawal strategy
#[test]
fn test_retirement_withdrawals() {
    let start_date = jiff::civil::date(2025, 1, 1);
    let birth_date = jiff::civil::date(1960, 1, 1); // Age 65 at start (retired)

    const BROKERAGE: AccountId = AccountId(1);
    const TRAD_IRA: AccountId = AccountId(2);
    const ROTH_IRA: AccountId = AccountId(3);
    const CASH: AccountId = AccountId(4);
    const SP500: AssetId = AssetId(1);
    const CASH_ASSET: AssetId = AssetId(2);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5,
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::Fixed(0.025),
        return_profiles: vec![
            ReturnProfile::Fixed(0.06), // Stocks
            ReturnProfile::Fixed(0.0),  // Cash
        ],
        accounts: vec![
            // Taxable brokerage
            Account {
                account_id: BROKERAGE,
                account_type: AccountType::Taxable,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 500_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: Some(300_000.0),
                }],
            },
            // Traditional IRA
            Account {
                account_id: TRAD_IRA,
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 800_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
            // Roth IRA
            Account {
                account_id: ROTH_IRA,
                account_type: AccountType::TaxFree,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 300_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
            // Cash account for spending
            Account {
                account_id: CASH,
                account_type: AccountType::Taxable,
                assets: vec![Asset {
                    asset_id: CASH_ASSET,
                    initial_value: 0.0,
                    return_profile_index: 1,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
        ],
        events: vec![
            // Annual retirement spending withdrawal
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Sweep {
                    to_account: CASH,
                    to_asset: CASH_ASSET,
                    target: TransferAmount::Fixed(80_000.0), // $80k/year spending
                    sources: WithdrawalSources::Strategy {
                        order: WithdrawalOrder::TaxEfficientEarly, // Taxable first
                        exclude_accounts: vec![CASH],
                    },
                    amount_mode: WithdrawalAmountMode::Net, // After taxes
                    lot_method: LotMethod::HighestCost,
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
            ],
            state_rate: 0.05,
            capital_gains_rate: 0.15,
        },
    };

    let result = simulate(&params, 42);

    // Total starting: $500k + $800k + $300k = $1.6M
    // 5 years of $80k net spending ~ $400k total spent
    // Plus growth at 6%
    let total_start = 500_000.0 + 800_000.0 + 300_000.0;
    let final_brokerage = result.final_account_balance(BROKERAGE);
    let final_trad = result.final_account_balance(TRAD_IRA);
    let final_roth = result.final_account_balance(ROTH_IRA);
    let final_cash = result.final_account_balance(CASH);
    let total_final = final_brokerage + final_trad + final_roth + final_cash;

    println!("Retirement withdrawal test:");
    println!("  Initial total: ${:.2}", total_start);
    println!("  Final Brokerage: ${:.2}", final_brokerage);
    println!("  Final Trad IRA: ${:.2}", final_trad);
    println!("  Final Roth IRA: ${:.2}", final_roth);
    println!("  Final Cash: ${:.2}", final_cash);
    println!("  Final total: ${:.2}", total_final);

    // Tax-efficient early strategy should withdraw from taxable (brokerage) first
    // After 5 years, brokerage should be lower than initial
    assert!(
        final_brokerage < 500_000.0,
        "Brokerage should be drawn down, got {}",
        final_brokerage
    );

    // Cash account should have accumulated some from sweeps
    assert!(final_cash > 0.0, "Cash should have money from sweeps");

    // Verify sweep records exist
    let sweep_count = result
        .records
        .iter()
        .filter(|r| match &r.kind {
            RecordKind::Transfer { source, .. } => {
                matches!(source.as_ref(), &TransactionSource::Sweep { .. })
            }
            _ => false,
        })
        .count();
    assert!(
        sweep_count >= 5,
        "Should have 5 yearly sweep records, got {}",
        sweep_count
    );
}

/// Test event chaining (retirement triggers spending start)
#[test]
fn test_event_chaining_retirement() {
    let start_date = jiff::civil::date(2025, 1, 1);
    let birth_date = jiff::civil::date(1960, 6, 15); // Age 64 at start

    const SAVINGS: AccountId = AccountId(1);
    const CASH: AccountId = AccountId(2);
    const SP500: AssetId = AssetId(1);

    const EVENT_CONTRIBUTION: EventId = EventId(1);
    const EVENT_RETIREMENT: EventId = EventId(2);
    const EVENT_SPENDING: EventId = EventId(3);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5,
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: vec![ReturnProfile::None],
        accounts: vec![
            Account {
                account_id: SAVINGS,
                account_type: AccountType::TaxFree,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 100_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
            Account {
                account_id: CASH,
                account_type: AccountType::TaxFree,
                assets: vec![Asset {
                    asset_id: SP500,
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                    initial_cost_basis: None,
                }],
            },
        ],
        events: vec![
            // Pre-retirement: Monthly $1k contribution
            Event {
                event_id: EVENT_CONTRIBUTION,
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Monthly,
                    start_condition: None,
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::External,
                    to: TransferEndpoint::Asset {
                        account_id: SAVINGS,
                        asset_id: SP500,
                    },
                    amount: TransferAmount::Fixed(1_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
            // Retirement at age 65 - stop contributions
            Event {
                event_id: EVENT_RETIREMENT,
                trigger: EventTrigger::Age {
                    years: 65,
                    months: None,
                },
                effects: vec![EventEffect::TerminateEvent(EVENT_CONTRIBUTION)],
                once: true,
            },
            // Post-retirement: yearly spending
            Event {
                event_id: EVENT_SPENDING,
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: Some(Box::new(EventTrigger::Age {
                        years: 65,
                        months: None,
                    })),
                    end_condition: None,
                },
                effects: vec![EventEffect::Transfer {
                    from: TransferEndpoint::Asset {
                        account_id: SAVINGS,
                        asset_id: SP500,
                    },
                    to: TransferEndpoint::Asset {
                        account_id: CASH,
                        asset_id: SP500,
                    },
                    amount: TransferAmount::Fixed(20_000.0),
                    adjust_for_inflation: false,
                    limits: None,
                }],
                once: false,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    assert!(
        result.event_was_triggered(EVENT_RETIREMENT),
        "Retirement should trigger"
    );

    // Pre-retirement: ~6 months of $1k = $6k contributions (Jan-Jun 2025)
    // Post-retirement: ~4 years of $20k spending
    // Savings: $100k + ~$6k - ~$80k = ~$26k
    let final_savings = result.final_account_balance(SAVINGS);
    let final_cash = result.final_account_balance(CASH);

    println!("Event chaining test:");
    println!("  Final Savings: ${:.2}", final_savings);
    println!("  Final Cash: ${:.2}", final_cash);

    // Verify contributions stopped and spending occurred
    assert!(final_cash > 0.0, "Cash should have spending transfers");
}
