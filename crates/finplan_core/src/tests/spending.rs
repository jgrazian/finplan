//! Dynamic spending (spec 22): the `Spending` effect's rules, its
//! pass-through, its cadence, and what it adds to the results.
use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetId, AssetLot, Cash, CashFlowKind, Event,
    EventEffect, EventId, EventTrigger, FundingPolicy, IncomeType, InflationProfile,
    InvestmentContainer, MonteCarloConfig, RepeatInterval, ReturnProfile, ReturnProfileId,
    SimulationResult, SpendingInitial, SpendingPortfolio, SpendingRule, StateEvent, TaxConfig,
    TaxStatus, TransferAmount, WarningKind, WithdrawalOrder, final_net_worth,
};
use crate::simulation::{
    BatchSpec, MonteCarloCoordinator, merge_batches, monte_carlo_simulate_with_config, prepare_run,
    run_batch, simulate,
};

const CHECKING: AccountId = AccountId(1);
const BROKERAGE: AccountId = AccountId(2);
const FUND: AssetId = AssetId(1);
const MARKET: ReturnProfileId = ReturnProfileId(0);
const FLAT: ReturnProfileId = ReturnProfileId(1);
/// The spending event; other events take lower ids so they run first.
const SPEND: EventId = EventId(10);

fn date(y: i16, m: i8, d: i8) -> jiff::civil::Date {
    jiff::civil::date(y, m, d)
}

/// $1M in a taxable brokerage (basis equal to value), an empty checking
/// account, no tax, a funding policy, and the market at `growth` a year.
fn plan(years: usize, growth: f64, inflation: f64) -> SimulationConfig {
    SimulationConfig {
        start_date: Some(date(2026, 1, 1)),
        duration_years: years,
        birth_date: Some(date(1965, 6, 1)),
        inflation_profile: InflationProfile::Fixed(inflation),
        return_profiles: HashMap::from([
            (MARKET, ReturnProfile::Fixed(growth)),
            (FLAT, ReturnProfile::Fixed(0.0)),
        ]),
        asset_returns: HashMap::from([(FUND, MARKET)]),
        tax_config: TaxConfig {
            federal_brackets: vec![],
            capital_gains_brackets: vec![],
            state_rate: 0.0,
            ..TaxConfig::default()
        },
        accounts: vec![
            Account {
                account_id: CHECKING,
                flavor: AccountFlavor::Bank(Cash {
                    value: 0.0,
                    return_profile_id: FLAT,
                }),
            },
            Account {
                account_id: BROKERAGE,
                flavor: AccountFlavor::Investment(InvestmentContainer {
                    tax_status: TaxStatus::Taxable,
                    cash: Cash {
                        value: 0.0,
                        return_profile_id: FLAT,
                    },
                    positions: vec![AssetLot {
                        asset_id: FUND,
                        purchase_date: date(2020, 1, 1),
                        units: 1_000_000.0,
                        cost_basis: 1_000_000.0,
                    }],
                    contribution_limit: None,
                }),
            },
        ],
        funding: Some(FundingPolicy {
            order: WithdrawalOrder::TaxEfficientEarly,
            exclude_accounts: vec![],
            from: None,
        }),
        ..Default::default()
    }
}

fn repeating(id: EventId, interval: RepeatInterval, start: jiff::civil::Date) -> EventTrigger {
    let _ = id;
    EventTrigger::Repeating {
        interval,
        start_condition: Some(Box::new(EventTrigger::Date(start))),
        end_condition: None,
        max_occurrences: None,
    }
}

fn with_spending(
    mut config: SimulationConfig,
    rule: SpendingRule,
    interval: RepeatInterval,
    start: jiff::civil::Date,
    pass_through: bool,
) -> SimulationConfig {
    config.events.push(Event {
        event_id: SPEND,
        trigger: repeating(SPEND, interval, start),
        effects: vec![EventEffect::Spending {
            from: CHECKING,
            rule,
            portfolio: SpendingPortfolio::Liquid,
            pass_through,
        }],
        once: false,
    });
    config
}

fn add_event(config: &mut SimulationConfig, id: u16, trigger: EventTrigger, effect: EventEffect) {
    config.events.push(Event {
        event_id: EventId(id),
        trigger,
        effects: vec![effect],
        once: false,
    });
}

/// What the spending event debited each calendar year.
fn draws_by_year(result: &SimulationResult) -> Vec<(i16, f64)> {
    let mut years: std::collections::BTreeMap<i16, f64> = std::collections::BTreeMap::new();
    for entry in &result.ledger {
        if entry.source_event == Some(SPEND)
            && let StateEvent::CashDebit { amount, .. } = entry.event
        {
            *years.entry(entry.date.year()).or_default() += amount;
        }
    }
    years.into_iter().collect()
}

/// Every expense debit (any source) in `year`.
fn expenses_in(result: &SimulationResult, year: i16) -> f64 {
    result
        .ledger
        .iter()
        .filter(|e| e.date.year() == year)
        .filter_map(|e| match e.event {
            StateEvent::CashDebit {
                amount,
                kind: CashFlowKind::Expense,
                ..
            } => Some(amount),
            _ => None,
        })
        .sum()
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

fn guardrails(initial: f64) -> SpendingRule {
    guardrails_with(initial, 0.20, 0)
}

/// Guardrails stepping 10%, from a fixed start, skipping the raise after a
/// loss.
fn guardrails_with(initial: f64, band: f64, protect_final_years: u8) -> SpendingRule {
    SpendingRule::Guardrails {
        initial: SpendingInitial::Amount(initial),
        band,
        step: 0.10,
        skip_inflation_after_loss: true,
        protect_final_years,
        floor: None,
        ceiling: None,
    }
}

fn crash(config: &mut SimulationConfig, on: jiff::civil::Date, drop: f64) {
    add_event(
        config,
        5,
        EventTrigger::Date(on),
        EventEffect::MarketShock { drop },
    );
}

// ── Rules on a fixed-return profile ─────────────────────────────────────

#[test]
fn percent_of_portfolio_is_rate_times_the_balance() {
    let config = with_spending(
        plan(4, 0.0, 0.0),
        SpendingRule::PercentOfPortfolio {
            rate: 0.04,
            floor: None,
            ceiling: None,
        },
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    let result = simulate(&config, 1).unwrap();
    let draws = draws_by_year(&result);
    let mut portfolio = 1_000_000.0;
    for (year, drawn) in &draws {
        assert!(close(*drawn, 0.04 * portfolio, 0.01), "{year}: {drawn}");
        portfolio -= drawn;
    }
    assert_eq!(draws.len(), 4);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
}

#[test]
fn percent_of_portfolio_clamps_hold() {
    let rule = |floor, ceiling| SpendingRule::PercentOfPortfolio {
        rate: 0.04,
        floor,
        ceiling,
    };
    // 4% of $1M is $40,000; the next year 4% of $960,000 is $38,400.
    let floored = with_spending(
        plan(3, 0.0, 0.0),
        rule(Some(39_000.0), None),
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    let draws = draws_by_year(&simulate(&floored, 1).unwrap());
    assert!(close(draws[0].1, 40_000.0, 0.01));
    assert!(close(draws[1].1, 39_000.0, 0.01), "{draws:?}");

    let capped = with_spending(
        plan(3, 0.0, 0.0),
        rule(None, Some(35_000.0)),
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    let draws = draws_by_year(&simulate(&capped, 1).unwrap());
    assert!(
        draws.iter().all(|(_, w)| close(*w, 35_000.0, 0.01)),
        "{draws:?}"
    );
}

#[test]
fn clamps_are_in_plan_start_dollars() {
    // 3% inflation: a $50,000 floor is $51,500 a year in.
    let config = with_spending(
        plan(2, 0.0, 0.03),
        SpendingRule::PercentOfPortfolio {
            rate: 0.001,
            floor: Some(50_000.0),
            ceiling: None,
        },
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    let draws = draws_by_year(&simulate(&config, 1).unwrap());
    assert!(close(draws[0].1, 50_000.0, 0.01));
    assert!(close(draws[1].1, 51_500.0, 0.01), "{draws:?}");
}

#[test]
fn vpw_spends_the_portfolio_down_by_the_final_age() {
    // Born mid-1965: 60 on the first draw, 69 on the tenth.
    let config = with_spending(
        plan(10, 0.0, 0.0),
        SpendingRule::Vpw {
            real_return: 0.0,
            final_age: 70,
        },
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    let result = simulate(&config, 1).unwrap();
    let draws = draws_by_year(&result);
    assert_eq!(draws.len(), 10);
    // At a zero return the payment is P / years left: level here.
    assert!(
        draws.iter().all(|(_, w)| close(*w, 100_000.0, 0.01)),
        "{draws:?}"
    );
    assert!(final_net_worth(&result).abs() < 1.0);
}

#[test]
fn vpw_with_a_return_draws_the_annuity_payment() {
    let config = with_spending(
        plan(1, 0.0, 0.0),
        SpendingRule::Vpw {
            real_return: 0.05,
            final_age: 90,
        },
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    let draws = draws_by_year(&simulate(&config, 1).unwrap());
    let expected = 1_000_000.0 * crate::model::pmt(0.05, 30);
    assert!(close(draws[0].1, expected, 0.01), "{draws:?} vs {expected}");
}

#[test]
fn capped_change_never_moves_more_than_its_caps() {
    let rule = SpendingRule::CappedChange {
        rate: 0.05,
        max_cut: 0.025,
        max_raise: 0.05,
    };
    let mut config = with_spending(
        plan(8, 0.15, 0.02),
        rule,
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    // A boom, then a crash: both caps bind.
    crash(&mut config, date(2030, 6, 1), 0.6);
    let result = simulate(&config, 1).unwrap();
    let diagnostics = result.diagnostics.spending.clone().unwrap();
    assert!(close(diagnostics.first_real, 50_000.0, 0.01));

    let draws = draws_by_year(&result);
    let real: Vec<f64> = draws
        .iter()
        .map(|(year, w)| w / 1.02_f64.powi(i32::from(year - 2026)))
        .collect();
    let mut raised = false;
    let mut cut = false;
    for pair in real.windows(2) {
        let change = pair[1] / pair[0] - 1.0;
        assert!((-0.025 - 1e-9..=0.05 + 1e-9).contains(&change), "{change}");
        raised |= close(change, 0.05, 1e-9);
        cut |= close(change, -0.025, 1e-9);
    }
    assert!(raised && cut, "{real:?}");
}

// ── Guardrails ──────────────────────────────────────────────────────────

/// $50,000 from $1M (5%), flat markets, a 50% crash in mid-2028.
fn crashed(protect_final_years: u8) -> SimulationResult {
    let mut config = with_spending(
        plan(6, 0.0, 0.0),
        guardrails_with(50_000.0, 0.20, protect_final_years),
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    crash(&mut config, date(2028, 6, 1), 0.5);
    simulate(&config, 1).unwrap()
}

#[test]
fn capital_preservation_cuts_by_step_once_a_year() {
    let result = crashed(0);
    let draws = draws_by_year(&result);
    // 2026-2028 inside the band (5.0%, 5.3%, 5.6% against 4%-6%).
    for (_, w) in &draws[..3] {
        assert!(close(*w, 50_000.0, 0.01), "{draws:?}");
    }
    // 2029: $50,000 of $425,000 is 11.8%: one cut of 10%, not repeated to
    // get back inside the band.
    assert!(close(draws[3].1, 45_000.0, 0.01), "{draws:?}");
    // 2030: still above the band, cut again from last year's draw.
    assert!(close(draws[4].1, 40_500.0, 0.01), "{draws:?}");

    let diagnostics = result.diagnostics.spending.unwrap();
    assert!(close(diagnostics.first_real, 50_000.0, 0.01));
    assert!(close(diagnostics.largest_cut, 0.10, 1e-9));
    assert!(diagnostics.lowest_real < 45_000.0);
    // 2030 on is below $45,000 (90% of the first draw).
    assert!(diagnostics.years_below_first >= 2);
}

#[test]
fn nothing_is_cut_within_the_protected_final_years() {
    let draws = draws_by_year(&crashed(15));
    assert!(
        draws.iter().all(|(_, w)| close(*w, 50_000.0, 0.01)),
        "{draws:?}"
    );
}

#[test]
fn prosperity_raises_after_a_boom() {
    let config = with_spending(
        plan(6, 0.25, 0.0),
        guardrails(50_000.0),
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    let draws = draws_by_year(&simulate(&config, 1).unwrap());
    let raises: Vec<f64> = draws.windows(2).map(|p| p[1].1 / p[0].1).collect();
    assert!(raises.iter().any(|r| close(*r, 1.10, 1e-9)), "{draws:?}");
    assert!(raises.iter().all(|r| *r <= 1.10 + 1e-9), "{draws:?}");
}

/// Two years at 3% inflation with a 10% drop in the first. `deposit` lands
/// in the brokerage before the drop, so the portfolio can end up larger
/// despite the loss.
fn after_a_loss(deposit: f64) -> Vec<(i16, f64)> {
    let mut config = with_spending(
        plan(2, 0.0, 0.03),
        guardrails_with(50_000.0, 0.99, 0),
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    crash(&mut config, date(2026, 6, 1), 0.10);
    if deposit > 0.0 {
        add_event(
            &mut config,
            4,
            EventTrigger::Date(date(2026, 3, 1)),
            EventEffect::Income {
                to: BROKERAGE,
                amount: TransferAmount::fixed(deposit),
                amount_mode: AmountMode::Net,
                income_type: IncomeType::TaxFree,
            },
        );
    }
    draws_by_year(&simulate(&config, 1).unwrap())
}

#[test]
fn the_inflation_raise_is_skipped_after_a_loss_only_above_the_initial_rate() {
    // $50,000 × 1.03 of about $855,000 is 6.0%, above the 5% start: skipped.
    let skipped = after_a_loss(0.0);
    assert!(close(skipped[1].1, 50_000.0, 0.01), "{skipped:?}");

    // With $1M more deposited the rate is about 2.8%: the raise is made.
    let raised = after_a_loss(1_000_000.0);
    assert!(close(raised[1].1, 51_500.0, 0.01), "{raised:?}");
}

// ── Pass-through ────────────────────────────────────────────────────────

fn monthly_from(start: jiff::civil::Date) -> EventTrigger {
    repeating(EventId(0), RepeatInterval::Monthly, start)
}

/// A level $40,000 draw (guardrails that never trigger), monthly.
fn level_draw(pass_through: bool) -> SimulationConfig {
    with_spending(
        plan(1, 0.0, 0.0),
        guardrails_with(40_000.0, 0.99, 0),
        RepeatInterval::Monthly,
        date(2026, 1, 1),
        pass_through,
    )
}

fn income(config: &mut SimulationConfig, yearly: f64) {
    add_event(
        config,
        1,
        monthly_from(date(2026, 1, 1)),
        EventEffect::Income {
            to: CHECKING,
            amount: TransferAmount::fixed(yearly / 12.0),
            amount_mode: AmountMode::Net,
            income_type: IncomeType::TaxFree,
        },
    );
}

fn expense(config: &mut SimulationConfig, yearly: f64) {
    add_event(
        config,
        2,
        monthly_from(date(2026, 1, 1)),
        EventEffect::Expense {
            from: CHECKING,
            amount: TransferAmount::fixed(yearly / 12.0),
        },
    );
}

#[test]
fn pass_through_spends_income_on_top_of_the_draw() {
    let mut config = level_draw(true);
    income(&mut config, 30_000.0);
    let result = simulate(&config, 1).unwrap();
    assert!(close(expenses_in(&result, 2026), 70_000.0, 0.01));
    // The portfolio gave up the draw and no more.
    assert!(close(final_net_worth(&result), 960_000.0, 0.01));
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
}

#[test]
fn a_fixed_expense_through_the_same_account_comes_out_of_the_draw() {
    let mut config = level_draw(true);
    expense(&mut config, 20_000.0);
    let result = simulate(&config, 1).unwrap();
    assert!(close(draws_by_year(&result)[0].1, 20_000.0, 0.01));
    assert!(close(expenses_in(&result, 2026), 40_000.0, 0.01));
    assert!(close(final_net_worth(&result), 960_000.0, 0.01));
}

#[test]
fn fixed_costs_above_income_and_draw_fall_to_the_funding_policy() {
    let mut config = level_draw(true);
    expense(&mut config, 60_000.0);
    let result = simulate(&config, 1).unwrap();
    assert!(close(draws_by_year(&result)[0].1, 0.0, 0.01));
    assert!(close(final_net_worth(&result), 940_000.0, 0.01));
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);

    // With no policy, the deficit is a shortfall.
    config.funding = None;
    let result = simulate(&config, 1).unwrap();
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::CashShortfall)
    );
}

#[test]
fn without_pass_through_the_effect_spends_exactly_the_draw() {
    let mut config = level_draw(false);
    income(&mut config, 30_000.0);
    let result = simulate(&config, 1).unwrap();
    assert!(close(draws_by_year(&result)[0].1, 40_000.0, 0.01));
    assert!(close(expenses_in(&result, 2026), 40_000.0, 0.01));
    // The income stays in checking.
    assert!(close(final_net_worth(&result), 990_000.0, 0.01));
}

#[test]
fn pass_through_ignores_what_built_up_before_spending_started() {
    // Salary banks $10,000 a month all year; spending starts in December.
    let mut config = with_spending(
        plan(1, 0.0, 0.0),
        guardrails_with(12_000.0, 0.99, 0),
        RepeatInterval::Monthly,
        date(2026, 12, 1),
        true,
    );
    income(&mut config, 120_000.0);
    let result = simulate(&config, 1).unwrap();
    // December's $10,000 of salary arrives the same day and passes through;
    // the $110,000 saved before does not.
    assert!(close(draws_by_year(&result)[0].1, 11_000.0, 0.01));
}

// ── Cadence and state ───────────────────────────────────────────────────

#[test]
fn monthly_and_yearly_draw_the_same_amount_a_year() {
    let rule = SpendingRule::PercentOfPortfolio {
        rate: 0.04,
        floor: None,
        ceiling: None,
    };
    let yearly = with_spending(
        plan(3, 0.05, 0.02),
        rule.clone(),
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    let monthly = with_spending(
        plan(3, 0.05, 0.02),
        rule.clone(),
        RepeatInterval::Monthly,
        date(2026, 1, 1),
        false,
    );
    let quarterly = with_spending(
        plan(3, 0.05, 0.02),
        rule,
        RepeatInterval::Quarterly,
        date(2026, 1, 1),
        false,
    );
    let y = simulate(&yearly, 1).unwrap();
    let m = simulate(&monthly, 1).unwrap();
    let q = simulate(&quarterly, 1).unwrap();
    // The first year's draw is decided on the same day from the same balance.
    let (yd, md, qd) = (
        draws_by_year(&y)[0].1,
        draws_by_year(&m)[0].1,
        draws_by_year(&q)[0].1,
    );
    assert!(close(yd, 40_000.0, 0.01) && close(md, yd, 0.01) && close(qd, yd, 0.01));
    let first = |r: &SimulationResult| r.diagnostics.spending.as_ref().unwrap().first_real;
    assert!(close(first(&y), first(&m), 1e-6));
}

#[test]
fn a_july_start_pays_six_twelfths() {
    let config = with_spending(
        plan(2, 0.0, 0.0),
        SpendingRule::PercentOfPortfolio {
            rate: 0.04,
            floor: None,
            ceiling: None,
        },
        RepeatInterval::Monthly,
        date(2026, 7, 1),
        false,
    );
    let result = simulate(&config, 1).unwrap();
    let draws = draws_by_year(&result);
    assert!(close(draws[0].1, 20_000.0, 0.01), "{draws:?}");
    // The year's decision is the whole-year draw.
    assert!(close(
        result.diagnostics.spending.unwrap().first_real,
        40_000.0,
        0.01
    ));
}

#[test]
fn a_spending_effect_needs_a_supported_interval() {
    let config = with_spending(
        plan(1, 0.0, 0.0),
        guardrails(40_000.0),
        RepeatInterval::Weekly,
        date(2026, 1, 1),
        false,
    );
    let result = simulate(&config, 1).unwrap();
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::EvaluationFailed && w.event_id == Some(SPEND))
    );
}

/// Guardrails on a volatile market: the rule's memory matters.
fn stochastic() -> SimulationConfig {
    let mut config = with_spending(
        plan(15, 0.0, 0.0),
        guardrails(45_000.0),
        RepeatInterval::Monthly,
        date(2026, 1, 1),
        true,
    );
    config.return_profiles.insert(
        MARKET,
        ReturnProfile::Normal {
            mean: 0.06,
            std_dev: 0.18,
        },
    );
    config.inflation_profile = InflationProfile::Normal {
        mean: 0.03,
        std_dev: 0.01,
    };
    income(&mut config, 12_000.0);
    config
}

#[test]
fn a_reseeded_path_reproduces_its_draws() {
    let config = stochastic();
    let first = simulate(&config, 99).unwrap();
    // Other paths in between leave nothing behind.
    for seed in 0..5 {
        simulate(&config, seed).unwrap();
    }
    let again = simulate(&config, 99).unwrap();
    assert_eq!(draws_by_year(&first), draws_by_year(&again));
    assert_eq!(first.diagnostics, again.diagnostics);
    // And batches (no ledger) see the same path.
    let mut quiet = config.clone();
    quiet.collect_ledger = false;
    assert_eq!(simulate(&quiet, 99).unwrap().diagnostics, first.diagnostics);
}

// ── Results ─────────────────────────────────────────────────────────────

fn mc(iterations: usize, batches: usize) -> MonteCarloConfig {
    MonteCarloConfig {
        iterations,
        parallel_batches: batches,
        seed: Some(5),
        ..Default::default()
    }
}

/// Type-7 quantile, independently of the engine's.
fn type7(values: &mut [f64], p: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    let h = (values.len() - 1) as f64 * p;
    let (lo, hi) = (values[h.floor() as usize], values[h.ceil() as usize]);
    lo + (hi - lo) * h.fract()
}

#[test]
fn real_spending_and_stats_match_the_paths() {
    let config = stochastic();
    let mc_config = mc(60, 3);
    let prepared = prepare_run(&config, &mc_config).unwrap();
    let mut coordinator = MonteCarloCoordinator::new(&mc_config, 5).unwrap();
    let round: Vec<BatchSpec> = coordinator.next_round().unwrap();
    let outputs: Vec<_> = round
        .iter()
        .map(|spec| run_batch(&prepared, spec, None).unwrap())
        .collect();
    let seeds: Vec<u64> = outputs
        .iter()
        .flat_map(|o| o.results.iter().map(|(seed, _)| *seed))
        .collect();
    let summary = merge_batches(&config, &mc_config, outputs).unwrap();

    let paths: Vec<SimulationResult> = seeds
        .iter()
        .map(|seed| simulate(&config, *seed).unwrap())
        .collect();
    let real = summary.real_spending.as_ref().unwrap();
    assert_eq!(real.num_iterations, 60);
    assert_eq!(real.points.len(), 15);
    for (index, point) in real.points.iter().enumerate() {
        let mut column: Vec<f64> = paths
            .iter()
            .map(|p| p.diagnostics.real_spending_by_year[index])
            .collect();
        assert!(close(point.p50, type7(&mut column, 0.5), 1e-6));
        assert!(close(point.p5, type7(&mut column, 0.05), 1e-6));
        assert!(close(point.p95, type7(&mut column, 0.95), 1e-6));
        assert_eq!(point.date, date(2026 + index as i16, 12, 31));
    }

    // Total spending is the year's expense debits, deflated.
    let path = &paths[0];
    for (index, year) in (2026..2041).enumerate() {
        let nominal = expenses_in(path, year);
        let real = nominal / path.cumulative_inflation[index];
        assert!(
            close(path.diagnostics.real_spending_by_year[index], real, 1e-6),
            "{year}"
        );
    }

    let stats = summary.stats.spending.as_ref().unwrap();
    let mut lifetime: Vec<f64> = paths
        .iter()
        .map(|p| p.diagnostics.spending.as_ref().unwrap().lifetime_real)
        .collect();
    let mut lowest: Vec<f64> = paths
        .iter()
        .map(|p| p.diagnostics.spending.as_ref().unwrap().lowest_real)
        .collect();
    assert!(close(
        stats.median_lifetime_spending,
        type7(&mut lifetime, 0.5),
        1e-6
    ));
    assert!(close(stats.p10_lowest_draw, type7(&mut lowest, 0.10), 1e-6));
    let cuts = paths
        .iter()
        .filter(|p| p.diagnostics.spending.as_ref().unwrap().largest_cut > 0.10)
        .count();
    assert!(close(stats.cut_rate, cuts as f64 / 60.0, 1e-12));
}

#[test]
fn batches_merge_to_the_same_summary_through_a_worker() {
    let config = stochastic();
    let mc_config = mc(40, 4);
    let prepared = prepare_run(&config, &mc_config).unwrap();
    let mut coordinator = MonteCarloCoordinator::new(&mc_config, 5).unwrap();
    let round = coordinator.next_round().unwrap();
    let outputs: Vec<_> = round
        .iter()
        .map(|spec| run_batch(&prepared, spec, None).unwrap())
        .collect();
    let wired: Vec<_> = outputs
        .iter()
        .rev()
        .map(|o| serde_json::from_str(&serde_json::to_string(o).unwrap()).unwrap())
        .collect();
    let direct = merge_batches(&config, &mc_config, outputs).unwrap();
    let via_worker = merge_batches(&config, &mc_config, wired).unwrap();
    assert_eq!(
        serde_json::to_value(&direct.stats.spending).unwrap(),
        serde_json::to_value(&via_worker.stats.spending).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&direct.real_spending).unwrap(),
        serde_json::to_value(&via_worker.real_spending).unwrap()
    );
    // The in-process run agrees.
    let whole = monte_carlo_simulate_with_config(&config, &mc_config).unwrap();
    assert_eq!(
        serde_json::to_value(&whole.real_spending).unwrap(),
        serde_json::to_value(&direct.real_spending).unwrap()
    );
}

#[test]
fn a_plan_without_the_effect_serializes_as_before() {
    let mut config = plan(3, 0.0, 0.0);
    expense(&mut config, 40_000.0);
    let summary = monte_carlo_simulate_with_config(&config, &mc(10, 2)).unwrap();
    assert!(summary.real_spending.is_none() && summary.stats.spending.is_none());
    let value = serde_json::to_value(&summary).unwrap();
    assert!(value.get("real_spending").is_none());
    assert!(value["stats"].get("spending").is_none());
    let path = &summary.percentile_runs[0].1;
    let diagnostics = serde_json::to_value(&path.diagnostics).unwrap();
    assert!(diagnostics.get("spending").is_none());
    assert!(diagnostics.get("real_spending_by_year").is_none());
}

// ── Analysis ────────────────────────────────────────────────────────────

#[test]
fn a_rule_term_is_swept_where_the_rule_has_it() {
    use crate::analysis::{
        EffectParam, EffectTarget, SpendingField, SweepParameter, SweepTarget, apply_parameter,
    };
    let config = with_spending(
        plan(3, 0.0, 0.0),
        guardrails(40_000.0),
        RepeatInterval::Yearly,
        date(2026, 1, 1),
        false,
    );
    let sweep = |field| SweepParameter {
        event_id: SPEND,
        target: SweepTarget::Effect {
            param: EffectParam::SpendingRule(field),
            target: EffectTarget::FirstEligible,
        },
        min_value: 30_000.0,
        max_value: 50_000.0,
        step_count: 3,
    };
    let swept = apply_parameter(&config, &sweep(SpendingField::InitialAmount), 45_000.0).unwrap();
    let draws = draws_by_year(&simulate(&swept, 1).unwrap());
    assert!(close(draws[0].1, 45_000.0, 0.01), "{draws:?}");
    let floored = apply_parameter(&config, &sweep(SpendingField::Floor), 39_000.0).unwrap();
    let EventEffect::Spending {
        rule: SpendingRule::Guardrails { floor, .. },
        ..
    } = &floored.events.last().unwrap().effects[0]
    else {
        panic!("still guardrails");
    };
    assert_eq!(*floor, Some(39_000.0));
    // Capped change has no floor to sweep.
    assert!(
        SpendingField::Floor
            .apply(
                &SpendingRule::CappedChange {
                    rate: 0.05,
                    max_cut: 0.025,
                    max_raise: 0.05
                },
                1.0
            )
            .is_err()
    );
}

#[test]
fn solve_finds_the_largest_starting_draw_above_a_lowest_draw_floor() {
    use crate::analysis::{
        EffectParam, EffectTarget, SolveConfig, SolveConstraint, SolveConstraintMetric,
        SolveObjective, SpendingField, SweepParameter, SweepTarget, solve,
    };
    let mut config = stochastic();
    config.duration_years = 12;
    let parameter = SweepParameter {
        event_id: SPEND,
        target: SweepTarget::Effect {
            param: EffectParam::SpendingRule(SpendingField::InitialAmount),
            target: EffectTarget::FirstEligible,
        },
        min_value: 20_000.0,
        max_value: 120_000.0,
        step_count: 2,
    };
    let floor = 25_000.0;
    let solve_config = SolveConfig {
        parameters: vec![parameter],
        objective: SolveObjective::MaxParameter,
        constraint: SolveConstraint {
            metric: SolveConstraintMetric::FundingSuccessRate,
            min_value: 0.9,
        },
        also: vec![SolveConstraint {
            metric: SolveConstraintMetric::LowestDrawP10 { min_real: floor },
            min_value: 0.0,
        }],
        mc_iterations: 40,
        parallel_batches: 2,
        seed: Some(3),
        tolerance: 500.0,
        max_probes: 12,
    };
    let results = solve(&config, &solve_config, None).unwrap();
    let best = results.best.clone().unwrap_or_else(|| {
        panic!(
            "a starting draw clears both: {:?}",
            results
                .probes
                .iter()
                .map(|p| (
                    p.values[0],
                    p.funding_success_rate,
                    p.spending.as_ref().map(|s| s.p10_lowest_draw)
                ))
                .collect::<Vec<_>>()
        )
    });
    assert!(results.constraint_std_error().is_some());
    let spending = best.spending.as_ref().expect("spending stats on the probe");
    assert!(spending.p10_lowest_draw >= floor);
    assert!(best.funding_success_rate.unwrap() >= 0.9);
    // Every probe called feasible clears both bars, and nothing above the
    // answer clears funding.
    for probe in &results.probes {
        let clears = probe.funding_success_rate.is_some_and(|r| r >= 0.9)
            && probe
                .spending
                .as_ref()
                .is_some_and(|s| s.p10_lowest_draw >= floor);
        assert_eq!(probe.feasible, clears, "{:?}", probe.values);
        if probe.values[0] > best.values[0] {
            assert!(
                probe.funding_success_rate.unwrap() < 0.9,
                "{:?}",
                probe.values
            );
        }
    }

    // A floor above what the largest fundable draw reaches has no answer.
    let mut too_high = solve_config.clone();
    too_high.also[0].metric = SolveConstraintMetric::LowestDrawP10 {
        min_real: 200_000.0,
    };
    assert!(solve(&config, &too_high, None).unwrap().best.is_none());
}
