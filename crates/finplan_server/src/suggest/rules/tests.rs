//! The rules against the anonymized default snapshot (`../testdata/`) and a
//! hand-built median path shaped like the fixture run's: cash piling up in
//! USAA before retirement, a home bought at 35 (2031) with a sweep, an
//! inflation-adjusted mortgage payment paid off in 2047, and shortfalls that
//! concentrate in USAA.
//!
//! Ids: events 1 Salary, 5 Home Purchase, 6 Sweep, 7 Morgtage Paydown;
//! accounts 1 Vanguard, 2 Vanguard Roth IRA, 3 Fidelity 401(k), 4 Fidelity
//! Roth, 5 Robinhood, 6 USAA (bank), 7 Mortgage, 8 House; effect 9 is the
//! mortgage payment, whose amount 18 wraps Fixed amount 17.

use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::api::funding::{AccountCount, FundingDiagnostics, YearCount};
use crate::api::runs::{AccountSeries, Band, CashFlow, InflationPoint, Stats};
use crate::suggest::{ChangeOp, ChangeTarget, ResolvedChange, resolve};

const FIRST: i64 = 2026;
const LAST: i64 = 2095;

fn graph() -> ScenarioGraph {
    // Born 1996: Home Purchase (age 35) in 2031, Retirement (age 40) in 2036.
    serde_json::from_str(include_str!("../testdata/default_snapshot.json")).unwrap()
}

fn f(year: i64) -> f64 {
    1.035_f64.powi((year - FIRST) as i32)
}

fn usaa(year: i64) -> f64 {
    match year {
        2026 => 307e3,
        2027 => 457e3,
        2028 => 586e3,
        2029 => 730e3,
        2030 => 879e3,
        2031 => 900e3,
        2032 => 1.0e6,
        2033 => 1.1e6,
        2034 => 1.2e6,
        2035 => 1.3e6,
        2036 => 1.2e6,
        2037 => 970e3,
        2038 => 740e3,
        2039 => 510e3,
        2040 => 280e3,
        2041 => 50e3,
        _ => 25e3,
    }
}

fn balance(account: i64, year: i64) -> f64 {
    match account {
        1 => 2.2e6 * 1.07_f64.powi((year - FIRST) as i32),
        6 => usaa(year),
        7 if year >= 2031 => (-1.08e6 + 70e3 * (year - 2031) as f64).min(0.0),
        8 if year >= 2031 => 1.3e6,
        _ => 0.0,
    }
}

fn expenses(year: i64) -> f64 {
    match year {
        2026 => 32e3,
        2027..=2030 => 96e3 * f(year),
        2031 => 382e3,
        2032..=2035 => 200e3,
        _ => 250e3 * f(year),
    }
}

/// A run's results with one path, "0.5", shaped as the API serves it.
fn results() -> Results {
    // Plan start, each Dec 31, then the terminal date.
    let mut points: Vec<(String, i64)> = vec![("2026-09-03".into(), FIRST)];
    points.extend((FIRST..=LAST).map(|y| (format!("{y}-12-31"), y)));
    points.push(("2096-09-03".into(), LAST));
    let at = |account: i64, year: i64, date: &str| {
        if date == "2026-09-03" {
            if account == 6 {
                250e3
            } else {
                balance(account, FIRST)
            }
        } else {
            balance(account, year)
        }
    };
    let accounts = [1, 2, 3, 4, 5, 6, 7, 8];
    let account_series = accounts
        .iter()
        .map(|&id| AccountSeries {
            account_id: id,
            label: format!("account {id}"),
            values: points.iter().map(|(d, y)| at(id, *y, d)).collect(),
        })
        .collect();
    let net_worth = points
        .iter()
        .map(|(d, y)| accounts.iter().map(|&id| at(id, *y, d)).sum())
        .collect();
    Results {
        run_id: 1,
        scenario_id: 1,
        stats: Stats {
            num_iterations: 2000,
            success_rate: 0.909,
            funding_success_rate: Some(0.9055),
            mean_final_net_worth: 0.0,
            std_dev_final_net_worth: 0.0,
            min_final_net_worth: 0.0,
            max_final_net_worth: 0.0,
            lifetime_taxes: 0.0,
            converged: None,
            convergence_metric: None,
            convergence_value: None,
            percentile_values: Vec::new(),
        },
        bands: vec![Band {
            path_id: "0.5".into(),
            percentile: Some(0.5),
            dates: points.iter().map(|(d, _)| d.clone()).collect(),
            net_worth,
            inflation: points.iter().map(|(_, y)| f(*y)).collect(),
        }],
        real_net_worth: None,
        path_details: true,
        series_id: "0.5".into(),
        account_series,
        series_percentile: Some(0.5),
        cash_flows: (FIRST..=LAST)
            .map(|year| CashFlow {
                year,
                income: match year {
                    2026 => 72e3,
                    2027..=2035 => 205e3 * f(year),
                    _ => 0.0,
                },
                expenses: expenses(year),
                contributions: 0.0,
                withdrawals: match year {
                    2031 => 216e3,
                    2042.. => 250e3 * f(year),
                    _ => 0.0,
                },
                appreciation: 0.0,
                net_cash_flow: 0.0,
                taxes: 0.0,
            })
            .collect(),
        warnings: Vec::new(),
        inflation: (FIRST..=LAST)
            .map(|year| InflationPoint {
                year,
                factor: f(year),
            })
            .collect(),
        ledger_years: Vec::new(),
        funding_diagnostics: Some(FundingDiagnostics {
            iterations: 2000,
            failed: 189,
            cash_shortfall: 180,
            event_failure: 9,
            iteration_limit: 0,
            failed_solvent: 12,
            first_shortfall_years: vec![YearCount {
                year: 2074,
                count: 180,
            }],
            median_first_shortfall_year: Some(2074),
            shortfall_accounts: vec![
                AccountCount {
                    account_id: Some(6),
                    count: 170,
                },
                AccountCount {
                    account_id: Some(1),
                    count: 10,
                },
            ],
            event_failures: Vec::new(),
            liquid_depleted_years: Vec::new(),
            median_max_deficit: Some(38_200.0),
            median_shortfall_years: Some(6.0),
            worst_seed: Some("123".into()),
        }),
    }
}

fn set_usaa(results: &mut Results, year: i64, value: f64) {
    let date = format!("{year}-12-31");
    let i = results.bands[0]
        .dates
        .iter()
        .position(|d| *d == date)
        .unwrap();
    let series = results
        .account_series
        .iter_mut()
        .find(|s| s.account_id == 6)
        .unwrap();
    series.values[i] = value;
}

fn rules_of(drafts: &[Draft]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for d in drafts {
        *counts.entry(d.rule).or_default() += 1;
    }
    counts
}

fn only<'a>(drafts: &'a [Draft], rule: &str) -> &'a Draft {
    let found: Vec<_> = drafts.iter().filter(|d| d.rule == rule).collect();
    assert_eq!(found.len(), 1, "one {rule} draft, got {found:#?}");
    found[0]
}

/// Every change of every step of every path of `d`.
fn changes(d: &Draft) -> Vec<Change> {
    d.paths.iter().flat_map(|p| p.changes().cloned()).collect()
}

/// Every path's steps resolve in order against the graph the draft read.
fn assert_changes_resolve(graph: &ScenarioGraph, drafts: &[Draft]) {
    for draft in drafts {
        for path in &draft.paths {
            let steps: Vec<Vec<Change>> = path.steps.iter().map(|s| s.changes.clone()).collect();
            match crate::suggest::resolve_steps(graph, &steps, &Default::default()) {
                Ok(Ok(_)) => {}
                Ok(Err(failed)) => panic!(
                    "{} path {} step {} does not resolve: {:#?}",
                    draft.rule, path.key, failed.step, failed.problems
                ),
                Err(err) => panic!("{} path {}: {err}", draft.rule, path.key),
            }
        }
    }
}

#[test]
fn the_fixture_run_gets_the_expected_notes() {
    let drafts = review(&graph(), &results());
    assert_eq!(
        rules_of(&drafts),
        BTreeMap::from([
            ("idle_bank_cash", 1),
            ("liability_payment_inflation_adjusted", 1),
            ("shortfall_account_concentration", 1),
            ("sweep_sells_while_cash", 1),
            // GOOG single stock, VWO emerging, VFIFX target-date.
            ("unmapped_or_mismatched_assets", 3),
            ("unused_contribution_limits", 1),
        ])
    );
    assert_changes_resolve(&graph(), &drafts);
}

#[test]
fn drafts_are_ordered_by_severity_then_rule() {
    let drafts = review(&graph(), &results());
    let keys: Vec<_> = drafts.iter().map(|d| (d.kind, d.rule)).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert_eq!(drafts[0].kind, Kind::Fix);
    assert_eq!(drafts.last().unwrap().kind, Kind::Read);
}

// ── cost_basis_equals_value ─────────────────────────────────────────────────

#[test]
fn lots_bought_at_todays_price_are_flagged() {
    let mut g = graph();
    let prices: BTreeMap<i64, f64> = g.assets.iter().map(|a| (a.id, a.initial_price)).collect();
    for account in [1, 5] {
        for lot in g.positions.get_mut(&account).unwrap() {
            lot.cost_basis = lot.units * prices[&lot.asset_id];
        }
    }
    let drafts = review(&g, &results());
    let d = only(&drafts, "cost_basis_equals_value");
    assert_eq!(d.kind, Kind::Check);
    assert_eq!(d.section, Section::Portfolio);
    assert!(
        d.title.starts_with("Every taxable lot cost exactly"),
        "{}",
        d.title
    );
    assert!(d.reasoning.contains("2026-09-03"), "{}", d.reasoning);
    assert!(d.paths.is_empty());
}

#[test]
fn lots_with_their_own_basis_are_not() {
    // The anonymized snapshot's bases are unrelated to price.
    let drafts = review(&graph(), &results());
    assert!(!rules_of(&drafts).contains_key("cost_basis_equals_value"));
}

// ── idle_bank_cash ──────────────────────────────────────────────────────────

#[test]
fn cash_piling_up_in_the_bank_is_flagged() {
    let drafts = review(&graph(), &results());
    let d = only(&drafts, "idle_bank_cash");
    assert_eq!(
        d.title,
        "USAA holds over two years of spending for 11 years running, peaking at $1.3M in 2035"
    );
    assert!(
        d.reasoning.contains("from 2027 through 2037"),
        "{}",
        d.reasoning
    );
    assert!(d.reasoning.contains("Cash / T-Bills"), "{}", d.reasoning);
    assert!(d.evidence.contains(&Evidence::AccountSeries {
        account_id: 6,
        date: "2035-12-31".into(),
        value: 1.3e6,
    }));
}

#[test]
fn a_buffer_under_two_years_is_not() {
    let mut r = results();
    for year in FIRST..=LAST {
        set_usaa(&mut r, year, 100e3);
    }
    let drafts = review(&graph(), &r);
    assert!(!rules_of(&drafts).contains_key("idle_bank_cash"));
}

#[test]
fn a_short_run_of_idle_years_is_not() {
    let mut r = results();
    // Break the run every fourth year.
    for year in (2029..=2037).step_by(4) {
        set_usaa(&mut r, year, 10e3);
    }
    let drafts = review(&graph(), &r);
    assert!(!rules_of(&drafts).contains_key("idle_bank_cash"));
}

// ── unused_contribution_limits ──────────────────────────────────────────────

#[test]
fn limits_nobody_contributes_to_are_flagged() {
    let drafts = review(&graph(), &results());
    let d = only(&drafts, "unused_contribution_limits");
    assert_eq!(
        d.title,
        "Nothing is ever contributed to Vanguard Roth IRA, Fidelity 401(k) and Fidelity Roth"
    );
    assert!(
        d.reasoning
            .contains("($7,500 a year, $24,500 a year and $72,000 a year)"),
        "{}",
        d.reasoning
    );
    assert!(
        d.reasoning.contains("Salary lands in USAA"),
        "{}",
        d.reasoning
    );
    assert!(
        d.reasoning.contains("$0 in all 10 years with income"),
        "{}",
        d.reasoning
    );
}

#[test]
fn an_account_income_pays_into_is_not() {
    let mut g = graph();
    // Salary's Income effect pays into the 401(k) instead.
    g.effects.get_mut(&1).unwrap().to_account_id = Some(3);
    let drafts = review(&g, &results());
    let d = only(&drafts, "unused_contribution_limits");
    assert!(!d.title.contains("401(k)"), "{}", d.title);
}

#[test]
fn no_limits_or_no_income_means_no_note() {
    let mut g = graph();
    for inv in g.investment.values_mut() {
        inv.contribution_limit = None;
    }
    assert!(!rules_of(&review(&g, &results())).contains_key("unused_contribution_limits"));

    let mut r = results();
    for flow in &mut r.cash_flows {
        flow.income = 0.0;
    }
    assert!(!rules_of(&review(&graph(), &r)).contains_key("unused_contribution_limits"));
}

// ── sweep_sells_while_cash ──────────────────────────────────────────────────

#[test]
fn a_sweep_ahead_of_an_expense_the_bank_covers_is_removed() {
    let g = graph();
    let drafts = review(&g, &results());
    let d = only(&drafts, "sweep_sells_while_cash");
    assert_eq!(d.kind, Kind::Fix);
    assert_eq!(d.section, Section::Plan);
    assert_eq!(
        d.title,
        "Home Purchase sells $238k of investments while USAA holds $879k"
    );
    assert!(d.reasoning.contains("In 2031"), "{}", d.reasoning);
    assert!(d.reasoning.contains("$216k"), "{}", d.reasoning);

    let [path] = d.paths.as_slice() else {
        panic!("one path");
    };
    assert!(path.recommended);
    assert_eq!(path.label, "Remove the sweep; pay from USAA's cash");
    let [change] = changes(d).try_into().expect("one change");
    assert_eq!(change.op, ChangeOp::Remove);
    assert_eq!(change.target, ChangeTarget::Event(5));
    assert_eq!(change.path, "/effects/0");
    assert_eq!(change.expect.as_ref().unwrap()["kind"], json!("Sweep"));

    let resolved = resolve(&g, &changes(d)).unwrap();
    let [ResolvedChange::ReplaceEvent { id: 5, body }] = resolved.changes.as_slice() else {
        panic!("replaces Home Purchase: {:?}", resolved.changes);
    };
    assert_eq!(body.effects.len(), 3);
}

#[test]
fn a_sweep_the_bank_could_not_cover_stays() {
    let mut r = results();
    set_usaa(&mut r, 2030, 100e3);
    assert!(!rules_of(&review(&graph(), &r)).contains_key("sweep_sells_while_cash"));
}

// ── liability_payment_inflation_adjusted ────────────────────────────────────

#[test]
fn an_inflation_adjusted_loan_payment_is_fixed() {
    let g = graph();
    let drafts = review(&g, &results());
    let d = only(&drafts, "liability_payment_inflation_adjusted");
    assert_eq!(d.kind, Kind::Fix);
    assert!(
        d.title
            .starts_with("Morgtage Paydown rises with inflation, from $7,126/mo to $"),
        "{}",
        d.title
    );
    assert!(
        d.reasoning.contains("by 2047, when Mortgage reaches zero"),
        "{}",
        d.reasoning
    );

    // Two courses of action: keep today's figure (recommended), or pay the
    // level payment that clears the loan's draw over 30 years.
    let [fixed, amortizing] = d.paths.as_slice() else {
        panic!("two paths: {:#?}", d.paths);
    };
    assert_eq!(fixed.key, "fixed");
    assert!(fixed.recommended);
    assert_eq!(fixed.label, "Fix the payment at $6,000/mo");
    let [change] = fixed
        .changes()
        .cloned()
        .collect::<Vec<_>>()
        .try_into()
        .expect("one");
    assert_eq!(change.op, ChangeOp::Replace);
    assert_eq!(change.target, ChangeTarget::Event(7));
    assert_eq!(change.path, "/effects/0/amount");
    let expect = json!({"kind": "InflationAdjusted", "inner": {"kind": "Fixed", "value": 6000.0}});
    assert_eq!(change.expect, Some(expect.clone()));
    assert_eq!(
        change.value,
        Some(json!({"kind": "Fixed", "value": 6000.0}))
    );

    // Home Purchase (2031) draws $1M, inflation-adjusted, into the loan at 6%.
    let principal = 1_000_000.0 * f(2031);
    let r = 0.06 / 12.0;
    let payment = principal * r / (1.0 - (1.0_f64 + r).powi(-360));
    assert_eq!(amortizing.key, "amortizing");
    assert!(!amortizing.recommended);
    assert!(
        amortizing
            .label
            .starts_with("Fix it at the 30-year payment, $"),
        "{}",
        amortizing.label
    );
    assert!(
        amortizing
            .reasoning
            .as_deref()
            .unwrap()
            .contains("assuming a 30-year term"),
        "{:?}",
        amortizing.reasoning
    );
    let [change] = amortizing
        .changes()
        .cloned()
        .collect::<Vec<_>>()
        .try_into()
        .expect("one");
    assert_eq!(change.expect, Some(expect));
    let value = change.value.unwrap()["value"].as_f64().unwrap();
    assert!((value - payment).abs() < 0.01, "{value} vs {payment}");
    assert_changes_resolve(&g, std::slice::from_ref(d));
}

#[test]
fn a_loan_whose_payment_already_amortizes_offers_one_path() {
    // Pay the amortizing figure already (inflation-adjusted, so the note
    // stands): the alternative would be the same number.
    let g = graph();
    let principal = 1_000_000.0 * f(2031);
    let r = 0.06 / 12.0;
    let payment = principal * r / (1.0 - (1.0_f64 + r).powi(-360));
    let mut g2 = g.clone();
    let inner = g2.effects[&9]
        .amount_id
        .and_then(|id| g2.amounts[&id].left_id)
        .unwrap();
    g2.amounts.get_mut(&inner).unwrap().value = Some(payment);
    let drafts = review(&g2, &results());
    let d = only(&drafts, "liability_payment_inflation_adjusted");
    assert_eq!(d.paths.len(), 1);
    assert_eq!(d.paths[0].key, "fixed");
}

#[test]
fn a_fixed_loan_payment_is_left_alone() {
    let mut g = graph();
    g.effects.get_mut(&9).unwrap().amount_id = Some(17);
    assert!(
        !rules_of(&review(&g, &results())).contains_key("liability_payment_inflation_adjusted")
    );
}

// ── unmapped_or_mismatched_assets ───────────────────────────────────────────

#[test]
fn mismatched_assets_are_flagged() {
    let drafts = review(&graph(), &results());
    let titles: Vec<_> = drafts
        .iter()
        .filter(|d| d.rule == "unmapped_or_mismatched_assets")
        .map(|d| d.title.as_str())
        .collect();
    assert_eq!(
        titles,
        [
            "GOOG moves exactly like US Total Market",
            "VFIFX is modelled as US Total Market",
            "VWO is modelled as International Developed",
        ]
    );
}

#[test]
fn a_stock_with_tracking_error_and_an_unheld_asset_are_not() {
    let mut g = graph();
    g.assets
        .iter_mut()
        .find(|a| a.name == "GOOG")
        .unwrap()
        .tracking_error = Some(0.3);
    let drafts = review(&g, &results());
    assert!(!drafts.iter().any(|d| d.title.starts_with("GOOG")));
    // House is unmapped but held only through the property account.
    assert!(!drafts.iter().any(|d| d.title.starts_with("House")));
}

#[test]
fn an_unmapped_holding_is_flagged_with_its_value() {
    let mut g = graph();
    g.assets
        .iter_mut()
        .find(|a| a.name == "VBTLX")
        .unwrap()
        .return_profile_id = None;
    let drafts = review(&g, &results());
    assert!(
        drafts.iter().any(|d| d
            .title
            .starts_with("VBTLX has no return assumption, so its $")),
        "{drafts:#?}"
    );
}

// ── shortfall_account_concentration ─────────────────────────────────────────

#[test]
fn shortfalls_concentrated_in_one_account_are_read() {
    let drafts = review(&graph(), &results());
    let d = only(&drafts, "shortfall_account_concentration");
    assert_eq!(d.kind, Kind::Read);
    assert_eq!(d.section, Section::Results);
    assert_eq!(
        d.title,
        "USAA is the account that runs short in 170 of 180 shortfall paths"
    );
    assert!(
        d.reasoning.contains("in 2074, at age 78"),
        "{}",
        d.reasoning
    );
    assert!(d.reasoning.contains("up to $38,200"), "{}", d.reasoning);
    assert!(!d.reasoning.contains("liquidity"), "{}", d.reasoning);
}

#[test]
fn failing_paths_that_end_solvent_are_read_as_liquidity() {
    let mut r = results();
    let fd = r.funding_diagnostics.as_mut().unwrap();
    fd.shortfall_accounts = vec![AccountCount {
        account_id: Some(6),
        count: 40,
    }];
    fd.failed_solvent = 150;
    let drafts = review(&graph(), &r);
    let d = only(&drafts, "shortfall_account_concentration");
    assert_eq!(
        d.title,
        "150 of 189 failing paths still end with money: liquidity, not wealth"
    );
}

#[test]
fn no_diagnostics_or_no_pattern_means_no_note() {
    let mut r = results();
    r.funding_diagnostics = None;
    assert!(!rules_of(&review(&graph(), &r)).contains_key("shortfall_account_concentration"));

    let mut r = results();
    let fd = r.funding_diagnostics.as_mut().unwrap();
    fd.shortfall_accounts = vec![AccountCount {
        account_id: Some(6),
        count: 40,
    }];
    assert!(!rules_of(&review(&graph(), &r)).contains_key("shortfall_account_concentration"));
}

// ── success_vs_funding_gap ──────────────────────────────────────────────────

#[test]
fn success_well_above_funding_is_read() {
    let mut r = results();
    r.stats.success_rate = 0.965;
    let drafts = review(&graph(), &r);
    let d = only(&drafts, "success_vs_funding_gap");
    assert!(
        d.title.starts_with("Success says 96.5%, but 90."),
        "{}",
        d.title
    );
    assert!(d.reasoning.contains("119 paths"), "{}", d.reasoning);
}

#[test]
fn property_propping_up_net_worth_is_read() {
    let mut r = results();
    // Only Vanguard's $1M beside the house on the last point.
    let last = r.bands[0].dates.len() - 1;
    for series in &mut r.account_series {
        series.values[last] = match series.account_id {
            1 => 1e6,
            8 => series.values[last],
            _ => 0.0,
        };
    }
    r.bands[0].net_worth[last] = r.account_series.iter().map(|s| s.values[last]).sum();
    let drafts = review(&graph(), &r);
    let d = only(&drafts, "success_vs_funding_gap");
    assert!(d.title.starts_with("House is "), "{}", d.title);
    assert!(d.reasoning.contains("cannot pay a bill"), "{}", d.reasoning);
}

#[test]
fn a_small_gap_is_not() {
    assert!(!rules_of(&review(&graph(), &results())).contains_key("success_vs_funding_gap"));
}

// ── formatting ──────────────────────────────────────────────────────────────

#[test]
fn money_reads_like_the_review_tab() {
    assert_eq!(money(38_200.0), "$38,200");
    assert_eq!(money(6_000.0), "$6,000");
    assert_eq!(money(218_400.0), "$218k");
    assert_eq!(money(1_450_000.0), "$1.45M");
    assert_eq!(money(2_200_000.0), "$2.2M");
    assert_eq!(money(114_500_000.0), "$114.5M");
    assert_eq!(money(-1_600_000.0), "−$1.6M");
    assert_eq!(money(0.0), "$0");
    assert_eq!(percent(0.955), "95.5%");
}
