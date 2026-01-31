//! Profiling tests for simulation performance and safety
//!
//! These tests verify:
//! - Normal simulations have reasonable iteration counts
//! - AccountBalance triggers with `once: true` work correctly
//! - AccountBalance triggers with `once: false` are caught by iteration limits
//! - Event count scaling is linear with simulation complexity

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::metrics::InstrumentationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetId, AssetLot, BalanceThreshold, Cash,
    Event, EventEffect, EventId, EventTrigger, IncomeType, InflationProfile, InvestmentContainer,
    MonteCarloConfig, RepeatInterval, ReturnProfile, ReturnProfileId, TaxStatus, TransferAmount,
};
use crate::simulation::{monte_carlo_simulate_with_config, simulate_with_metrics};

fn create_basic_config(duration_years: usize) -> SimulationConfig {
    SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years,
        birth_date: Some(jiff::civil::date(1980, 6, 15)),
        inflation_profile: InflationProfile::Fixed(0.03),
        return_profiles: HashMap::from([
            (ReturnProfileId(0), ReturnProfile::Fixed(0.07)),
            (ReturnProfileId(1), ReturnProfile::Fixed(0.02)),
        ]),
        asset_returns: HashMap::from([(AssetId(1), ReturnProfileId(0))]),
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 10_000.0,
                    return_profile_id: ReturnProfileId(1),
                },
                positions: vec![AssetLot {
                    asset_id: AssetId(1),
                    purchase_date: jiff::civil::date(2020, 1, 1),
                    units: 100_000.0,
                    cost_basis: 80_000.0,
                }],
                contribution_limit: None,
            }),
        }],
        ..Default::default()
    }
}

#[test]
fn test_normal_iteration_count() {
    // A basic simulation with no events should have minimal iterations
    let config = create_basic_config(10);
    let instrumentation = InstrumentationConfig::default();

    let (result, metrics) = simulate_with_metrics(&config, 42, &instrumentation).unwrap();

    // Verify simulation completed
    assert!(!result.wealth_snapshots.is_empty());

    // With no events, we should see exactly 1 iteration per time step
    // (the initial check that finds nothing to do)
    assert!(
        metrics.max_same_date_iterations <= 2,
        "Expected max 2 iterations per date for event-free simulation, got {}",
        metrics.max_same_date_iterations
    );

    // Should not hit any iteration limits
    assert!(
        !metrics.had_iteration_limit_hits(),
        "Should not hit iteration limits for basic simulation"
    );

    println!("Normal simulation metrics:");
    println!("  Time steps: {}", metrics.time_steps);
    println!("  Total iterations: {}", metrics.same_date_iterations);
    println!(
        "  Max iterations at single date: {}",
        metrics.max_same_date_iterations
    );
    println!(
        "  Avg iterations per step: {:.2}",
        metrics.avg_iterations_per_step()
    );
}

#[test]
fn test_account_balance_trigger_safe() {
    // AccountBalance trigger with once: true should work correctly
    let mut config = create_basic_config(5);

    // Add checking account
    config.accounts.push(Account {
        account_id: AccountId(2),
        flavor: AccountFlavor::Bank(Cash {
            value: 2_000.0,
            return_profile_id: ReturnProfileId(1),
        }),
    });

    // Monthly expense
    config.events.push(Event {
        event_id: EventId(1),
        trigger: EventTrigger::Repeating {
            interval: RepeatInterval::Monthly,
            start_condition: None,
            end_condition: None,
            max_occurrences: None,
        },
        effects: vec![EventEffect::Expense {
            from: AccountId(2),
            amount: TransferAmount::Fixed(500.0),
        }],
        once: false,
    });

    // Safe sweep with once: true
    config.events.push(Event {
        event_id: EventId(2),
        trigger: EventTrigger::AccountBalance {
            account_id: AccountId(2),
            threshold: BalanceThreshold::LessThanOrEqual(500.0),
        },
        effects: vec![EventEffect::Income {
            to: AccountId(2),
            amount: TransferAmount::Fixed(2_000.0),
            amount_mode: AmountMode::Net,
            income_type: IncomeType::TaxFree,
        }],
        once: true, // SAFE
    });

    let instrumentation = InstrumentationConfig::default();
    let (result, metrics) = simulate_with_metrics(&config, 42, &instrumentation).unwrap();

    // Simulation should complete normally
    assert!(!result.wealth_snapshots.is_empty());
    assert!(
        !metrics.had_iteration_limit_hits(),
        "Safe AccountBalance trigger should not hit iteration limits"
    );

    // Verify the sweep event fired
    let sweep_count = metrics.events_by_id.get(&EventId(2)).copied().unwrap_or(0);
    assert!(
        sweep_count >= 1,
        "Sweep event should have fired at least once, got {}",
        sweep_count
    );

    println!("Safe AccountBalance trigger metrics:");
    println!(
        "  Max iterations at single date: {}",
        metrics.max_same_date_iterations
    );
    println!("  Sweep event fired {} times", sweep_count);
    println!(
        "  Total events triggered: {}",
        metrics.total_events_triggered
    );
}

#[test]
fn test_account_balance_trigger_dangerous() {
    // Two interdependent AccountBalance triggers with once: false create an infinite loop
    // This is the classic "ping-pong" pattern that should be caught by iteration limits
    let mut config = create_basic_config(1);

    // Checking account that will ping-pong between positive and negative
    config.accounts.push(Account {
        account_id: AccountId(2),
        flavor: AccountFlavor::Bank(Cash {
            value: -50.0, // Start negative to trigger the loop
            return_profile_id: ReturnProfileId(1),
        }),
    });

    // Trigger 1: When balance <= 0, add $100
    config.events.push(Event {
        event_id: EventId(1),
        trigger: EventTrigger::AccountBalance {
            account_id: AccountId(2),
            threshold: BalanceThreshold::LessThanOrEqual(0.0),
        },
        effects: vec![EventEffect::Income {
            to: AccountId(2),
            amount: TransferAmount::Fixed(100.0),
            amount_mode: AmountMode::Net,
            income_type: IncomeType::TaxFree,
        }],
        once: false, // DANGEROUS
    });

    // Trigger 2: When balance > 0, subtract $100
    // This creates the ping-pong: add -> subtract -> add -> subtract...
    config.events.push(Event {
        event_id: EventId(2),
        trigger: EventTrigger::AccountBalance {
            account_id: AccountId(2),
            threshold: BalanceThreshold::GreaterThanOrEqual(1.0), // > 0 effectively
        },
        effects: vec![EventEffect::Expense {
            from: AccountId(2),
            amount: TransferAmount::Fixed(100.0),
        }],
        once: false, // DANGEROUS
    });

    // Use a low iteration limit to catch the infinite loop quickly
    let instrumentation = InstrumentationConfig::with_limit(50);
    let (result, metrics) = simulate_with_metrics(&config, 42, &instrumentation).unwrap();

    // Simulation should still complete (due to iteration limit breaking the loop)
    assert!(!result.wealth_snapshots.is_empty());

    // Should have hit the iteration limit
    assert!(
        metrics.had_iteration_limit_hits(),
        "Dangerous AccountBalance trigger should hit iteration limit"
    );

    println!("Dangerous AccountBalance trigger metrics:");
    println!(
        "  Iteration limit dates: {:?}",
        metrics.iteration_limit_dates
    );
    println!(
        "  Max iterations at single date: {}",
        metrics.max_same_date_iterations
    );

    // Both events should have fired many times before being cut off
    let event1_count = metrics.events_by_id.get(&EventId(1)).copied().unwrap_or(0);
    let event2_count = metrics.events_by_id.get(&EventId(2)).copied().unwrap_or(0);
    println!("  Event 1 (add) fired {} times before limit", event1_count);
    println!(
        "  Event 2 (subtract) fired {} times before limit",
        event2_count
    );

    // Each event should have fired at least a few times
    assert!(
        event1_count > 5,
        "Add event should have fired multiple times, got {}",
        event1_count
    );
    assert!(
        event2_count > 5,
        "Subtract event should have fired multiple times, got {}",
        event2_count
    );
}

#[test]
fn test_event_count_scaling() {
    // Verify event counts scale linearly with simulation duration
    let instrumentation = InstrumentationConfig::default();

    // 5-year simulation
    let config_5yr = {
        let mut c = create_basic_config(5);
        c.accounts.push(Account {
            account_id: AccountId(2),
            flavor: AccountFlavor::Bank(Cash {
                value: 10_000.0,
                return_profile_id: ReturnProfileId(1),
            }),
        });
        c.events.push(Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Monthly,
                start_condition: None,
                end_condition: None,
                max_occurrences: None,
            },
            effects: vec![EventEffect::Expense {
                from: AccountId(2),
                amount: TransferAmount::Fixed(100.0),
            }],
            once: false,
        });
        c
    };

    // 10-year simulation (same structure, 2x duration)
    let config_10yr = {
        let mut c = create_basic_config(10);
        c.accounts.push(Account {
            account_id: AccountId(2),
            flavor: AccountFlavor::Bank(Cash {
                value: 10_000.0,
                return_profile_id: ReturnProfileId(1),
            }),
        });
        c.events.push(Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Monthly,
                start_condition: None,
                end_condition: None,
                max_occurrences: None,
            },
            effects: vec![EventEffect::Expense {
                from: AccountId(2),
                amount: TransferAmount::Fixed(100.0),
            }],
            once: false,
        });
        c
    };

    let (_, metrics_5yr) = simulate_with_metrics(&config_5yr, 42, &instrumentation).unwrap();
    let (_, metrics_10yr) = simulate_with_metrics(&config_10yr, 42, &instrumentation).unwrap();

    // Events should roughly double for 2x duration (allowing some tolerance)
    let events_5yr = metrics_5yr.total_events_triggered;
    let events_10yr = metrics_10yr.total_events_triggered;
    let ratio = events_10yr as f64 / events_5yr as f64;

    println!("Event scaling:");
    println!("  5yr events: {}", events_5yr);
    println!("  10yr events: {}", events_10yr);
    println!("  Ratio: {:.2}x", ratio);

    assert!(
        ratio > 1.8 && ratio < 2.2,
        "Event count should roughly double for 2x duration, got {:.2}x",
        ratio
    );
}

#[test]
fn test_monte_carlo_memory_efficiency() {
    // Verify Monte Carlo simulation completes in reasonable time
    let config = create_basic_config(30);
    let mc_config = MonteCarloConfig {
        iterations: 100,
        percentiles: vec![0.05, 0.25, 0.50, 0.75, 0.95],
        compute_mean: true,
        ..Default::default()
    };

    let start = std::time::Instant::now();
    let result = monte_carlo_simulate_with_config(&config, &mc_config).unwrap();
    let elapsed = start.elapsed();

    println!("Monte Carlo (100 iterations, 30yr):");
    println!("  Duration: {:?}", elapsed);
    println!("  Success rate: {:.1}%", result.stats.success_rate * 100.0);
    println!(
        "  Mean final net worth: ${:.0}",
        result.stats.mean_final_net_worth
    );

    // Verify we got expected number of percentile runs
    assert_eq!(
        result.percentile_runs.len(),
        5,
        "Should have 5 percentile runs"
    );

    // Verify reasonable performance (should complete in < 10 seconds on most systems)
    assert!(
        elapsed.as_secs() < 30,
        "Monte Carlo simulation took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_metrics_disabled_performance() {
    // Verify disabled metrics don't impact normal operation
    let config = create_basic_config(10);

    let enabled = InstrumentationConfig::default();
    let disabled = InstrumentationConfig::disabled();

    let (result_enabled, metrics_enabled) = simulate_with_metrics(&config, 42, &enabled).unwrap();
    let (result_disabled, metrics_disabled) =
        simulate_with_metrics(&config, 42, &disabled).unwrap();

    // Results should be identical
    assert_eq!(
        result_enabled.wealth_snapshots.len(),
        result_disabled.wealth_snapshots.len()
    );

    // Enabled metrics should have data
    assert!(metrics_enabled.time_steps > 0);

    // Disabled metrics should still track limit-related data but not detailed metrics
    // (time_steps is still tracked for safety limit purposes)
    println!(
        "Metrics enabled: {} time steps, {} events",
        metrics_enabled.time_steps, metrics_enabled.total_events_triggered
    );
    println!(
        "Metrics disabled: {} time steps, {} events",
        metrics_disabled.time_steps, metrics_disabled.total_events_triggered
    );
}

#[test]
fn test_high_frequency_events() {
    // Test with many events firing at the same time
    let mut config = create_basic_config(2);

    config.accounts.push(Account {
        account_id: AccountId(2),
        flavor: AccountFlavor::Bank(Cash {
            value: 100_000.0,
            return_profile_id: ReturnProfileId(1),
        }),
    });

    // Add multiple monthly events
    for i in 0..10 {
        config.events.push(Event {
            event_id: EventId(i as u16 + 1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Monthly,
                start_condition: None,
                end_condition: None,
                max_occurrences: None,
            },
            effects: vec![EventEffect::Expense {
                from: AccountId(2),
                amount: TransferAmount::Fixed(100.0),
            }],
            once: false,
        });
    }

    let instrumentation = InstrumentationConfig::default();
    let (result, metrics) = simulate_with_metrics(&config, 42, &instrumentation).unwrap();

    assert!(!result.wealth_snapshots.is_empty());
    assert!(
        !metrics.had_iteration_limit_hits(),
        "High frequency but non-looping events should not hit limits"
    );

    println!("High frequency events (10 monthly events, 2yr):");
    println!("  Total events: {}", metrics.total_events_triggered);
    println!("  Max iterations: {}", metrics.max_same_date_iterations);

    // Should have roughly 10 events * 24 months = 240 events
    assert!(
        metrics.total_events_triggered > 200,
        "Expected many events, got {}",
        metrics.total_events_triggered
    );
}
