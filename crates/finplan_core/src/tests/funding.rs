//! Funding is checked throughout the path, not inferred from terminal wealth.
use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, Cash, Event, EventEffect, EventId, EventTrigger, LoanDetail,
    MonteCarloConfig, MonteCarloProgress, ReturnProfile, ReturnProfileId, TransferAmount,
    WarningKind, final_net_worth,
};
use crate::simulation::{monte_carlo_simulate_with_config, monte_carlo_stats_only, simulate};

fn plan() -> SimulationConfig {
    SimulationConfig {
        start_date: Some(jiff::civil::date(2026, 1, 1)),
        duration_years: 1,
        return_profiles: HashMap::from([(ReturnProfileId(0), ReturnProfile::Fixed(0.0))]),
        accounts: vec![bank(0, 100.0), bank(1, 10_000.0)],
        ..Default::default()
    }
}

fn bank(id: u16, value: f64) -> Account {
    Account {
        account_id: AccountId(id),
        flavor: AccountFlavor::Bank(Cash {
            value,
            return_profile_id: ReturnProfileId(0),
        }),
    }
}

fn event(id: u16, month: i8, effects: Vec<EventEffect>) -> Event {
    Event {
        event_id: EventId(id),
        trigger: EventTrigger::Date(jiff::civil::date(2026, month, 1)),
        effects,
        once: true,
    }
}

fn spend(amount: f64) -> EventEffect {
    EventEffect::Expense {
        from: AccountId(0),
        amount: TransferAmount::Fixed(amount),
    }
}

fn fund(amount: f64) -> EventEffect {
    EventEffect::CashTransfer {
        from: AccountId(1),
        to: AccountId(0),
        amount: TransferAmount::Fixed(amount),
    }
}

fn mc() -> MonteCarloConfig {
    MonteCarloConfig {
        iterations: 16,
        seed: Some(42),
        compute_mean: false,
        ..Default::default()
    }
}

#[test]
fn positive_terminal_wealth_does_not_mean_cash_was_funded() {
    let mut config = plan();
    config.events = vec![
        event(0, 2, vec![spend(500.0)]),
        event(1, 3, vec![fund(1_000.0)]),
    ];
    for collect_ledger in [true, false] {
        config.collect_ledger = collect_ledger;
        let result = simulate(&config, 42).unwrap();
        assert!(final_net_worth(&result) > 0.0);
        assert!(result.final_account_balance(AccountId(0)).unwrap() > 0.0);
        let warnings: Vec<_> = result
            .warnings
            .iter()
            .filter(|w| w.kind == WarningKind::CashShortfall)
            .collect();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].date, jiff::civil::date(2026, 2, 1));
        let summary = monte_carlo_simulate_with_config(&config, &mc()).unwrap();
        assert_eq!(summary.stats.success_rate, 1.0);
        assert_eq!(summary.stats.funding_success_rate, Some(0.0));
        let (stats, _) =
            monte_carlo_stats_only(&config, &mc(), &MonteCarloProgress::new()).unwrap();
        assert_eq!(stats.funding_success_rate, Some(0.0));
    }
}

#[test]
fn same_date_funding_settles_before_check_even_across_trigger_passes() {
    let mut config = plan();
    config.events = vec![
        Event {
            event_id: EventId(0),
            trigger: EventTrigger::Manual,
            effects: vec![fund(500.0)],
            once: true,
        },
        event(
            1,
            2,
            vec![spend(500.0), EventEffect::TriggerEvent(EventId(0))],
        ),
    ];
    let summary = monte_carlo_simulate_with_config(&config, &mc()).unwrap();
    assert_eq!(summary.stats.funding_success_rate, Some(1.0));
    assert!(
        summary
            .percentile_runs
            .iter()
            .all(|(_, r)| r.warnings.is_empty())
    );
}

#[test]
fn unfunded_accounts_are_not_offset_by_other_accounts_or_later_recovery() {
    let mut config = plan();
    config.events = vec![
        event(0, 2, vec![spend(500.0)]),
        event(1, 5, vec![spend(500.0)]),
    ];
    let result = simulate(&config, 42).unwrap();
    assert_eq!(
        result
            .warnings
            .iter()
            .filter(|w| w.kind == WarningKind::CashShortfall)
            .count(),
        1
    );
}

#[test]
fn ordinary_liabilities_are_not_cash_shortfalls_and_zero_is_funded() {
    let mut config = plan();
    config.accounts = vec![
        bank(0, 0.0),
        Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Liability(LoanDetail {
                principal: 1000.0,
                interest_rate: 0.0,
            }),
        },
    ];
    let summary = monte_carlo_simulate_with_config(&config, &mc()).unwrap();
    assert_eq!(summary.stats.success_rate, 0.0);
    assert_eq!(summary.stats.funding_success_rate, Some(1.0));
}

#[test]
fn half_cent_tolerance_does_not_hide_material_shortfalls() {
    for (cash, funded) in [(-0.004, 1.0), (-0.006, 0.0), (-1.0, 0.0)] {
        let mut config = plan();
        config.accounts[0] = bank(0, cash);
        assert_eq!(
            monte_carlo_simulate_with_config(&config, &mc())
                .unwrap()
                .stats
                .funding_success_rate,
            Some(funded)
        );
    }
}

#[test]
fn skipped_effect_is_not_a_successful_funding_check() {
    let mut config = plan();
    config.events = vec![event(
        0,
        2,
        vec![EventEffect::Expense {
            from: AccountId(99),
            amount: TransferAmount::Fixed(100.0),
        }],
    )];
    let summary = monte_carlo_simulate_with_config(&config, &mc()).unwrap();
    assert_eq!(summary.stats.success_rate, 1.0);
    assert_eq!(summary.stats.funding_success_rate, Some(0.0));
    assert!(
        summary.percentile_runs[0]
            .1
            .warnings
            .iter()
            .any(|w| w.kind != WarningKind::CashShortfall)
    );
}

#[test]
fn chained_effect_errors_also_fail_the_funding_check() {
    let mut config = plan();
    config.events = vec![
        Event {
            event_id: EventId(0),
            trigger: EventTrigger::Manual,
            once: true,
            effects: vec![EventEffect::Expense {
                from: AccountId(99),
                amount: TransferAmount::Fixed(100.0),
            }],
        },
        event(1, 2, vec![EventEffect::TriggerEvent(EventId(0))]),
    ];
    let summary = monte_carlo_simulate_with_config(&config, &mc()).unwrap();
    assert_eq!(summary.stats.funding_success_rate, Some(0.0));
    assert!(
        summary.percentile_runs[0]
            .1
            .warnings
            .iter()
            .any(|w| w.event_id == Some(EventId(0)))
    );
}

#[test]
fn investment_cash_shortfalls_cannot_be_offset_by_positions() {
    let mut config = plan();
    config.accounts[0].flavor = AccountFlavor::Investment(crate::model::InvestmentContainer {
        tax_status: crate::model::TaxStatus::Taxable,
        cash: Cash {
            value: -10.0,
            return_profile_id: ReturnProfileId(0),
        },
        positions: vec![crate::model::AssetLot {
            asset_id: crate::model::AssetId(0),
            purchase_date: jiff::civil::date(2026, 1, 1),
            units: 100.0,
            cost_basis: 10_000.0,
        }],
        contribution_limit: None,
    });
    config
        .asset_returns
        .insert(crate::model::AssetId(0), ReturnProfileId(0));
    config.asset_prices.insert(crate::model::AssetId(0), 100.0);
    let summary = monte_carlo_simulate_with_config(&config, &mc()).unwrap();
    assert_eq!(summary.stats.success_rate, 1.0);
    assert_eq!(summary.stats.funding_success_rate, Some(0.0));
}

#[test]
fn failed_trigger_evaluation_cannot_silently_skip_spending() {
    let mut config = plan();
    config.events = vec![Event {
        event_id: EventId(0),
        once: true,
        trigger: EventTrigger::AccountBalance {
            account_id: AccountId(99),
            threshold: crate::model::BalanceThreshold::LessThanOrEqual(100.0),
        },
        effects: vec![spend(500.0)],
    }];
    let summary = monte_carlo_simulate_with_config(&config, &mc()).unwrap();
    assert_eq!(summary.stats.funding_success_rate, Some(0.0));
    assert!(
        summary.percentile_runs[0]
            .1
            .warnings
            .iter()
            .any(|w| w.message.starts_with("failed to evaluate trigger"))
    );
}

#[test]
fn funding_counts_all_iterations_and_matches_replayed_paths() {
    let mut config = plan();
    config.events = vec![event(
        0,
        2,
        vec![EventEffect::Random {
            probability: 0.5,
            on_true: Box::new(spend(500.0)),
            on_false: None,
        }],
    )];
    let mut mc_config = mc();
    mc_config.percentiles = (0..16).map(|i| f64::from(i) / 16.0).collect();
    let summary = monte_carlo_simulate_with_config(&config, &mc_config).unwrap();
    let funded = summary
        .percentile_runs
        .iter()
        .filter(|(_, r)| r.warnings.is_empty())
        .count();
    assert!(funded > 0 && funded < 16);
    assert_eq!(
        summary.stats.funding_success_rate,
        Some(funded as f64 / 16.0)
    );
}
