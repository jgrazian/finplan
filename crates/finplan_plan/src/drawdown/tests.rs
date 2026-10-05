//! The projection against the anonymized default plan (never a personal
//! fixture).

use finplan_core::analysis::{McRunner, StatsRun};
use finplan_core::config::SimulationConfig;
use finplan_core::error::SimulationError;
use finplan_core::model::{
    AccountId, AmountMode, Event, EventEffect, EventId, EventTrigger, IncomeType, LotMethod,
    MonteCarloConfig, MonteCarloProgress, MonteCarloSummary, TransferAmount, WithdrawalOrder,
    WithdrawalSources,
};
use finplan_core::simulation::{monte_carlo_simulate_with_progress, monte_carlo_stats_only};
use serde_json::json;

use super::normalize::{has_fixed_sweep, set_order};
use super::*;
use crate::edit::{self, EditOp};
use crate::graph::ScenarioGraph;
use crate::specs::scenarios::SetFunding;

const SEED: u64 = 7;

fn default_graph() -> ScenarioGraph {
    serde_json::from_str(include_str!("../../testdata/default_snapshot.json")).unwrap()
}

fn make(graph: &mut ScenarioGraph, op: serde_json::Value) -> i64 {
    let op: EditOp = serde_json::from_value(op).unwrap();
    edit::apply(graph, &op).unwrap().id.unwrap_or_default()
}

fn funding(graph: &mut ScenarioGraph, strategy: &str) {
    let body: SetFunding =
        serde_json::from_value(json!({"funding": {"strategy": strategy}})).unwrap();
    edit::set_funding(graph, &body).unwrap();
}

/// Retired at once, 76, with a pre-tax account that owes distributions.
fn rmd_graph() -> ScenarioGraph {
    let mut graph = default_graph();
    graph.scenario.duration_years = 12;
    make(
        &mut graph,
        json!({"op": "update_scenario", "body": {"birth_date": "1950-01-01"}}),
    );
    make(
        &mut graph,
        json!({"op": "create_event", "body": {
            "name": "RMD", "enabled": true,
            "trigger": {"kind": "Repeating", "interval": "Yearly"},
            "effects": [{"kind": "ApplyRmd", "to_account_id": 6}]}}),
    );
    graph
}

/// The default plan with the event that sells investments when cash runs low
/// taken out, so cash alone pays for retirement.
fn dry_graph() -> ScenarioGraph {
    let mut graph = default_graph();
    graph.scenario.duration_years = 20;
    make(&mut graph, json!({"op": "delete_event", "id": 6}));
    graph
}

fn assert_balanced(body: &DrawdownBody) {
    for choice in &body.choices {
        assert!(!choice.years.is_empty());
        for y in &choice.years {
            let inflow: f64 = y.income.iter().sum::<f64>()
                + y.withdrawals.iter().sum::<f64>()
                + y.cash
                + y.shortfall;
            let outflow = y.spending + y.withdrawal_taxes + y.conversion_tax + y.surplus;
            assert!(
                (inflow - outflow).abs() <= 1.0,
                "{:?} {}: {inflow} vs {outflow}",
                choice.choice,
                y.year
            );
            assert!(y.cash >= 0.0 && y.surplus >= 0.0 && y.shortfall >= 0.0);
            assert!(y.cash == 0.0 || y.surplus == 0.0);
            assert_eq!(y.withdrawals.len(), body.accounts.len());
            assert_eq!(y.balances.len(), body.accounts.len());
            assert_eq!(y.income.len(), body.income_sources.len());
            for (rmd, withdrawn) in y.rmd.iter().zip(&y.withdrawals) {
                assert!(*rmd <= withdrawn + 0.01, "an RMD is part of a withdrawal");
            }
        }
    }
}

#[test]
fn the_default_list_runs_and_every_year_balances() {
    let body = project(&default_graph(), SEED, &DrawdownRequest::default()).unwrap();
    assert_eq!(body.choices.len(), 7);
    assert_eq!(body.choices[0].choice, StrategyChoice::AS_PLANNED);
    assert_eq!(body.seed, "7");
    assert_eq!(body.retirement.source, RetirementSource::Income);
    assert_eq!(
        body.retirement.year, 2036,
        "the year after the salary stops"
    );
    assert_eq!(body.accounts.len(), 6, "five investments and a bank");
    assert!(body.plan_funding.is_none());
    assert_balanced(&body);
    // Same seed, same answer.
    let again = project(&default_graph(), SEED, &DrawdownRequest::default()).unwrap();
    assert_eq!(
        serde_json::to_string(&body).unwrap(),
        serde_json::to_string(&again).unwrap()
    );
}

#[test]
fn a_strategy_is_an_overlay_only_when_the_plan_has_no_policy() {
    let request = DrawdownRequest::default();
    let body = project(&default_graph(), SEED, &request).unwrap();
    assert!(!body.choices[0].overlay);
    assert!(body.choices[1..].iter().all(|c| c.overlay));

    let mut graph = default_graph();
    funding(&mut graph, "PenaltyAware");
    let body = project(&graph, SEED, &request).unwrap();
    assert!(body.choices.iter().all(|c| !c.overlay));
    assert_eq!(
        body.plan_funding.unwrap().strategy,
        crate::specs::WithdrawalStrategy::PenaltyAware
    );
    assert_balanced(&project(&graph, SEED, &request).unwrap());
}

#[test]
fn strategies_sell_from_different_accounts() {
    let body = project(&default_graph(), SEED, &DrawdownRequest::default()).unwrap();
    let first_seller = |strategy: crate::specs::WithdrawalStrategy| {
        let choice = body
            .choices
            .iter()
            .find(|c| matches!(&c.choice, StrategyChoice::Strategy { strategy: s, .. } if *s == strategy))
            .unwrap();
        let year = choice
            .years
            .iter()
            .find(|y| y.withdrawals.iter().any(|w| *w > 1.0))
            .expect("sells something");
        let (i, _) = year
            .withdrawals
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        body.accounts[i].tax_status.clone().unwrap()
    };
    use crate::specs::WithdrawalStrategy::*;
    assert_eq!(first_seller(TaxEfficientEarly), "Taxable");
    assert_eq!(first_seller(TaxDeferredFirst), "TaxDeferred");
    assert_eq!(first_seller(TaxFreeFirst), "TaxFree");
    // Tax-free first pays no tax on the way out.
    let tax = |s: crate::specs::WithdrawalStrategy| {
        body.choices
            .iter()
            .find(|c| matches!(&c.choice, StrategyChoice::Strategy { strategy, .. } if *strategy == s))
            .unwrap()
            .summary
            .lifetime_tax
    };
    assert!(tax(TaxFreeFirst) < tax(TaxDeferredFirst));
}

#[test]
fn as_planned_shows_the_cash_running_out_and_a_strategy_covers_it() {
    let body = project(&dry_graph(), SEED, &DrawdownRequest::default()).unwrap();
    assert_balanced(&body);
    let planned = &body.choices[0];
    let first = planned.summary.first_shortfall_year.expect("cash runs out");
    assert!(
        planned
            .years
            .iter()
            .find(|y| y.year == first)
            .unwrap()
            .shortfall
            > 1.0
    );
    for covered in &body.choices[1..] {
        assert!(covered.overlay);
        assert_eq!(
            covered.summary.first_shortfall_year, None,
            "{:?}",
            covered.choice
        );
        assert!(
            covered
                .years
                .iter()
                .any(|y| y.withdrawals.iter().sum::<f64>() > 1.0)
        );
    }
}

#[test]
fn required_distributions_are_marked_and_the_excess_is_surplus() {
    let body = project(&rmd_graph(), SEED, &DrawdownRequest::default()).unwrap();
    assert_balanced(&body);
    let deferred = body
        .accounts
        .iter()
        .find(|a| a.tax_status.as_deref() == Some("TaxDeferred"))
        .unwrap()
        .id;
    for choice in &body.choices {
        let marker = choice
            .summary
            .markers
            .iter()
            .find(|m| m.kind == MarkerKind::Rmd && m.id == deferred)
            .unwrap_or_else(|| panic!("{:?} has an RMD marker", choice.choice));
        let i = body.accounts.iter().position(|a| a.id == deferred).unwrap();
        let year = choice.years.iter().find(|y| y.year == marker.year).unwrap();
        assert!(year.rmd[i] > 1.0);
    }
    assert!(
        body.choices[0].years.iter().any(|y| y.surplus > 1.0),
        "a distribution larger than spending leaves a surplus"
    );
}

#[test]
fn retirement_comes_from_the_request_then_the_parameter_then_the_income() {
    let graph = default_graph();
    let asked = DrawdownRequest {
        retirement_year: Some(2050),
        strategies: Some(vec![StrategyChoice::AS_PLANNED]),
    };
    let body = project(&graph, SEED, &asked).unwrap();
    assert_eq!(body.retirement.source, RetirementSource::Request);
    assert_eq!(body.retirement.year, 2050);
    assert_eq!(body.choices[0].years[0].year, 2050);
    assert_eq!(body.choices[0].years.last().unwrap().year, 2096);

    let mut graph = default_graph();
    make(
        &mut graph,
        json!({"op": "create_parameter", "body": {
            "name": "Retirement age", "value": {"kind": "Age", "years": 50, "months": 6}}}),
    );
    let body = project(&graph, SEED, &DrawdownRequest::default()).unwrap();
    assert_eq!(body.retirement.source, RetirementSource::Parameter);
    assert_eq!(
        body.retirement.year, 2046,
        "born 1996-01-01, 50 and 6 months"
    );
    assert_eq!(body.retirement.age, Some(50.5));
    assert_eq!(body.birth_year, Some(1996));

    // No salary stops, no birth date, no parameter: the first year.
    let mut plain = default_graph();
    make(&mut plain, json!({"op": "delete_event", "id": 1}));
    let body = project(&plain, SEED, &DrawdownRequest::default()).unwrap();
    assert_eq!(body.retirement.source, RetirementSource::Start);
    assert_eq!(body.retirement.year, 2026);
}

#[test]
fn a_bad_request_is_refused() {
    let graph = default_graph();
    let refused = |request: DrawdownRequest| project(&graph, SEED, &request).unwrap_err();
    refused(DrawdownRequest {
        strategies: Some(vec![]),
        ..Default::default()
    });
    refused(DrawdownRequest {
        strategies: Some(vec![StrategyChoice::AS_PLANNED; MAX_CHOICES + 1]),
        ..Default::default()
    });
    refused(DrawdownRequest {
        strategies: Some(vec![StrategyChoice::Strategy {
            strategy: crate::specs::WithdrawalStrategy::ProRata,
            bracket_ceiling: Some(0.12),
            conversion: None,
        }]),
        ..Default::default()
    });
    refused(DrawdownRequest {
        retirement_year: Some(12),
        ..Default::default()
    });
    for ceiling_rate in [0.0, 1.0, f64::NAN] {
        refused(DrawdownRequest {
            strategies: Some(vec![StrategyChoice::AsPlanned {
                conversion: Some(ConversionChoice::UpTo { ceiling_rate }),
            }]),
            ..Default::default()
        });
    }
}

fn sweep(sources: WithdrawalSources) -> EventEffect {
    EventEffect::Sweep {
        sources,
        to: AccountId(0),
        amount: TransferAmount::fixed(10.0),
        amount_mode: AmountMode::Net,
        lot_method: LotMethod::Fifo,
        income_type: IncomeType::Taxable,
    }
}

fn strategy_order(effect: &EventEffect) -> Option<WithdrawalOrder> {
    match effect {
        EventEffect::Sweep {
            sources: WithdrawalSources::Strategy { order, .. },
            ..
        } => Some(*order),
        _ => None,
    }
}

#[test]
fn normalizing_reaches_nested_sweeps_and_leaves_fixed_ones() {
    let by_strategy = sweep(WithdrawalSources::Strategy {
        order: WithdrawalOrder::TaxEfficientEarly,
        exclude_accounts: vec![AccountId(1)],
    });
    let mut nested = EventEffect::Random {
        probability: 0.5,
        on_true: Box::new(by_strategy.clone()),
        on_false: Some(Box::new(EventEffect::Random {
            probability: 0.5,
            on_true: Box::new(by_strategy),
            on_false: None,
        })),
    };
    set_order(&mut nested, WithdrawalOrder::ProRata);
    let EventEffect::Random {
        on_true, on_false, ..
    } = &nested
    else {
        unreachable!()
    };
    assert!(matches!(
        strategy_order(on_true),
        Some(WithdrawalOrder::ProRata)
    ));
    let Some(inner) = on_false.as_deref() else {
        unreachable!()
    };
    let EventEffect::Random { on_true, .. } = inner else {
        unreachable!()
    };
    assert!(matches!(
        strategy_order(on_true),
        Some(WithdrawalOrder::ProRata)
    ));
    // Its exclusions stay.
    assert!(matches!(
        &**on_true,
        EventEffect::Sweep { sources: WithdrawalSources::Strategy { exclude_accounts, .. }, .. }
            if exclude_accounts == &vec![AccountId(1)]
    ));

    let mut fixed = sweep(WithdrawalSources::SingleAccount(AccountId(1)));
    set_order(&mut fixed, WithdrawalOrder::ProRata);
    assert!(has_fixed_sweep(&fixed));
    assert!(!has_fixed_sweep(&nested));
    assert!(has_fixed_sweep(&EventEffect::Random {
        probability: 0.1,
        on_true: Box::new(nested),
        on_false: Some(Box::new(fixed)),
    }));
}

#[test]
fn normalizing_a_config_installs_or_retargets_the_policy() {
    let mut config = SimulationConfig::default();
    config.events.push(Event {
        event_id: EventId(0),
        trigger: EventTrigger::Manual,
        effects: vec![sweep(WithdrawalSources::Strategy {
            order: WithdrawalOrder::TaxEfficientEarly,
            exclude_accounts: vec![],
        })],
        once: false,
    });
    let when = jiff::civil::date(2040, 1, 1);
    let choice = StrategyChoice::Strategy {
        strategy: crate::specs::WithdrawalStrategy::TaxFreeFirst,
        bracket_ceiling: None,
        conversion: None,
    };

    let (same, overlay) = normalize(&config, &StrategyChoice::AS_PLANNED, when).unwrap();
    assert!(!overlay && same.funding.is_none());
    assert!(matches!(
        strategy_order(&same.events[0].effects[0]),
        Some(WithdrawalOrder::TaxEfficientEarly)
    ));

    let (installed, overlay) = normalize(&config, &choice, when).unwrap();
    assert!(overlay);
    let policy = installed.funding.unwrap();
    assert!(matches!(policy.order, WithdrawalOrder::TaxFreeFirst));
    assert_eq!(policy.from, Some(when));
    assert!(matches!(
        strategy_order(&installed.events[0].effects[0]),
        Some(WithdrawalOrder::TaxFreeFirst)
    ));

    config.funding = Some(finplan_core::model::FundingPolicy {
        order: WithdrawalOrder::ProRata,
        exclude_accounts: vec![AccountId(3)],
        from: None,
    });
    let (retargeted, overlay) = normalize(&config, &choice, when).unwrap();
    assert!(!overlay);
    let policy = retargeted.funding.unwrap();
    assert!(matches!(policy.order, WithdrawalOrder::TaxFreeFirst));
    assert_eq!(policy.exclude_accounts, vec![AccountId(3)]);
    assert_eq!(policy.from, None, "the plan's own start is kept");
}

#[test]
fn sweeps_that_name_their_source_are_listed() {
    let mut graph = default_graph();
    assert!(
        project(&graph, SEED, &DrawdownRequest::default())
            .unwrap()
            .fixed_sweeps
            .is_empty()
    );
    // Point the Home Purchase sweep (effect 4) at one account.
    graph.withdrawal_sources.get_mut(&4).unwrap().mode = "SingleAccount".into();
    graph.withdrawal_sources.get_mut(&4).unwrap().account_id = Some(1);
    let body = project(&graph, SEED, &DrawdownRequest::default()).unwrap();
    assert_eq!(body.fixed_sweeps.len(), 1);
    assert_eq!(body.fixed_sweeps[0].event_id, 5);
    assert_eq!(body.fixed_sweeps[0].name, "Home Purchase");
}

/// A sequential, uncounted runner.
struct Sequential;

impl McRunner for Sequential {
    fn stats(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<StatsRun, SimulationError> {
        monte_carlo_stats_only(config, mc, &MonteCarloProgress::default())
    }
    fn summary(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<MonteCarloSummary, SimulationError> {
        monte_carlo_simulate_with_progress(config, mc, &MonteCarloProgress::default())
    }
    fn cancelled(&self) -> bool {
        false
    }
}

#[test]
fn comparing_runs_every_strategy_on_the_same_markets() {
    let mut graph = dry_graph();
    graph.scenario.duration_years = 15;
    let body = CompareRequest {
        request: Some(DrawdownRequest::default()),
        iterations: Some(25),
    };
    let a = compare(&graph, &body, &mut Sequential).unwrap();
    assert_eq!(a.iterations, 25);
    assert_eq!(a.rows.len(), 7);
    assert!(!a.rows[0].overlay && a.rows[1..].iter().all(|r| r.overlay));
    assert!(a.rows.iter().all(|r| r.median_path_tax.is_some()
        && r.median_final_net_worth_real.is_some()
        && r.funding_success_rate.is_some()));
    // Cash alone cannot pay, selling investments can.
    assert!(a.rows[0].funding_success_rate < a.rows[1].funding_success_rate);
    // Tax-free first sells no taxable gains or pre-tax income.
    let tax = |i: usize| a.rows[i].median_path_tax.unwrap();
    assert!(tax(3) <= tax(2), "tax-free first <= tax-deferred first");

    let b = compare(&graph, &body, &mut Sequential).unwrap();
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap(),
        "common random numbers: the same answer every time"
    );

    let too_many = CompareRequest {
        iterations: Some(MAX_COMPARE_ITERATIONS + 1),
        ..Default::default()
    };
    assert!(compare(&graph, &too_many, &mut Sequential).is_err());
    let too_few = CompareRequest {
        iterations: Some(3),
        ..Default::default()
    };
    assert!(compare(&graph, &too_few, &mut Sequential).is_err());
}

#[test]
fn a_cancelled_runner_stops_the_comparison() {
    struct Stopped;
    impl McRunner for Stopped {
        fn stats(
            &mut self,
            _: &SimulationConfig,
            _: &MonteCarloConfig,
        ) -> Result<StatsRun, SimulationError> {
            Err(SimulationError::Cancelled)
        }
        fn summary(
            &mut self,
            _: &SimulationConfig,
            _: &MonteCarloConfig,
        ) -> Result<MonteCarloSummary, SimulationError> {
            Err(SimulationError::Cancelled)
        }
        fn cancelled(&self) -> bool {
            true
        }
    }
    let out = compare(&default_graph(), &CompareRequest::default(), &mut Stopped);
    assert!(out.unwrap_err().is_cancelled());
}

#[test]
fn a_distribution_is_counted_gross_like_the_amount_required() {
    let compiled = crate::compile::compile(&rmd_graph()).unwrap();
    let result = finplan_core::simulation::simulate(&compiled.config, SEED).unwrap();
    let distributions: Vec<(f64, f64)> = result
        .ledger
        .iter()
        .filter_map(|entry| match entry.event {
            finplan_core::model::StateEvent::RmdWithdrawal {
                required_amount,
                actual_amount,
                ..
            } => Some((required_amount, actual_amount)),
            _ => None,
        })
        .collect();
    // The first falls due on a full account, which pays all of it: gross, not
    // net of the tax withheld on the sale. Later ones can find it drained.
    let (required, actual) = distributions[0];
    assert!(
        (required - actual).abs() < 1.0,
        "required {required}, distributed {actual}"
    );
    assert!(distributions.iter().all(|(r, a)| *a <= r + 1.0));
}

#[test]
fn cash_withdrawn_from_an_account_is_attributed_to_it() {
    // The 401(k) holds cash, which its first RMD draws before selling.
    let mut graph = rmd_graph();
    graph.investment.get_mut(&3).unwrap().cash_value = 60_000.0;
    let body = project(&graph, SEED, &DrawdownRequest::default()).unwrap();
    assert_balanced(&body);

    let compiled = crate::compile::compile(&graph).unwrap();
    let k401 = compiled.id_map.account(3).unwrap();
    let mut config = compiled.config.clone();
    config.collect_ledger = true;
    let result = finplan_core::simulation::simulate(&config, SEED).unwrap();
    let in_year = |year: i64, cash_only: bool| -> f64 {
        result
            .ledger
            .iter()
            .filter(|e| i64::from(e.date.year()) == year)
            .filter_map(|e| match e.event {
                finplan_core::model::StateEvent::CashWithdrawal { account_id, amount }
                    if account_id == k401 =>
                {
                    Some(amount)
                }
                finplan_core::model::StateEvent::AssetSale {
                    account_id,
                    proceeds,
                    ..
                } if account_id == k401 && !cash_only => Some(proceeds),
                _ => None,
            })
            .sum()
    };

    let i = body.accounts.iter().position(|a| a.id == 3).unwrap();
    let planned = &body.choices[0];
    let first_rmd = planned.years.iter().find(|y| y.rmd[i] > 1.0).unwrap();
    assert!(in_year(first_rmd.year, true) > 1.0, "the RMD drew cash");
    assert!(
        (first_rmd.withdrawals[i] - in_year(first_rmd.year, false)).abs() < 0.01,
        "{} vs {}",
        first_rmd.withdrawals[i],
        in_year(first_rmd.year, false)
    );
}

fn set_deferred_tax_rate(graph: &mut ScenarioGraph, rate: f64) {
    make(
        graph,
        json!({"op": "update_scenario", "body": {"deferred_tax_rate": rate}}),
    );
}

#[test]
fn the_summary_values_tax_deferred_money_after_tax() {
    let graph = default_graph();
    assert_eq!(graph.scenario.deferred_tax_rate, 0.24);
    let body = project(&graph, SEED, &DrawdownRequest::default()).unwrap();
    let deferred: Vec<usize> = body
        .accounts
        .iter()
        .enumerate()
        .filter(|(_, a)| a.tax_status.as_deref() == Some("TaxDeferred"))
        .map(|(i, _)| i)
        .collect();
    assert!(!deferred.is_empty(), "the default plan has a 401(k)");
    let mut left_some = false;
    for choice in &body.choices {
        let s = &choice.summary;
        let last = choice.years.last().unwrap();
        let pre_tax: f64 = deferred.iter().map(|i| last.balances[*i]).sum();
        left_some |= pre_tax > 1.0;
        assert!(
            (s.after_tax_ending_balance - (s.ending_balance - 0.24 * pre_tax)).abs() < 1.0,
            "{:?}",
            choice.choice
        );
        assert!(
            (s.after_tax_ending_balance_real - s.after_tax_ending_balance / last.inflation).abs()
                < 1e-6
        );
    }
    assert!(left_some, "some strategy ends with pre-tax money");

    // At 0% a tax-deferred dollar is a dollar.
    let mut untaxed = default_graph();
    set_deferred_tax_rate(&mut untaxed, 0.0);
    let body = project(&untaxed, SEED, &DrawdownRequest::default()).unwrap();
    for choice in &body.choices {
        assert_eq!(
            choice.summary.after_tax_ending_balance,
            choice.summary.ending_balance
        );
    }
}

#[test]
fn comparison_rows_carry_the_after_tax_median() {
    let mut graph = dry_graph();
    graph.scenario.duration_years = 15;
    let body = CompareRequest {
        request: Some(DrawdownRequest {
            strategies: Some(vec![
                StrategyChoice::AS_PLANNED,
                StrategyChoice::Strategy {
                    strategy: crate::specs::WithdrawalStrategy::TaxDeferredFirst,
                    bracket_ceiling: None,
                    conversion: None,
                },
            ]),
            retirement_year: None,
        }),
        iterations: Some(25),
    };
    let taxed = compare(&graph, &body, &mut Sequential).unwrap();
    // As planned sells nothing, so its 401(k) is all there at the end.
    assert!(taxed.rows[0].median_after_tax_ending_balance < taxed.rows[0].median_final_net_worth);
    for row in &taxed.rows {
        assert!(row.median_after_tax_ending_balance <= row.median_final_net_worth);
        let real = row.median_after_tax_ending_balance_real.unwrap();
        let factor = row.median_final_net_worth / row.median_final_net_worth_real.unwrap();
        assert!((real * factor - row.median_after_tax_ending_balance).abs() < 1e-3);
    }

    set_deferred_tax_rate(&mut graph, 0.0);
    let untaxed = compare(&graph, &body, &mut Sequential).unwrap();
    for (row, before) in untaxed.rows.iter().zip(&taxed.rows) {
        assert_eq!(
            row.median_after_tax_ending_balance,
            row.median_final_net_worth
        );
        // The rate values the end of a run; it never changes how one plays.
        assert_eq!(row.median_final_net_worth, before.median_final_net_worth);
        assert_eq!(row.success_rate, before.success_rate);
    }
}

#[test]
fn a_conversion_is_not_a_withdrawal() {
    // Nothing sells investments in the dry plan, so any 401(k) withdrawal
    // would be the conversion miscounted. The tax is paid from the bank.
    let mut graph = dry_graph();
    make(
        &mut graph,
        json!({"op": "create_event", "body": {
            "name": "Roth conversions",
            "trigger": {"kind": "Repeating", "interval": "Yearly",
                        "start_condition": {"kind": "Date", "on_date": "2027-12-30"}},
            "effects": [{"kind": "RothConversion", "from_account_id": 3, "to_account_id": 2,
                         "amount": {"kind": "Fixed", "value": 20000.0},
                         "pay_tax_from_account_id": 6}]}}),
    );
    let converting = project(&graph, SEED, &DrawdownRequest::default()).unwrap();
    let plain = project(&dry_graph(), SEED, &DrawdownRequest::default()).unwrap();
    assert_balanced(&converting);

    let at = |id: i64| converting.accounts.iter().position(|a| a.id == id).unwrap();
    let (k401, roth) = (at(3), at(2));
    let years = &converting.choices[0].years;
    assert!(years.iter().all(|y| y.withdrawals[k401] == 0.0));
    let before = plain.choices[0].years.last().unwrap();
    let after = years.last().unwrap();
    assert!(after.balances[k401] < before.balances[k401]);
    assert!(after.balances[roth] > before.balances[roth]);
}

// ── the conversion toggle (spec 21, part 3) ─────────────────────────────────

const UP_TO_22: ConversionChoice = ConversionChoice::UpTo { ceiling_rate: 0.22 };

/// As planned and two strategies, all with the toggle at `conversion`.
fn toggled(conversion: Option<ConversionChoice>) -> DrawdownRequest {
    use crate::specs::WithdrawalStrategy::*;
    DrawdownRequest {
        strategies: Some(vec![
            StrategyChoice::AsPlanned { conversion },
            StrategyChoice::Strategy {
                strategy: TaxEfficientEarly,
                bracket_ceiling: None,
                conversion,
            },
            StrategyChoice::Strategy {
                strategy: BracketFilling,
                bracket_ceiling: Some(0.12),
                conversion,
            },
        ]),
        retirement_year: None,
    }
}

/// The dry plan with its own conversions: $20k a year from the 401(k) into
/// the Vanguard Roth from 2027, the tax paid from `payer` or withheld. Returns
/// the plan and the event's id.
fn converting_graph(payer: Option<i64>) -> (ScenarioGraph, i64) {
    let mut graph = dry_graph();
    let id = make(
        &mut graph,
        json!({"op": "create_event", "body": {
            "name": "Roth conversions", "enabled": true, "fires_once": false,
            "trigger": {"kind": "Repeating", "interval": "Yearly",
                        "start_condition": {"kind": "Date", "on_date": "2027-12-30"}},
            "effects": [{"kind": "RothConversion", "from_account_id": 3, "to_account_id": 2,
                         "amount": {"kind": "Fixed", "value": 20000.0},
                         "pay_tax_from_account_id": payer}]}}),
    );
    (graph, id)
}

/// The path `project` folds for `choice`, with its ledger.
fn path_of(
    graph: &ScenarioGraph,
    choice: &StrategyChoice,
) -> finplan_core::model::SimulationResult {
    let compiled = crate::compile::compile(graph).unwrap();
    let mut base = None;
    let retirement = super::retirement::resolve(&compiled, None, SEED, &mut base).unwrap();
    let conversions = super::conversion::Conversions::prepare(
        graph,
        &compiled,
        &retirement,
        std::slice::from_ref(choice),
    )
    .unwrap();
    let (from, _) = conversions.base(&compiled, choice);
    let (mut config, _) = normalize(&from.config, choice, retirement.date).unwrap();
    config.collect_ledger = true;
    finplan_core::simulation::simulate(&config, SEED).unwrap()
}

/// Per year: the gross converted, and its tax with the penalties the
/// converting event paid, read off the ledger.
fn ledger_conversions(
    result: &finplan_core::model::SimulationResult,
) -> std::collections::BTreeMap<i64, (f64, f64)> {
    use finplan_core::model::StateEvent;
    let converting: std::collections::BTreeSet<_> = result
        .ledger
        .iter()
        .filter(|e| matches!(e.event, StateEvent::RothConversion { .. }))
        .filter_map(|e| e.source_event)
        .collect();
    let mut by_year = std::collections::BTreeMap::<i64, (f64, f64)>::new();
    for entry in &result.ledger {
        let year = by_year.entry(i64::from(entry.date.year())).or_default();
        match entry.event {
            StateEvent::RothConversion { amount, tax, .. } => {
                year.0 += amount;
                year.1 += tax;
            }
            StateEvent::EarlyWithdrawalPenalty { penalty_amount, .. }
                if entry.source_event.is_some_and(|e| converting.contains(&e)) =>
            {
                year.1 += penalty_amount;
            }
            _ => {}
        }
    }
    by_year
}

#[test]
fn every_year_balances_with_conversions_on() {
    // Tax paid from the bank, withheld (before 59½, so penalized), and the
    // overlay's, paid from the largest taxable account.
    let cases = [
        converting_graph(Some(6)).0,
        converting_graph(None).0,
        default_graph(),
    ];
    for graph in &cases {
        for conversion in [None, Some(UP_TO_22), Some(ConversionChoice::Off)] {
            let body = project(graph, SEED, &toggled(conversion)).unwrap();
            assert_balanced(&body);
            for choice in &body.choices {
                assert!(choice.years.iter().all(|y| y.conversion >= 0.0
                    && y.conversion_tax >= 0.0
                    && y.withdrawal_taxes >= 0.0));
                let converted: f64 = choice.years.iter().map(|y| y.conversion).sum();
                match conversion {
                    Some(ConversionChoice::Off) => assert_eq!(converted, 0.0),
                    Some(_) => assert!(converted > 1.0, "{:?}", choice.choice),
                    None => {}
                }
            }
        }
    }
}

#[test]
fn the_conversion_and_its_tax_line_up_with_the_ledger() {
    let as_planned = StrategyChoice::AS_PLANNED;
    let overlaid = StrategyChoice::AsPlanned {
        conversion: Some(UP_TO_22),
    };
    let cases = [
        (converting_graph(Some(6)).0, as_planned.clone()),
        (converting_graph(None).0, as_planned),
        (default_graph(), overlaid),
    ];
    for (i, (graph, choice)) in cases.iter().enumerate() {
        let request = DrawdownRequest {
            strategies: Some(vec![choice.clone()]),
            retirement_year: None,
        };
        let body = project(graph, SEED, &request).unwrap();
        let ledger = ledger_conversions(&path_of(graph, choice));
        let years = &body.choices[0].years;
        assert!(
            years.iter().any(|y| y.conversion > 1.0),
            "case {i} converts"
        );
        for y in years {
            let (gross, tax) = ledger.get(&y.year).copied().unwrap_or_default();
            assert!((y.conversion - gross).abs() < 0.01, "case {i} {}", y.year);
            assert!(
                (y.conversion_tax - tax).abs() < 0.01,
                "case {i} {}: {} vs {tax}",
                y.year,
                y.conversion_tax
            );
        }
    }

    // Withheld before 59½: the part kept back is a 401(k) distribution, and
    // its tax and penalty are conversion tax, not tax on withdrawals.
    let (graph, _) = converting_graph(None);
    let body = project(&graph, SEED, &DrawdownRequest::default()).unwrap();
    let k401 = body.accounts.iter().position(|a| a.id == 3).unwrap();
    let year = body.choices[0]
        .years
        .iter()
        .find(|y| y.conversion > 1.0)
        .unwrap();
    assert!(year.withdrawals[k401] > 1.0);
    assert!(
        (year.withdrawals[k401] - year.conversion_tax).abs() < 0.01,
        "withheld = tax / (1 - penalty rate) = tax + penalty"
    );
    assert!(year.withdrawal_taxes < 0.01, "nothing else sold that year");
}

#[test]
fn the_overlay_is_added_only_when_the_plan_has_no_conversion() {
    // The default plan has none: a rate adds the template's event.
    let body = project(&default_graph(), SEED, &toggled(Some(UP_TO_22))).unwrap();
    assert!(body.conversions.events.is_empty());
    assert_eq!(body.conversions.unavailable, None);
    let overlay = body.conversions.overlay.clone().unwrap();
    assert_eq!(
        overlay,
        ConversionOverlay {
            from_account_id: 3,
            to_account_id: 4,
            pay_tax_from_account_id: Some(1),
            start_year: 2036,
            until_age: 75,
        },
        "the largest 401(k) into the largest Roth, tax from the largest taxable \
         account, from retirement until RMDs (born 1996)"
    );
    for choice in &body.choices {
        assert!(choice.conversion_overlay);
        let years: Vec<i64> = choice
            .years
            .iter()
            .filter(|y| y.conversion > 1.0)
            .map(|y| y.year)
            .collect();
        assert_eq!(years.first(), Some(&2036));
        assert!(*years.last().unwrap() < 1996 + 75);
    }
    // As planned and None add nothing.
    for conversion in [None, Some(ConversionChoice::Off)] {
        let body = project(&default_graph(), SEED, &toggled(conversion)).unwrap();
        assert!(body.conversions.overlay.is_some());
        for choice in &body.choices {
            assert!(!choice.conversion_overlay);
            assert!(choice.years.iter().all(|y| y.conversion == 0.0));
        }
    }

    // A plan with its own: a rate retargets it, and nothing is added.
    let (graph, event) = converting_graph(Some(6));
    let body = project(&graph, SEED, &toggled(Some(UP_TO_22))).unwrap();
    assert_eq!(body.conversions.events, vec![event]);
    assert_eq!(body.conversions.overlay, None);
    let planned = project(&graph, SEED, &toggled(None)).unwrap();
    for (filled, fixed) in body.choices.iter().zip(&planned.choices) {
        assert!(!filled.conversion_overlay);
        let first = |c: &DrawdownChoice| c.years.iter().map(|y| y.conversion).find(|v| *v > 1.0);
        assert!((first(fixed).unwrap() - 20_000.0).abs() < 0.01);
        assert!(
            first(filled).unwrap() > 20_000.0,
            "the 22% bracket holds more than $20k"
        );
    }

    // The comparison says the same.
    let mut short = default_graph();
    short.scenario.duration_years = 15;
    let request = DrawdownRequest {
        strategies: Some(vec![
            StrategyChoice::AS_PLANNED,
            StrategyChoice::AsPlanned {
                conversion: Some(UP_TO_22),
            },
        ]),
        retirement_year: Some(2030),
    };
    let rows = compare(
        &short,
        &CompareRequest {
            request: Some(request),
            iterations: Some(25),
        },
        &mut Sequential,
    )
    .unwrap()
    .rows;
    assert!(!rows[0].conversion_overlay && rows[1].conversion_overlay);
}

#[test]
fn none_switches_the_plans_conversions_off() {
    let (graph, _) = converting_graph(Some(6));
    let off = project(&graph, SEED, &toggled(Some(ConversionChoice::Off))).unwrap();
    let planned = project(&graph, SEED, &toggled(None)).unwrap();
    let roth = off.accounts.iter().position(|a| a.id == 2).unwrap();
    for (off, planned) in off.choices.iter().zip(&planned.choices) {
        assert!(!off.conversion_overlay);
        assert!(
            off.years
                .iter()
                .all(|y| y.conversion == 0.0 && y.conversion_tax == 0.0)
        );
        assert!(planned.years.iter().any(|y| y.conversion > 1.0));
        let end = |c: &DrawdownChoice| c.years.last().unwrap().balances[roth];
        assert!(end(off) < end(planned), "{:?}", off.choice);
    }
}

#[test]
fn the_toggle_is_unavailable_without_a_roth_or_a_pre_tax_account() {
    let mut graph = default_graph();
    make(&mut graph, json!({"op": "delete_account", "id": 2}));
    make(&mut graph, json!({"op": "delete_account", "id": 4}));
    let body = project(&graph, SEED, &DrawdownRequest::default()).unwrap();
    let reason = body.conversions.unavailable.unwrap();
    assert!(reason.contains("Roth"), "{reason}");
    assert_eq!(body.conversions.overlay, None);
    assert!(project(&graph, SEED, &toggled(Some(UP_TO_22))).is_err());
    // None asks for nothing the plan lacks.
    assert_balanced(&project(&graph, SEED, &toggled(Some(ConversionChoice::Off))).unwrap());

    let mut graph = default_graph();
    make(&mut graph, json!({"op": "delete_account", "id": 3}));
    let body = project(&graph, SEED, &DrawdownRequest::default()).unwrap();
    assert!(
        body.conversions
            .unavailable
            .unwrap()
            .contains("tax-deferred")
    );
}

/// Retired at 60 with a large 401(k): the plan lives on its taxable accounts
/// with the low brackets empty, then its RMDs, from 75, come out at high
/// rates.
fn deferred_heavy_graph() -> ScenarioGraph {
    let mut graph = default_graph();
    graph.scenario.duration_years = 32;
    make(
        &mut graph,
        json!({"op": "update_scenario", "body": {"birth_date": "1966-01-01"}}),
    );
    make(&mut graph, json!({"op": "delete_event", "id": 1}));
    for lot in graph.positions.get_mut(&3).unwrap() {
        lot.units *= 8.0;
        lot.cost_basis *= 8.0;
    }
    make(
        &mut graph,
        json!({"op": "create_event", "body": {
            "name": "RMD", "enabled": true, "fires_once": false,
            "trigger": {"kind": "Repeating", "interval": "Yearly"},
            "effects": [{"kind": "ApplyRmd", "to_account_id": 6}]}}),
    );
    graph
}

#[test]
fn filling_low_brackets_ahead_of_rmds_beats_not_converting_after_tax() {
    let graph = deferred_heavy_graph();
    let request = DrawdownRequest {
        strategies: Some(vec![
            StrategyChoice::AsPlanned {
                conversion: Some(ConversionChoice::Off),
            },
            StrategyChoice::AsPlanned {
                conversion: Some(UP_TO_22),
            },
        ]),
        retirement_year: None,
    };
    let body = project(&graph, SEED, &request).unwrap();
    assert_balanced(&body);
    let (none, filled) = (&body.choices[0].summary, &body.choices[1].summary);
    assert!(body.choices[1].conversion_overlay);
    assert!(
        none.markers.iter().any(|m| m.kind == MarkerKind::Rmd),
        "RMDs fall due inside the plan"
    );
    assert!(
        filled.after_tax_ending_balance > none.after_tax_ending_balance,
        "{} vs {}",
        filled.after_tax_ending_balance,
        none.after_tax_ending_balance
    );

    let rows = compare(
        &graph,
        &CompareRequest {
            request: Some(request),
            iterations: Some(25),
        },
        &mut Sequential,
    )
    .unwrap()
    .rows;
    assert!(
        rows[1].median_after_tax_ending_balance > rows[0].median_after_tax_ending_balance,
        "{} vs {}",
        rows[1].median_after_tax_ending_balance,
        rows[0].median_after_tax_ending_balance
    );
}
