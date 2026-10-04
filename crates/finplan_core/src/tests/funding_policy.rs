//! The funding policy: selling investments to cover cash deficits, real
//! pro-rata draws, the tax cash-flow kind and percentile seeds.

use crate::config::{
    AccountBuilder, AssetBuilder, EventBuilder, SimulationBuilder, SimulationConfig,
    SimulationMetadata,
};
use crate::model::{
    AccountId, CashFlowKind, FundingPolicy, MonteCarloConfig, ReturnProfile, SimulationResult,
    StateEvent, WarningKind, WithdrawalOrder, final_net_worth,
};
use crate::simulation::{monte_carlo_simulate_with_config, simulate};

const SEED: u64 = 42;

/// Checking with $1,000, two taxable brokerages and a $2,000 monthly expense.
/// Prices are flat and basis equals value, so a sale realises no gain.
fn plan(brokerage_a: f64, brokerage_b: f64) -> (SimulationConfig, SimulationMetadata) {
    SimulationBuilder::new()
        .start(2026, 1, 1)
        .years(1)
        .inflation(0.0)
        .asset(AssetBuilder::new("FUND").price(1.0).fixed_return(0.0))
        .bank("Checking", 1_000.0)
        .account(AccountBuilder::taxable_brokerage("A").cash(0.0))
        .account(AccountBuilder::taxable_brokerage("B").cash(0.0))
        .position("A", "FUND", brokerage_a, brokerage_a)
        .position("B", "FUND", brokerage_b, brokerage_b)
        .event(
            EventBuilder::expense("Rent")
                .from_account("Checking")
                .amount(2_000.0)
                .monthly(),
        )
        .build()
}

fn policy(order: WithdrawalOrder) -> Option<FundingPolicy> {
    Some(FundingPolicy {
        order,
        exclude_accounts: vec![],
        from: None,
    })
}

fn shortfalls(result: &SimulationResult) -> usize {
    result
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::CashShortfall)
        .count()
}

/// Proceeds of each `AssetSale` with no source event, by selling account.
fn policy_sales(result: &SimulationResult, account: AccountId) -> f64 {
    result
        .ledger
        .iter()
        .filter(|e| e.source_event.is_none())
        .filter_map(|e| match &e.event {
            StateEvent::AssetSale {
                account_id,
                proceeds,
                ..
            } if *account_id == account => Some(*proceeds),
            _ => None,
        })
        .sum()
}

fn all_sales(result: &SimulationResult) -> Vec<&crate::model::LedgerEntry> {
    result
        .ledger
        .iter()
        .filter(|e| matches!(e.event, StateEvent::AssetSale { .. }))
        .collect()
}

#[test]
fn without_a_policy_the_deficit_is_a_shortfall() {
    let (config, _) = plan(100_000.0, 100_000.0);
    let result = simulate(&config, SEED).unwrap();
    assert_eq!(shortfalls(&result), 1);
    assert!(all_sales(&result).is_empty());
    let message = &result.warnings[0].message;
    assert!(message.contains("add a withdrawal"), "{message}");
}

#[test]
fn a_policy_sells_investments_to_cover_spending() {
    let (mut config, meta) = plan(100_000.0, 100_000.0);
    config.funding = policy(WithdrawalOrder::TaxEfficientEarly);
    let result = simulate(&config, SEED).unwrap();
    assert_eq!(shortfalls(&result), 0);

    let sales = all_sales(&result);
    assert!(!sales.is_empty());
    assert!(sales.iter().all(|e| e.source_event.is_none()));

    // $24,000 spent from $1,000 of cash; the rest came from investments.
    let checking = meta.account_id("Checking").unwrap();
    assert!(result.final_account_balance(checking).unwrap() >= -0.01);
    let net = final_net_worth(&result);
    assert!((net - (201_000.0 - 24_000.0)).abs() < 1.0, "{net}");
}

#[test]
fn the_policy_waits_for_its_from_date() {
    let (mut config, _) = plan(100_000.0, 100_000.0);
    config.funding = Some(FundingPolicy {
        order: WithdrawalOrder::TaxEfficientEarly,
        exclude_accounts: vec![],
        from: Some(jiff::civil::date(2026, 6, 1)),
    });
    let result = simulate(&config, SEED).unwrap();
    let sales = all_sales(&result);
    assert!(!sales.is_empty());
    assert!(
        sales
            .iter()
            .all(|e| e.date >= jiff::civil::date(2026, 6, 1))
    );
    // The early months were short, and the warning says so.
    assert_eq!(shortfalls(&result), 1);
    let first = result
        .warnings
        .iter()
        .find(|w| w.kind == WarningKind::CashShortfall)
        .unwrap();
    assert!(first.date < jiff::civil::date(2026, 6, 1));
    assert!(first.message.contains("add a withdrawal"));
}

#[test]
fn excluded_accounts_are_never_sold() {
    let (mut config, meta) = plan(100_000.0, 100_000.0);
    let a = meta.account_id("A").unwrap();
    let b = meta.account_id("B").unwrap();
    config.funding = Some(FundingPolicy {
        order: WithdrawalOrder::TaxEfficientEarly,
        exclude_accounts: vec![a],
        from: None,
    });
    let result = simulate(&config, SEED).unwrap();
    assert_eq!(shortfalls(&result), 0);
    assert_eq!(policy_sales(&result, a), 0.0);
    assert!(policy_sales(&result, b) > 0.0);
}

#[test]
fn an_event_sweep_fires_first_and_the_policy_covers_the_rest() {
    let (mut config, meta) = SimulationBuilder::new()
        .start(2026, 1, 1)
        .years(1)
        .inflation(0.0)
        .asset(AssetBuilder::new("FUND").price(1.0).fixed_return(0.0))
        .bank("Checking", 0.0)
        .account(AccountBuilder::taxable_brokerage("A").cash(0.0))
        .account(AccountBuilder::taxable_brokerage("B").cash(0.0))
        .position("A", "FUND", 100_000.0, 100_000.0)
        .position("B", "FUND", 100_000.0, 100_000.0)
        .event(
            EventBuilder::expense("Rent")
                .from_account("Checking")
                .amount(2_000.0)
                .monthly(),
        )
        // The sweep funds only $500 of each $2,000, from account B.
        .event(
            EventBuilder::withdrawal("Top up")
                .to_account("Checking")
                .from_single_account("B")
                .amount(500.0)
                .monthly(),
        )
        .build();
    config.funding = policy(WithdrawalOrder::TaxEfficientEarly);
    let result = simulate(&config, SEED).unwrap();
    let a = meta.account_id("A").unwrap();
    let b = meta.account_id("B").unwrap();
    assert_eq!(shortfalls(&result), 0);
    assert!(result.ledger.iter().any(|e| e.source_event.is_some()
        && matches!(e.event, StateEvent::AssetSale { account_id, .. } if account_id == b)));
    // Taxable-first order takes the lowest id: the policy sells A, not B.
    assert!(policy_sales(&result, a) > 0.0);
    assert_eq!(policy_sales(&result, b), 0.0);
    // Whatever the sweep did not supply is what the policy sold: $1,500 a month.
    assert!((policy_sales(&result, a) - 18_000.0).abs() < 1.0);
}

#[test]
fn exhausted_investments_still_record_the_shortfall() {
    let (mut config, _) = plan(3_000.0, 3_000.0);
    config.funding = policy(WithdrawalOrder::TaxEfficientEarly);
    let result = simulate(&config, SEED).unwrap();
    assert_eq!(shortfalls(&result), 1);
    let message = &result
        .warnings
        .iter()
        .find(|w| w.kind == WarningKind::CashShortfall)
        .unwrap()
        .message;
    assert!(message.contains("funding policy"), "{message}");
    assert!(!message.contains("add a withdrawal"), "{message}");
    // Everything sellable was sold.
    let sold: f64 = all_sales(&result)
        .iter()
        .map(|e| match e.event {
            StateEvent::AssetSale { proceeds, .. } => proceeds,
            _ => 0.0,
        })
        .sum();
    assert!((sold - 6_000.0).abs() < 1.0, "{sold}");
}

#[test]
fn pro_rata_splits_by_market_value() {
    let (mut config, meta) = plan(30_000.0, 90_000.0);
    config.funding = policy(WithdrawalOrder::ProRata);
    let result = simulate(&config, SEED).unwrap();
    assert_eq!(shortfalls(&result), 0);
    let a = policy_sales(&result, meta.account_id("A").unwrap());
    let b = policy_sales(&result, meta.account_id("B").unwrap());
    assert!(a > 0.0 && b > 0.0);
    // Each draw is a share of the balances at that moment, so the 1:3 split
    // of value holds in total as both shrink in step.
    assert!((b / a - 3.0).abs() < 0.01, "{a} {b}");
}

#[test]
fn pro_rata_with_a_nearly_empty_account_still_funds_everything() {
    let (mut config, meta) = plan(500.0, 100_000.0);
    config.funding = policy(WithdrawalOrder::ProRata);
    let result = simulate(&config, SEED).unwrap();
    assert_eq!(shortfalls(&result), 0);
    let a = policy_sales(&result, meta.account_id("A").unwrap());
    let b = policy_sales(&result, meta.account_id("B").unwrap());
    assert!(a > 0.0 && a < 500.0, "{a}");
    assert!((a + b - 23_000.0).abs() < 1.0, "{a} {b}");
}

#[test]
fn no_policy_matches_the_run_from_before_the_field_existed() {
    let (config, _) = plan(100_000.0, 100_000.0);
    assert!(config.funding.is_none());
    let one = simulate(&config, SEED).unwrap();
    let two = simulate(&config, SEED).unwrap();
    assert_eq!(final_net_worth(&one), final_net_worth(&two));
    assert_eq!(one.ledger.len(), two.ledger.len());
    // Nothing is sold and the books are exactly the cash the plan started with.
    assert!(all_sales(&one).is_empty());
    assert!((final_net_worth(&one) - (201_000.0 - 24_000.0)).abs() < 1.0);
    // A stored config without the field loads with the policy off.
    let back: SimulationConfig = serde_json::from_str("{}").unwrap();
    assert!(back.funding.is_none());
}

#[test]
fn percentile_seeds_reproduce_the_percentile_paths() {
    let (mut config, _) = SimulationBuilder::new()
        .start(2026, 1, 1)
        .years(10)
        .inflation(0.02)
        .asset(
            AssetBuilder::new("FUND")
                .price(1.0)
                .return_profile(ReturnProfile::Normal {
                    mean: 0.07,
                    std_dev: 0.15,
                }),
        )
        .account(AccountBuilder::taxable_brokerage("A").cash(0.0))
        .position("A", "FUND", 100_000.0, 100_000.0)
        .build();
    config.collect_ledger = true;
    let mc = MonteCarloConfig {
        iterations: 50,
        seed: Some(7),
        compute_mean: false,
        ..Default::default()
    };
    let summary = monte_carlo_simulate_with_config(&config, &mc).unwrap();
    assert_eq!(
        summary.percentile_seeds.len(),
        summary.percentile_runs.len()
    );
    for (p, seed) in &summary.percentile_seeds {
        let replay = simulate(&config, *seed).unwrap();
        let path = summary.get_percentile(*p).unwrap();
        assert_eq!(final_net_worth(&replay), final_net_worth(path), "p{p}");
    }
    assert!(summary.get_percentile(50.0).is_some() || !summary.percentile_seeds.is_empty());
}

#[test]
fn rsu_sell_to_cover_tax_is_its_own_kind_but_still_an_expense() {
    let (config, _) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(1)
        .inflation(0.0)
        .asset(AssetBuilder::new("GOOG").price(100.0).fixed_return(0.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
        .event(
            EventBuilder::rsu_vesting("RSU Vest")
                .to_account("Brokerage")
                .asset_in("Brokerage", "GOOG")
                .units(100.0)
                .sell_to_cover()
                .on_date(jiff::civil::date(2025, 3, 15))
                .once(),
        )
        .build();
    let result = simulate(&config, SEED).unwrap();
    let taxes: f64 = result
        .ledger
        .iter()
        .filter_map(|e| match e.event {
            StateEvent::CashDebit {
                amount,
                kind: CashFlowKind::Tax,
                ..
            } => Some(amount),
            _ => None,
        })
        .sum();
    assert!(taxes > 1_000.0, "{taxes}");
    assert!(!result.ledger.iter().any(|e| matches!(
        e.event,
        StateEvent::CashDebit {
            kind: CashFlowKind::Expense,
            ..
        }
    )));
    let expenses: f64 = result.yearly_cash_flows.iter().map(|y| y.expenses).sum();
    assert!((expenses - taxes).abs() < 1e-6, "{expenses} {taxes}");
}
