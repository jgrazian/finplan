//! The plan-shaped GET bodies, built from a [`ScenarioGraph`].
//!
//! Each function returns what the server's route of the same name returns, so
//! anything holding a graph (the server after loading one, the browser's local
//! store) answers the same API with the same types. Orderings are the routes':
//! accounts, assets and events by `(sort_order, id)`; a library by
//! `(sort_order, name)`, tax configs by name.
//!
//! A graph carries the caller's whole library of return profiles and
//! distributions, and its tax configs and inflation profiles in
//! [`ScenarioGraph::tax_configs`] and [`ScenarioGraph::inflation_profiles`]
//! (which a live load fills and the serialized form leaves out). The library
//! readers here read those; whoever holds a serialized graph fills them first.

use std::collections::HashMap;

use crate::compile;
use crate::error::{PlanError, PlanResult};
use crate::graph::{DistributionRow, ScenarioGraph, TaxConfigEntry};
use crate::specs::accounts::{
    Account, ContributionPeriod, FlavorSpec, PlanType, Position, TaxStatus,
};
use crate::specs::assets::Asset;
use crate::specs::events::Event;
use crate::specs::parameters::{NamedParameter, usages};
use crate::specs::profiles::{AssetClass, DistributionSpec, HistoryPreset, Profile};
use crate::specs::scenarios::{CompileReport, Scenario, ScenarioStatus};
use crate::specs::taxes::{Bracket, TaxConfig};

pub use crate::expressions::{ExpressionValidation, validate_effect as validate_expression};
pub use crate::preflight::{PreflightIssue, PreflightReport, preflight};

// ── the scenario ────────────────────────────────────────────────────────────

/// What a scenario's response carries that its graph does not: the slug the
/// server assigns, whether it is a draft, and the result of its latest run.
#[derive(Debug, Clone)]
pub struct ScenarioExtras {
    pub slug: String,
    pub status: ScenarioStatus,
    pub last_run_at: Option<String>,
    pub last_success_rate: Option<f64>,
}

impl ScenarioExtras {
    /// An active plan that has not run (or whose runs the caller does not
    /// keep with it).
    pub fn active(slug: impl Into<String>) -> Self {
        ScenarioExtras {
            slug: slug.into(),
            status: ScenarioStatus::Active,
            last_run_at: None,
            last_success_rate: None,
        }
    }
}

/// `GET /scenarios/{id}`: the graph's scenario row, plus what `extras` says.
pub fn scenario(graph: &ScenarioGraph, extras: ScenarioExtras) -> Scenario {
    let row = &graph.scenario;
    Scenario {
        id: row.id,
        slug: extras.slug,
        name: row.name.clone(),
        description: row.description.clone(),
        start_date: row.start_date.clone(),
        birth_date: row.birth_date.clone(),
        duration_years: row.duration_years,
        inflation_profile_id: row.inflation_profile_id,
        tax_config_id: row.tax_config_id,
        collect_ledger: row.collect_ledger != 0,
        status: extras.status,
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
        last_run_at: extras.last_run_at,
        last_success_rate: extras.last_success_rate,
    }
}

/// `POST /scenarios/{id}/compile`: lower the scenario without running it.
/// Lets the UI surface configuration errors before a long Monte Carlo run.
pub fn compile_report(graph: &ScenarioGraph) -> PlanResult<CompileReport> {
    let compiled = compile::compile(graph)?;
    Ok(CompileReport {
        ok: true,
        accounts: compiled.config.accounts.len(),
        assets: compiled.config.asset_prices.len(),
        events: compiled.config.events.len(),
        return_profiles: compiled.config.return_profiles.len(),
        duration_years: compiled.config.duration_years,
    })
}

// ── accounts ────────────────────────────────────────────────────────────────

/// `GET /scenarios/{id}/accounts`.
pub fn accounts(graph: &ScenarioGraph) -> PlanResult<Vec<Account>> {
    let mut rows: Vec<_> = graph.accounts.iter().collect();
    rows.sort_by_key(|a| (a.sort_order, a.id));
    rows.into_iter().map(|a| account(graph, a.id)).collect()
}

/// `GET /scenarios/{id}/accounts/{account}`: the account, its flavor fields
/// flattened in beside it, and its lots.
pub fn account(graph: &ScenarioGraph, id: i64) -> PlanResult<Account> {
    let row = graph
        .accounts
        .iter()
        .find(|a| a.id == id)
        .ok_or(PlanError::NotFound("account"))?;
    let detail = || PlanError::internal(format!("account {id} has no {} row", row.flavor));
    let flavor = match row.flavor.as_str() {
        "Bank" => {
            let bank = graph.bank.get(&id).ok_or_else(detail)?;
            FlavorSpec::Bank {
                cash_value: bank.cash_value,
                return_profile_id: bank.return_profile_id,
            }
        }
        "Investment" => {
            let inv = graph.investment.get(&id).ok_or_else(detail)?;
            FlavorSpec::Investment {
                tax_status: match inv.tax_status.as_str() {
                    "TaxDeferred" => TaxStatus::TaxDeferred,
                    "TaxFree" => TaxStatus::TaxFree,
                    _ => TaxStatus::Taxable,
                },
                cash_value: inv.cash_value,
                cash_return_profile_id: inv.cash_return_profile_id,
                contribution_limit: inv.contribution_limit,
                contribution_period: match inv.contribution_period.as_deref() {
                    Some("Monthly") => Some(ContributionPeriod::Monthly),
                    Some("Yearly") => Some(ContributionPeriod::Yearly),
                    _ => None,
                },
                plan_type: inv.plan_type.as_deref().and_then(PlanType::parse),
                catch_up: inv.catch_up.clone(),
            }
        }
        "Property" => {
            let property = graph.property.get(&id).ok_or_else(detail)?;
            FlavorSpec::Property {
                asset_id: property.asset_id,
                value: property.value,
            }
        }
        _ => {
            let loan = graph.liability.get(&id).ok_or_else(detail)?;
            FlavorSpec::Liability {
                principal: loan.principal,
                interest_rate: loan.interest_rate,
                repayment: crate::specs::repayment_of(loan.repay_from_account_id, loan.term_months),
            }
        }
    };
    Ok(Account {
        id: row.id,
        name: row.name.clone(),
        description: row.description.clone(),
        sort_order: row.sort_order,
        flavor,
        positions: positions(graph, id),
    })
}

/// `GET /scenarios/{id}/accounts/{account}/positions`: the account's lots in
/// the order the graph holds them, which a load fills as the routes list them
/// (`ORDER BY sort_order, purchase_date, id`) and an in-memory edit keeps.
/// An account with no lots, or none by that id, has none.
pub fn positions(graph: &ScenarioGraph, account_id: i64) -> Vec<Position> {
    graph
        .positions
        .get(&account_id)
        .into_iter()
        .flatten()
        .map(|p| Position {
            id: p.id,
            asset_id: p.asset_id,
            purchase_date: p.purchase_date.clone(),
            units: p.units,
            cost_basis: p.cost_basis,
        })
        .collect()
}

// ── assets, events, parameters ──────────────────────────────────────────────

/// `GET /scenarios/{id}/assets`.
pub fn assets(graph: &ScenarioGraph) -> Vec<Asset> {
    let mut rows: Vec<_> = graph.assets.iter().collect();
    rows.sort_by_key(|a| (a.sort_order, a.id));
    rows.into_iter().map(asset_of).collect()
}

/// `GET /scenarios/{id}/assets/{asset}`.
pub fn asset(graph: &ScenarioGraph, id: i64) -> PlanResult<Asset> {
    graph
        .assets
        .iter()
        .find(|a| a.id == id)
        .map(asset_of)
        .ok_or(PlanError::NotFound("asset"))
}

fn asset_of(row: &crate::graph::AssetRow) -> Asset {
    Asset {
        id: row.id,
        name: row.name.clone(),
        description: row.description.clone(),
        initial_price: row.initial_price,
        return_profile_id: row.return_profile_id,
        tracking_error: row.tracking_error,
        sort_order: row.sort_order,
    }
}

/// `GET /scenarios/{id}/events`.
pub fn events(graph: &ScenarioGraph) -> PlanResult<Vec<Event>> {
    let mut rows: Vec<_> = graph.events.iter().collect();
    rows.sort_by_key(|e| (e.sort_order, e.id));
    rows.into_iter().map(|e| event(graph, e.id)).collect()
}

/// `GET /scenarios/{id}/events/{event}`.
pub fn event(graph: &ScenarioGraph, id: i64) -> PlanResult<Event> {
    crate::specs::events::read_event(graph, id)
}

/// `GET /scenarios/{id}/parameters`, each with where it is used.
pub fn parameters(graph: &ScenarioGraph) -> PlanResult<Vec<NamedParameter>> {
    graph
        .parameters
        .iter()
        .map(|p| {
            Ok(NamedParameter {
                id: p.id,
                scenario_id: graph.scenario.id,
                name: p.name.clone(),
                value: p.try_into()?,
                uses: usages(graph, p.id)?,
            })
        })
        .collect()
}

// ── the library: profiles and tax configs ───────────────────────────────────

/// `GET /history-presets`: the bootstrap histories the engine ships with.
pub fn history_presets() -> Vec<HistoryPreset> {
    compile::HISTORY_PRESETS
        .iter()
        .filter_map(|id| {
            // Every id in the table resolves; `filter_map` rather than an
            // unwrap so a mismatch drops one row instead of the process.
            let history = compile::historical_returns(id).ok()?;
            Some(HistoryPreset {
                id: (*id).to_string(),
                name: history.name.to_string(),
                start_year: i32::from(history.start_year),
                returns: history.returns.to_vec(),
            })
        })
        .collect()
}

/// The nested [`DistributionSpec`] rooted at row `id` of `rows`.
pub fn distribution(rows: &HashMap<i64, DistributionRow>, id: i64) -> PlanResult<DistributionSpec> {
    distribution_at(rows, id, 0)
}

fn distribution_at(
    rows: &HashMap<i64, DistributionRow>,
    id: i64,
    depth: usize,
) -> PlanResult<DistributionSpec> {
    if depth > 8 {
        return Err(PlanError::internal("distribution graph is cyclic"));
    }
    let row = rows.get(&id).ok_or(PlanError::NotFound("distribution"))?;
    Ok(match row.kind.as_str() {
        "Fixed" => DistributionSpec::Fixed {
            rate: row.rate.unwrap_or_default(),
        },
        "Normal" => DistributionSpec::Normal {
            mean: row.mean.unwrap_or_default(),
            std_dev: row.std_dev.unwrap_or_default(),
        },
        "LogNormal" => DistributionSpec::LogNormal {
            mean: row.mean.unwrap_or_default(),
            std_dev: row.std_dev.unwrap_or_default(),
        },
        "StudentT" => DistributionSpec::StudentT {
            mean: row.mean.unwrap_or_default(),
            scale: row.scale.unwrap_or_default(),
            df: row.df.unwrap_or(5.0),
        },
        "RegimeSwitching" => DistributionSpec::RegimeSwitching {
            bull: Box::new(distribution_at(
                rows,
                row.bull_id.unwrap_or_default(),
                depth + 1,
            )?),
            bear: Box::new(distribution_at(
                rows,
                row.bear_id.unwrap_or_default(),
                depth + 1,
            )?),
            bull_to_bear_prob: row.bull_to_bear_prob.unwrap_or_default(),
            bear_to_bull_prob: row.bear_to_bull_prob.unwrap_or_default(),
        },
        "Bootstrap" => DistributionSpec::Bootstrap {
            preset: row
                .history_preset
                .clone()
                .unwrap_or_else(|| "sp500".to_string()),
            block_size: row.block_size,
        },
        _ => DistributionSpec::None,
    })
}

/// Names of the assets and accounts of this plan that point at return profile
/// `profile_id`: assets, then bank accounts, then investment accounts' cash,
/// each by id. A profile is shared across a user's plans, so this is what
/// *this* plan uses; the server's own routes also count the other plans.
pub fn profile_users(graph: &ScenarioGraph, profile_id: i64) -> Vec<String> {
    let mut assets: Vec<_> = graph
        .assets
        .iter()
        .filter(|a| a.return_profile_id == Some(profile_id))
        .map(|a| (a.id, a.name.as_str()))
        .collect();
    assets.sort();
    let account_name = |id: i64| {
        graph
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name.as_str())
    };
    let mut banks: Vec<_> = graph
        .bank
        .values()
        .filter(|b| b.return_profile_id == profile_id)
        .filter_map(|b| Some((b.account_id, account_name(b.account_id)?)))
        .collect();
    banks.sort();
    let mut investments: Vec<_> = graph
        .investment
        .values()
        .filter(|i| i.cash_return_profile_id == profile_id)
        .filter_map(|i| Some((i.account_id, account_name(i.account_id)?)))
        .collect();
    investments.sort();
    [assets, banks, investments]
        .into_iter()
        .flatten()
        .map(|(_, name)| name.to_string())
        .collect()
}

/// `GET /return-profiles`, in library order, with `used_by` as
/// [`profile_users`] says.
pub fn return_profiles(graph: &ScenarioGraph) -> PlanResult<Vec<Profile>> {
    return_profiles_with_usage(graph, |id| profile_users(graph, id))
}

/// [`return_profiles`] with `used_by` taken from `used_by`, for a caller that
/// knows the library's users beyond this one plan.
pub fn return_profiles_with_usage(
    graph: &ScenarioGraph,
    used_by: impl Fn(i64) -> Vec<String>,
) -> PlanResult<Vec<Profile>> {
    let mut rows: Vec<_> = graph.return_profiles.values().collect();
    rows.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
    rows.into_iter()
        .map(|row| {
            Ok(Profile {
                id: row.id,
                name: row.name.clone(),
                description: row.description.clone(),
                asset_class: row.asset_class.as_deref().and_then(AssetClass::parse),
                distribution: distribution(&graph.distributions, row.distribution_id)?,
                used_by: used_by(row.id),
            })
        })
        .collect()
}

/// `GET /return-profiles/{id}`.
pub fn return_profile(graph: &ScenarioGraph, id: i64) -> PlanResult<Profile> {
    let row = graph
        .return_profiles
        .get(&id)
        .ok_or(PlanError::NotFound("return profile"))?;
    Ok(Profile {
        id: row.id,
        name: row.name.clone(),
        description: row.description.clone(),
        asset_class: row.asset_class.as_deref().and_then(AssetClass::parse),
        distribution: distribution(&graph.distributions, row.distribution_id)?,
        used_by: profile_users(graph, id),
    })
}

/// `GET /inflation-profiles`, in library order. An inflation profile is not a
/// holding, so it has no asset class and nothing "uses" it by that name.
pub fn inflation_profiles(graph: &ScenarioGraph) -> PlanResult<Vec<Profile>> {
    let mut rows: Vec<_> = graph.inflation_profiles.iter().collect();
    rows.sort_by(|(_, a), (_, b)| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
    rows.into_iter()
        .map(|(id, entry)| {
            Ok(Profile {
                id: *id,
                name: entry.name.clone(),
                description: entry.description.clone(),
                asset_class: None,
                distribution: distribution(&graph.distributions, entry.distribution_id)?,
                used_by: Vec::new(),
            })
        })
        .collect()
}

/// `GET /tax-configs`, by name.
pub fn tax_configs(graph: &ScenarioGraph) -> Vec<TaxConfig> {
    let mut entries: Vec<_> = graph.tax_configs.values().collect();
    entries.sort_by(|a, b| a.config.name.cmp(&b.config.name));
    entries.into_iter().map(tax_config_of).collect()
}

/// `GET /tax-configs/{id}`.
pub fn tax_config(graph: &ScenarioGraph, id: i64) -> PlanResult<TaxConfig> {
    graph
        .tax_configs
        .get(&id)
        .map(tax_config_of)
        .ok_or(PlanError::NotFound("tax config"))
}

fn tax_config_of(entry: &TaxConfigEntry) -> TaxConfig {
    let config = &entry.config;
    let mut brackets = entry.brackets.clone();
    brackets.sort_by(|a, b| a.threshold.total_cmp(&b.threshold));
    TaxConfig {
        id: config.id,
        name: config.name.clone(),
        description: entry.description.clone(),
        state_rate: config.state_rate,
        capital_gains_rate: config.capital_gains_rate,
        early_withdrawal_penalty_rate: config.early_withdrawal_penalty_rate,
        standard_deduction: config.standard_deduction,
        age_65_extra_deduction: config.age_65_extra_deduction,
        federal_brackets: brackets
            .into_iter()
            .map(|b| Bracket {
                threshold: b.threshold,
                rate: b.rate,
            })
            .collect(),
    }
}
