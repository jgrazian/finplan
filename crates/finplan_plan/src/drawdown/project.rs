//! One path per strategy, folded into yearly rows.

use std::collections::{BTreeMap, BTreeSet};

use finplan_core::model::{
    AccountSnapshot, AccountSnapshotFlavor, CashFlowKind, SimulationResult, StateEvent,
    WealthSnapshot,
};
use finplan_core::simulation::simulate;

use super::normalize::{fixed_sweeps, normalize};
use super::retirement::resolve;
use super::{
    DrawdownAccount, DrawdownBody, DrawdownChoice, DrawdownIncomeSource, DrawdownMarker,
    DrawdownRequest, DrawdownSummary, DrawdownYear, MarkerKind, StrategyChoice,
};
use crate::compile::{self, CompiledScenario};
use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;

/// A year under a cent is not a flow.
const EPS: f64 = 0.005;

/// One year of one path before its arrays are aligned to the body's labels.
#[derive(Default)]
struct Sparse {
    year: i64,
    inflation: f64,
    spending: f64,
    income: BTreeMap<i64, f64>,
    withdrawals: BTreeMap<i64, f64>,
    rmd: BTreeMap<i64, f64>,
    balances: BTreeMap<i64, f64>,
    /// Gross sale proceeds and the credits those sales produced.
    gross: f64,
    credits: f64,
    shortfall: f64,
    total_tax: f64,
}

struct Folded {
    years: Vec<Sparse>,
    ending_balance: f64,
    ending_balance_real: f64,
    after_tax_ending_balance: f64,
    after_tax_ending_balance_real: f64,
}

/// Total of the overdrawn bank balances at a snapshot, as a positive number.
fn overdrawn(snapshot: &WealthSnapshot) -> f64 {
    snapshot
        .accounts
        .iter()
        .filter_map(|a| match a.flavor {
            AccountSnapshotFlavor::Bank(cash) if cash < 0.0 => Some(-cash),
            _ => None,
        })
        .sum()
}

fn tracked_balance(account: &AccountSnapshot) -> Option<f64> {
    matches!(
        account.flavor,
        AccountSnapshotFlavor::Bank(_) | AccountSnapshotFlavor::Investment { .. }
    )
    .then(|| account.total_value())
}

/// Fold one simulated path from `from_year` to its last year.
fn fold(compiled: &CompiledScenario, result: &SimulationResult, from_year: i64) -> Folded {
    let snapshots = &result.wealth_snapshots;
    let year_of = |s: &WealthSnapshot| i64::from(s.date.year());
    let (Some(first), Some(last)) = (snapshots.first(), snapshots.last()) else {
        return Folded {
            years: Vec::new(),
            ending_balance: 0.0,
            ending_balance_real: 0.0,
            after_tax_ending_balance: 0.0,
            after_tax_ending_balance_real: 0.0,
        };
    };
    let start_year = year_of(first);
    let end_year = year_of(last);
    let from_year = from_year.clamp(start_year, end_year);

    let mut rows: BTreeMap<i64, Sparse> = (from_year..=end_year)
        .map(|year| {
            let index = (year - start_year) as usize;
            let inflation = result
                .cumulative_inflation
                .get(index)
                .or(result.cumulative_inflation.last())
                .copied()
                .unwrap_or(1.0);
            (
                year,
                Sparse {
                    year,
                    inflation,
                    ..Sparse::default()
                },
            )
        })
        .collect();

    let account = |id| compiled.id_map.account_db_id(id);
    for entry in &result.ledger {
        let Some(row) = rows.get_mut(&i64::from(entry.date.year())) else {
            continue;
        };
        match &entry.event {
            StateEvent::CashDebit {
                amount,
                kind: CashFlowKind::Expense,
                ..
            } => row.spending += amount,
            StateEvent::CashCredit {
                amount,
                kind: CashFlowKind::Income,
                ..
            } => {
                if let Some(event) = entry
                    .source_event
                    .and_then(|e| compiled.id_map.event_db_id(e))
                {
                    *row.income.entry(event).or_default() += amount;
                }
            }
            StateEvent::CashCredit {
                amount,
                kind: CashFlowKind::LiquidationProceeds | CashFlowKind::RmdWithdrawal,
                ..
            } => row.credits += amount,
            StateEvent::AssetSale {
                account_id,
                proceeds,
                ..
            } => {
                row.gross += proceeds;
                if let Some(id) = account(*account_id) {
                    *row.withdrawals.entry(id).or_default() += proceeds;
                }
            }
            StateEvent::RmdWithdrawal {
                account_id,
                actual_amount,
                ..
            } => {
                if let Some(id) = account(*account_id) {
                    *row.rmd.entry(id).or_default() += actual_amount;
                }
            }
            _ => {}
        }
    }

    // Year-end state: the last snapshot dated in the year. The baseline for
    // the first row's shortfall is the last one before it (or the start).
    let mut baseline = snapshots
        .iter()
        .rfind(|s| year_of(s) < from_year)
        .map_or_else(|| overdrawn(first), overdrawn);
    for (year, row) in &mut rows {
        let Some(snapshot) = snapshots.iter().rfind(|s| year_of(s) == *year) else {
            continue;
        };
        for a in &snapshot.accounts {
            if let (Some(id), Some(value)) = (account(a.account_id), tracked_balance(a)) {
                row.balances.insert(id, value);
            }
        }
        let now = overdrawn(snapshot);
        row.shortfall = (now - baseline).max(0.0);
        baseline = now;
        row.total_tax = result
            .yearly_taxes
            .iter()
            .filter(|t| i64::from(t.year) == *year)
            .map(|t| t.total_tax + t.early_withdrawal_penalties)
            .sum::<f64>()
            + 0.0;
    }

    let ending_balance: f64 = last.accounts.iter().map(AccountSnapshot::total_value).sum();
    let after_tax_ending_balance = compiled.config.after_tax_final_net_worth(result);
    let inflation = rows.values().last().map_or(1.0, |r| r.inflation);
    Folded {
        years: rows.into_values().collect(),
        ending_balance,
        ending_balance_real: ending_balance / inflation,
        after_tax_ending_balance,
        after_tax_ending_balance_real: after_tax_ending_balance / inflation,
    }
}

/// Lay a folded path out against the body's labels and balance the bars.
fn densify(
    folded: Folded,
    accounts: &[DrawdownAccount],
    sources: &[DrawdownIncomeSource],
) -> (Vec<DrawdownYear>, Vec<DrawdownMarker>) {
    let by_account = |map: &BTreeMap<i64, f64>| -> Vec<f64> {
        accounts
            .iter()
            .map(|a| map.get(&a.id).copied().unwrap_or(0.0))
            .collect()
    };
    let mut markers = Vec::new();
    let mut seen_income = BTreeSet::new();
    let mut seen_rmd = BTreeSet::new();
    let years = folded
        .years
        .into_iter()
        .map(|row| {
            let income: Vec<f64> = sources
                .iter()
                .map(|s| row.income.get(&s.event_id).copied().unwrap_or(0.0))
                .collect();
            let withdrawals = by_account(&row.withdrawals);
            let rmd = by_account(&row.rmd);
            for (source, amount) in sources.iter().zip(&income) {
                if *amount > EPS && seen_income.insert(source.event_id) {
                    markers.push(DrawdownMarker {
                        kind: MarkerKind::Income,
                        id: source.event_id,
                        year: row.year,
                    });
                }
            }
            for (account, amount) in accounts.iter().zip(&rmd) {
                if *amount > EPS && seen_rmd.insert(account.id) {
                    markers.push(DrawdownMarker {
                        kind: MarkerKind::Rmd,
                        id: account.id,
                        year: row.year,
                    });
                }
            }

            // Withheld = gross sales less the credits they produced. A credit
            // with no sale behind it (a house sold) must not read as negative.
            let withdrawal_taxes = (row.gross - row.credits).max(0.0);
            let tracked_in: f64 = income.iter().sum::<f64>() + withdrawals.iter().sum::<f64>();
            let need = row.spending + withdrawal_taxes - row.shortfall - tracked_in;
            DrawdownYear {
                year: row.year,
                inflation: row.inflation,
                spending: row.spending,
                income,
                withdrawals,
                rmd,
                withdrawal_taxes,
                cash: need.max(0.0),
                surplus: (-need).max(0.0),
                shortfall: row.shortfall,
                balances: by_account(&row.balances),
                total_tax: row.total_tax,
            }
        })
        .collect();
    (years, markers)
}

/// Re-simulate the run's input snapshot `graph` on `seed` once per strategy in
/// `request`, and fold each path's ledger into yearly rows.
pub fn project(
    graph: &ScenarioGraph,
    seed: u64,
    request: &DrawdownRequest,
) -> PlanResult<DrawdownBody> {
    let choices = request.choices()?;
    let compiled = compile::compile(graph)?;

    let mut base = None;
    let retirement = resolve(&compiled, request.retirement_year, seed, &mut base)?;

    let mut folded = Vec::with_capacity(choices.len());
    for choice in &choices {
        let (config, overlay) = normalize(&compiled.config, choice, retirement.date)?;
        // The plan as it is may already have been simulated to find the
        // retirement date.
        let reused = matches!(choice, StrategyChoice::AsPlanned)
            .then(|| base.take())
            .flatten();
        let result = match reused {
            Some(result) => result,
            None => {
                let mut config = config;
                config.collect_ledger = true;
                simulate(&config, seed).map_err(|e| PlanError::internal(e.to_string()))?
            }
        };
        folded.push((
            choice.clone(),
            overlay,
            fold(&compiled, &result, retirement.year),
        ));
    }

    let accounts = labelled_accounts(graph, &compiled);
    let income_ids: BTreeSet<i64> = folded
        .iter()
        .flat_map(|(_, _, f)| f.years.iter().flat_map(|y| y.income.keys().copied()))
        .collect();
    let mut events: Vec<_> = graph
        .events
        .iter()
        .filter(|e| income_ids.contains(&e.id))
        .collect();
    events.sort_by_key(|e| (e.sort_order, e.id));
    let income_sources: Vec<DrawdownIncomeSource> = events
        .into_iter()
        .map(|e| DrawdownIncomeSource {
            event_id: e.id,
            name: e.name.clone(),
        })
        .collect();

    let choices = folded
        .into_iter()
        .map(|(choice, overlay, folded)| {
            let Folded {
                ending_balance,
                ending_balance_real,
                after_tax_ending_balance,
                after_tax_ending_balance_real,
                ..
            } = folded;
            let (years, markers) = densify(folded, &accounts, &income_sources);
            DrawdownChoice {
                choice,
                overlay,
                summary: DrawdownSummary {
                    lifetime_spending: years.iter().map(|y| y.spending).sum(),
                    lifetime_tax: years.iter().map(|y| y.total_tax).sum::<f64>() + 0.0,
                    ending_balance,
                    ending_balance_real,
                    after_tax_ending_balance,
                    after_tax_ending_balance_real,
                    first_shortfall_year: years.iter().find(|y| y.shortfall > EPS).map(|y| y.year),
                    markers,
                },
                years,
            }
        })
        .collect();

    Ok(DrawdownBody {
        seed: seed.to_string(),
        retirement: retirement.info(),
        birth_year: compiled.config.birth_date.map(|d| i64::from(d.year())),
        accounts,
        income_sources,
        fixed_sweeps: fixed_sweeps(&compiled),
        plan_funding: graph.funding(),
        choices,
    })
}

/// The bank and investment accounts, in display order.
fn labelled_accounts(graph: &ScenarioGraph, compiled: &CompiledScenario) -> Vec<DrawdownAccount> {
    let mut rows: Vec<_> = graph
        .accounts
        .iter()
        .filter(|a| a.flavor == "Bank" || a.flavor == "Investment")
        .collect();
    rows.sort_by_key(|a| (a.sort_order, a.id));
    rows.into_iter()
        .map(|a| DrawdownAccount {
            id: a.id,
            name: compiled
                .account_names
                .get(&a.id)
                .cloned()
                .unwrap_or_else(|| a.name.clone()),
            flavor: a.flavor.clone(),
            tax_status: graph.investment.get(&a.id).map(|i| i.tax_status.clone()),
        })
        .collect()
}
