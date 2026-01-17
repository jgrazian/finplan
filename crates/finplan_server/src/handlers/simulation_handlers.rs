use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::{
    CreateSimulationRequest, RunSimulationRequest, SavedSimulation, SimulationListItem,
    SimulationRunRecord, UpdateSimulationRequest,
};
use crate::validation;
use finplan_core::config::SimulationConfig;
use finplan_core::model::{AccountId, MonteCarloResult, SimulationResult, StateEvent};
use finplan_core::simulation::monte_carlo_simulate;
use jiff::civil::Date;

// Database connection wrapper
type DbConn = Arc<Mutex<Connection>>;

// ============================================================================
// Simulation Handlers
// ============================================================================

pub async fn list_simulations(
    State(db): State<DbConn>,
) -> ApiResult<Json<Vec<SimulationListItem>>> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare("SELECT id, name, description, portfolio_id, created_at, updated_at FROM simulations ORDER BY updated_at DESC")?;

    let simulations = stmt
        .query_map([], |row| {
            Ok(SimulationListItem {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                portfolio_id: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(simulations))
}

pub async fn create_simulation(
    State(db): State<DbConn>,
    Json(req): Json<CreateSimulationRequest>,
) -> ApiResult<Json<SavedSimulation>> {
    // Validate input
    validation::validate_simulation_name(&req.name)?;
    validation::validate_simulation_params(&req.parameters)?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let params_json = serde_json::to_string(&req.parameters)?;

    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO simulations (id, name, description, parameters, portfolio_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, req.name, req.description, params_json, req.portfolio_id, now, now],
    )?;

    Ok(Json(SavedSimulation {
        id,
        name: req.name,
        description: req.description,
        parameters: req.parameters,
        portfolio_id: req.portfolio_id,
        created_at: now.clone(),
        updated_at: now,
    }))
}

pub async fn get_simulation(
    State(db): State<DbConn>,
    Path(id): Path<String>,
) -> ApiResult<Json<SavedSimulation>> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare("SELECT id, name, description, parameters, portfolio_id, created_at, updated_at FROM simulations WHERE id = ?1")?;

    let simulation = stmt
        .query_row([&id], |row| {
            let params_json: String = row.get(3)?;
            let parameters: SimulationConfig =
                serde_json::from_str(&params_json).map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(SavedSimulation {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                parameters,
                portfolio_id: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ApiError::SimulationNotFound(id.clone()),
            _ => ApiError::from(e),
        })?;

    Ok(Json(simulation))
}

pub async fn update_simulation(
    State(db): State<DbConn>,
    Path(id): Path<String>,
    Json(req): Json<UpdateSimulationRequest>,
) -> ApiResult<Json<SavedSimulation>> {
    let conn = db.lock()?;

    // Validate name if provided
    if let Some(ref name) = req.name {
        validation::validate_simulation_name(name)?;
    }

    // Validate parameters if provided
    if let Some(ref params) = req.parameters {
        validation::validate_simulation_params(params)?;
    }

    // Get existing simulation
    let mut stmt = conn
        .prepare("SELECT name, description, parameters, portfolio_id, created_at FROM simulations WHERE id = ?1")?;

    let (current_name, current_desc, current_params_json, current_portfolio_id, created_at): (
        String,
        Option<String>,
        String,
        Option<String>,
        String,
    ) = stmt
        .query_row([&id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ApiError::SimulationNotFound(id.clone()),
            _ => ApiError::from(e),
        })?;

    let name = req.name.unwrap_or(current_name);
    let description = req.description.or(current_desc);
    let portfolio_id = req.portfolio_id.or(current_portfolio_id);
    let parameters = if let Some(p) = req.parameters {
        p
    } else {
        serde_json::from_str(&current_params_json)?
    };

    let params_json = serde_json::to_string(&parameters)?;
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "UPDATE simulations SET name = ?1, description = ?2, parameters = ?3, portfolio_id = ?4, updated_at = ?5 WHERE id = ?6",
        rusqlite::params![name, description, params_json, portfolio_id, now, id],
    )?;

    Ok(Json(SavedSimulation {
        id,
        name,
        description,
        parameters,
        portfolio_id,
        created_at,
        updated_at: now,
    }))
}

pub async fn delete_simulation(
    State(db): State<DbConn>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let conn = db.lock()?;

    // Delete associated runs first (cascade should handle this, but be explicit)
    conn.execute(
        "DELETE FROM simulation_runs WHERE simulation_id = ?1",
        [&id],
    )?;

    let affected = conn.execute("DELETE FROM simulations WHERE id = ?1", [&id])?;

    if affected == 0 {
        Err(ApiError::SimulationNotFound(id))
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

// ============================================================================
// Simulation Run Handlers
// ============================================================================

pub async fn run_saved_simulation(
    State(db): State<DbConn>,
    Path(id): Path<String>,
    Json(req): Json<RunSimulationRequest>,
) -> ApiResult<Json<AggregatedResult>> {
    // Validate iterations
    validation::validate_iterations(req.iterations)?;

    // Get the simulation parameters
    let params = {
        let conn = db.lock()?;
        let mut stmt = conn.prepare("SELECT parameters FROM simulations WHERE id = ?1")?;

        let params_json: String = stmt
            .query_row([&id], |row| row.get(0))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ApiError::SimulationNotFound(id.clone()),
                _ => ApiError::from(e),
            })?;

        serde_json::from_str::<SimulationConfig>(&params_json)?
    };

    let iterations = req.iterations;
    let result = tokio::task::spawn_blocking(move || monte_carlo_simulate(&params, iterations))
        .await
        .map_err(|_| ApiError::InternalError)?;

    let aggregated = aggregate_results(result);

    // Save the run
    let run_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let result_json = serde_json::to_string(&aggregated)?;

    {
        let conn = db.lock()?;
        conn.execute(
            "INSERT INTO simulation_runs (id, simulation_id, result, iterations, ran_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![run_id, id, result_json, iterations as i32, now],
        )?;
    }

    Ok(Json(aggregated))
}

pub async fn run_simulation(
    Json(params): Json<SimulationConfig>,
) -> ApiResult<Json<AggregatedResult>> {
    // Validate parameters
    validation::validate_simulation_params(&params)?;

    let result = tokio::task::spawn_blocking(move || monte_carlo_simulate(&params, 100))
        .await
        .map_err(|_| ApiError::InternalError)?;

    let aggregated = aggregate_results(result);
    Ok(Json(aggregated))
}

pub async fn list_simulation_runs(
    State(db): State<DbConn>,
    Path(simulation_id): Path<String>,
) -> ApiResult<Json<Vec<SimulationRunRecord>>> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare("SELECT id, simulation_id, iterations, ran_at FROM simulation_runs WHERE simulation_id = ?1 ORDER BY ran_at DESC")?;

    let runs = stmt
        .query_map([&simulation_id], |row| {
            Ok(SimulationRunRecord {
                id: row.get(0)?,
                simulation_id: row.get(1)?,
                iterations: row.get(2)?,
                ran_at: row.get(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(runs))
}

pub async fn get_simulation_run(
    State(db): State<DbConn>,
    Path(id): Path<String>,
) -> ApiResult<Json<AggregatedResult>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare("SELECT result FROM simulation_runs WHERE id = ?1")?;

    let result_json: String = stmt
        .query_row([&id], |row| row.get(0))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ApiError::SimulationRunNotFound(id.clone()),
            _ => ApiError::from(e),
        })?;

    let result: AggregatedResult = serde_json::from_str(&result_json)?;

    Ok(Json(result))
}

// ============================================================================
// Aggregation Types and Functions
// ============================================================================

#[derive(Serialize, Deserialize)]
pub struct AggregatedResult {
    pub accounts: HashMap<AccountId, Vec<TimePointStats>>,
    pub total_portfolio: Vec<TimePointStats>,
    /// Growth components broken down by year (aggregated from ledger entries)
    pub growth_components: Vec<YearlyGrowthComponents>,
}

#[derive(Serialize, Deserialize)]
pub struct TimePointStats {
    pub date: String,
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct YearlyGrowthComponents {
    pub year: i32,
    /// Positive investment returns (cash appreciation)
    pub investment_returns: f64,
    /// Negative returns (losses)
    pub losses: f64,
    /// Cash inflows (credits)
    pub contributions: f64,
    /// Cash outflows (debits)
    pub cash_flow_expenses: f64,
    /// Asset sales
    pub withdrawals: f64,
}

/// Calculate the balance of an account at a specific date by replaying ledger entries
fn calculate_balance_at_date(
    result: &SimulationResult,
    account_id: AccountId,
    target_date: Date,
) -> f64 {
    // Start with initial value from account snapshot
    let mut balance = result
        .accounts
        .iter()
        .find(|a| a.account_id == account_id)
        .map(|a| a.starting_balance())
        .unwrap_or(0.0);

    // Replay all ledger entries up to target date
    for entry in &result.ledger {
        if entry.date > target_date {
            continue;
        }

        match &entry.event {
            StateEvent::CashCredit { to, amount } if *to == account_id => {
                balance += amount;
            }
            StateEvent::CashDebit { from, amount } if *from == account_id => {
                balance -= amount;
            }
            StateEvent::CashAppreciation {
                account_id: acc_id,
                new_value,
                previous_value,
                ..
            } if *acc_id == account_id => {
                balance += new_value - previous_value;
            }
            StateEvent::AssetPurchase {
                account_id: acc_id,
                cost_basis,
                ..
            } if *acc_id == account_id => {
                // Asset purchase uses cash to buy assets (cash goes down, asset value added)
                // The cost_basis represents what was spent
                balance -= cost_basis;
            }
            StateEvent::AssetSale {
                account_id: acc_id,
                proceeds,
                ..
            } if *acc_id == account_id => {
                // Asset sale adds proceeds to account
                balance += proceeds;
            }
            _ => {}
        }
    }

    balance
}

fn aggregate_results(mc_result: MonteCarloResult) -> AggregatedResult {
    let mut account_values: HashMap<AccountId, HashMap<Date, Vec<f64>>> = HashMap::new();
    let mut portfolio_values: HashMap<Date, Vec<f64>> = HashMap::new();

    // Growth components aggregation across all iterations (keyed by year)
    let mut growth_by_year: HashMap<i32, YearlyGrowthComponents> = HashMap::new();
    let num_iterations = mc_result.iterations.len() as f64;

    for sim_result in mc_result.iterations {
        let mut iteration_portfolio: HashMap<Date, f64> = HashMap::new();

        // Build time series by replaying ledger entries for each date
        for account in &sim_result.accounts {
            for date in &sim_result.dates {
                // Calculate balance at this date by replaying ledger entries up to this date
                let balance = calculate_balance_at_date(&sim_result, account.account_id, *date);

                // Account aggregation
                account_values
                    .entry(account.account_id)
                    .or_default()
                    .entry(*date)
                    .or_default()
                    .push(balance);

                // Portfolio aggregation (summing up for this iteration)
                *iteration_portfolio.entry(*date).or_default() += balance;
            }
        }

        // Add iteration totals to global portfolio stats
        for (date, total) in iteration_portfolio {
            portfolio_values.entry(date).or_default().push(total);
        }

        // Aggregate ledger entries by type
        for entry in &sim_result.ledger {
            let year = entry.date.year() as i32;
            let growth_entry =
                growth_by_year
                    .entry(year)
                    .or_insert_with(|| YearlyGrowthComponents {
                        year,
                        ..Default::default()
                    });

            match &entry.event {
                StateEvent::CashAppreciation {
                    new_value,
                    previous_value,
                    ..
                } => {
                    let return_amount = new_value - previous_value;
                    if return_amount >= 0.0 {
                        growth_entry.investment_returns += return_amount / num_iterations;
                    } else {
                        growth_entry.losses += return_amount / num_iterations;
                    }
                }
                StateEvent::CashCredit { amount, .. } => {
                    growth_entry.contributions += amount / num_iterations;
                }
                StateEvent::CashDebit { amount, .. } => {
                    growth_entry.cash_flow_expenses -= amount / num_iterations;
                }
                StateEvent::AssetSale { proceeds, .. } => {
                    growth_entry.withdrawals += proceeds / num_iterations;
                }
                _ => {}
            }
        }
    }

    let process_stats = |values: HashMap<Date, Vec<f64>>| -> Vec<TimePointStats> {
        let mut stats: Vec<TimePointStats> = values
            .into_iter()
            .map(|(date, mut vals)| {
                vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let len = vals.len();
                if len == 0 {
                    return TimePointStats {
                        date: date.to_string(),
                        p10: 0.0,
                        p50: 0.0,
                        p90: 0.0,
                    };
                }
                let p10 = vals[len / 10];
                let p50 = vals[len / 2];
                let p90 = vals[len * 9 / 10];
                TimePointStats {
                    date: date.to_string(),
                    p10,
                    p50,
                    p90,
                }
            })
            .collect();
        stats.sort_by(|a, b| a.date.cmp(&b.date));
        stats
    };

    let mut accounts_result = HashMap::new();
    for (id, values) in account_values {
        accounts_result.insert(id, process_stats(values));
    }

    // Sort growth components by year
    let mut growth_components: Vec<YearlyGrowthComponents> = growth_by_year.into_values().collect();
    growth_components.sort_by(|a, b| a.year.cmp(&b.year));

    AggregatedResult {
        accounts: accounts_result,
        total_portfolio: process_stats(portfolio_values),
        growth_components,
    }
}

// ============================================================================
// Builder-Style Simulation Creation
// ============================================================================

/// Create simulation from builder-style request (name-based, no IDs)
pub async fn create_simulation_from_request(
    State(db): State<DbConn>,
    Json(req): Json<crate::api_types::SimulationRequest>,
) -> ApiResult<Json<SavedSimulation>> {
    // Validate name
    validation::validate_simulation_name(&req.name)?;

    // Extract fields before consuming req
    let name = req.name.clone();
    let description = req.description.clone();

    // Convert to SimulationConfig using the builder
    let (config, metadata) = req.build()?;

    // Convert to SimulationConfig for storage
    let parameters = crate::api_conversion::config_to_parameters(&config, &metadata)?;

    // Store using existing logic
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let params_json = serde_json::to_string(&parameters)?;

    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO simulations (id, name, description, parameters, portfolio_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, &name, description, params_json, Option::<String>::None, &now, &now],
    )?;

    Ok(Json(SavedSimulation {
        id,
        name,
        description,
        parameters,
        portfolio_id: None,
        created_at: now.clone(),
        updated_at: now,
    }))
}
