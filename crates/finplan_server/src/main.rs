use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use finplan::models::{AccountId, MonteCarloResult, SimulationParameters, SimulationResult};
use finplan::simulation::monte_carlo_simulate;
use jiff::civil::Date;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

// Database connection wrapper
type DbConn = Arc<Mutex<Connection>>;

fn init_db(conn: &Connection) {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS simulations (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            parameters TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )
    .expect("Failed to create simulations table");

    conn.execute(
        "CREATE TABLE IF NOT EXISTS simulation_runs (
            id TEXT PRIMARY KEY,
            simulation_id TEXT NOT NULL,
            result TEXT NOT NULL,
            iterations INTEGER NOT NULL,
            ran_at TEXT NOT NULL,
            FOREIGN KEY (simulation_id) REFERENCES simulations(id) ON DELETE CASCADE
        )",
        [],
    )
    .expect("Failed to create simulation_runs table");
}

#[tokio::main]
async fn main() {
    let conn = Connection::open("finplan.db").expect("Failed to open database");
    init_db(&conn);
    let db: DbConn = Arc::new(Mutex::new(conn));

    let app = Router::new()
        .route("/", get(|| async { "FinPlan API Server" }))
        // Simulation CRUD
        .route("/api/simulations", get(list_simulations))
        .route("/api/simulations", post(create_simulation))
        .route("/api/simulations/{id}", get(get_simulation))
        .route("/api/simulations/{id}", put(update_simulation))
        .route("/api/simulations/{id}", delete(delete_simulation))
        // Run simulation
        .route("/api/simulations/{id}/run", post(run_saved_simulation))
        .route("/api/simulate", post(run_simulation))
        // Simulation runs history
        .route("/api/simulations/{id}/runs", get(list_simulation_runs))
        .route("/api/runs/{id}", get(get_simulation_run))
        .with_state(db)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// ============================================================================
// Simulation CRUD Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct SavedSimulation {
    id: String,
    name: String,
    description: Option<String>,
    parameters: SimulationParameters,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SimulationListItem {
    id: String,
    name: String,
    description: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct CreateSimulationRequest {
    name: String,
    description: Option<String>,
    parameters: SimulationParameters,
}

#[derive(Debug, Deserialize)]
struct UpdateSimulationRequest {
    name: Option<String>,
    description: Option<String>,
    parameters: Option<SimulationParameters>,
}

#[derive(Debug, Deserialize)]
struct RunSimulationRequest {
    #[serde(default = "default_iterations")]
    iterations: usize,
}

fn default_iterations() -> usize {
    100
}

#[derive(Debug, Serialize)]
struct SimulationRunRecord {
    id: String,
    simulation_id: String,
    iterations: i32,
    ran_at: String,
}

// ============================================================================
// Simulation CRUD Handlers
// ============================================================================

async fn list_simulations(State(db): State<DbConn>) -> Json<Vec<SimulationListItem>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, name, description, created_at, updated_at FROM simulations ORDER BY updated_at DESC")
        .unwrap();

    let simulations = stmt
        .query_map([], |row| {
            Ok(SimulationListItem {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(simulations)
}

async fn create_simulation(
    State(db): State<DbConn>,
    Json(req): Json<CreateSimulationRequest>,
) -> Result<Json<SavedSimulation>, StatusCode> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let params_json =
        serde_json::to_string(&req.parameters).map_err(|_| StatusCode::BAD_REQUEST)?;

    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO simulations (id, name, description, parameters, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, req.name, req.description, params_json, now, now],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(SavedSimulation {
        id,
        name: req.name,
        description: req.description,
        parameters: req.parameters,
        created_at: now.clone(),
        updated_at: now,
    }))
}

async fn get_simulation(
    State(db): State<DbConn>,
    Path(id): Path<String>,
) -> Result<Json<SavedSimulation>, StatusCode> {
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, name, description, parameters, created_at, updated_at FROM simulations WHERE id = ?1")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let simulation = stmt
        .query_row([&id], |row| {
            let params_json: String = row.get(3)?;
            let parameters: SimulationParameters =
                serde_json::from_str(&params_json).map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(SavedSimulation {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                parameters,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(simulation))
}

async fn update_simulation(
    State(db): State<DbConn>,
    Path(id): Path<String>,
    Json(req): Json<UpdateSimulationRequest>,
) -> Result<Json<SavedSimulation>, StatusCode> {
    let conn = db.lock().unwrap();

    // Get existing simulation
    let mut stmt = conn
        .prepare("SELECT name, description, parameters, created_at FROM simulations WHERE id = ?1")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (current_name, current_desc, current_params_json, created_at): (
        String,
        Option<String>,
        String,
        String,
    ) = stmt
        .query_row([&id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let name = req.name.unwrap_or(current_name);
    let description = req.description.or(current_desc);
    let parameters = if let Some(p) = req.parameters {
        p
    } else {
        serde_json::from_str(&current_params_json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let params_json = serde_json::to_string(&parameters).map_err(|_| StatusCode::BAD_REQUEST)?;
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "UPDATE simulations SET name = ?1, description = ?2, parameters = ?3, updated_at = ?4 WHERE id = ?5",
        rusqlite::params![name, description, params_json, now, id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(SavedSimulation {
        id,
        name,
        description,
        parameters,
        created_at,
        updated_at: now,
    }))
}

async fn delete_simulation(
    State(db): State<DbConn>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = db.lock().unwrap();

    // Delete associated runs first
    conn.execute(
        "DELETE FROM simulation_runs WHERE simulation_id = ?1",
        [&id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let affected = conn
        .execute("DELETE FROM simulations WHERE id = ?1", [&id])
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if affected == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

// ============================================================================
// Simulation Run Handlers
// ============================================================================

async fn run_saved_simulation(
    State(db): State<DbConn>,
    Path(id): Path<String>,
    Json(req): Json<RunSimulationRequest>,
) -> Result<Json<AggregatedResult>, StatusCode> {
    // Get the simulation parameters
    let params = {
        let conn = db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT parameters FROM simulations WHERE id = ?1")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let params_json: String = stmt
            .query_row([&id], |row| row.get(0))
            .map_err(|_| StatusCode::NOT_FOUND)?;

        serde_json::from_str::<SimulationParameters>(&params_json)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let iterations = req.iterations;
    let result = tokio::task::spawn_blocking(move || monte_carlo_simulate(&params, iterations))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let aggregated = aggregate_results(result);

    // Save the run
    let run_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let result_json =
        serde_json::to_string(&aggregated).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO simulation_runs (id, simulation_id, result, iterations, ran_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![run_id, id, result_json, iterations as i32, now],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(aggregated))
}

async fn run_simulation(Json(params): Json<SimulationParameters>) -> Json<AggregatedResult> {
    let result = tokio::task::spawn_blocking(move || monte_carlo_simulate(&params, 100))
        .await
        .unwrap();

    let aggregated = aggregate_results(result);
    Json(aggregated)
}

async fn list_simulation_runs(
    State(db): State<DbConn>,
    Path(simulation_id): Path<String>,
) -> Json<Vec<SimulationRunRecord>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, simulation_id, iterations, ran_at FROM simulation_runs WHERE simulation_id = ?1 ORDER BY ran_at DESC")
        .unwrap();

    let runs = stmt
        .query_map([&simulation_id], |row| {
            Ok(SimulationRunRecord {
                id: row.get(0)?,
                simulation_id: row.get(1)?,
                iterations: row.get(2)?,
                ran_at: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(runs)
}

async fn get_simulation_run(
    State(db): State<DbConn>,
    Path(id): Path<String>,
) -> Result<Json<AggregatedResult>, StatusCode> {
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT result FROM simulation_runs WHERE id = ?1")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result_json: String = stmt
        .query_row([&id], |row| row.get(0))
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let result: AggregatedResult =
        serde_json::from_str(&result_json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(result))
}

// ============================================================================
// Aggregation Types and Functions
// ============================================================================

#[derive(Serialize, Deserialize)]
struct AggregatedResult {
    accounts: HashMap<AccountId, Vec<TimePointStats>>,
    total_portfolio: Vec<TimePointStats>,
    /// Growth components broken down by year (aggregated from transaction logs)
    growth_components: Vec<YearlyGrowthComponents>,
}

#[derive(Serialize, Deserialize)]
struct TimePointStats {
    date: String,
    p10: f64,
    p50: f64,
    p90: f64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct YearlyGrowthComponents {
    year: i32,
    /// Positive investment returns
    investment_returns: f64,
    /// Negative returns (losses and debt interest)
    losses: f64,
    /// Cash inflows (income via CashFlow)
    contributions: f64,
    /// Cash outflows via CashFlow (expenses, NOT spending targets)
    cash_flow_expenses: f64,
    /// Withdrawals via SpendingTarget
    withdrawals: f64,
}

/// Calculate the balance of an account at a specific date by replaying transactions
fn calculate_balance_at_date(result: &SimulationResult, account_id: AccountId, target_date: Date) -> f64 {
    // Start with initial value
    let mut balance = result.accounts.iter()
        .find(|a| a.account_id == account_id)
        .map(|a| a.starting_balance())
        .unwrap_or(0.0);

    // Replay cash flows up to target date
    for cf in &result.cash_flow_history {
        if cf.account_id == account_id && cf.date <= target_date {
            balance += cf.amount;
        }
    }

    // Replay returns up to target date
    for ret in &result.return_history {
        if ret.account_id == account_id && ret.date <= target_date {
            balance += ret.return_amount;
        }
    }

    // Replay transfers up to target date
    for transfer in &result.transfer_history {
        if transfer.date <= target_date {
            if transfer.from_account_id == account_id {
                balance -= transfer.amount;
            }
            if transfer.to_account_id == account_id {
                balance += transfer.amount;
            }
        }
    }

    // Replay withdrawals up to target date
    for withdrawal in &result.withdrawal_history {
        if withdrawal.account_id == account_id && withdrawal.date <= target_date {
            balance -= withdrawal.gross_amount;
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

        // Build time series by replaying transactions for each date
        for account in &sim_result.accounts {
            for date in &sim_result.dates {
                // Calculate balance at this date by replaying transactions up to this date
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
        
        // Aggregate return history
        for ret in &sim_result.return_history {
            let year = ret.date.year() as i32;
            let entry = growth_by_year.entry(year).or_insert_with(|| YearlyGrowthComponents {
                year,
                ..Default::default()
            });
            if ret.return_amount >= 0.0 {
                entry.investment_returns += ret.return_amount / num_iterations;
            } else {
                entry.losses += ret.return_amount / num_iterations; // Already negative
            }
        }
        
        // Aggregate cash flow history
        for cf in &sim_result.cash_flow_history {
            let year = cf.date.year() as i32;
            let entry = growth_by_year.entry(year).or_insert_with(|| YearlyGrowthComponents {
                year,
                ..Default::default()
            });
            if cf.amount >= 0.0 {
                entry.contributions += cf.amount / num_iterations;
            } else {
                entry.cash_flow_expenses += cf.amount / num_iterations; // Already negative
            }
        }
        
        // Aggregate withdrawal history
        for wd in &sim_result.withdrawal_history {
            let year = wd.date.year() as i32;
            let entry = growth_by_year.entry(year).or_insert_with(|| YearlyGrowthComponents {
                year,
                ..Default::default()
            });
            entry.withdrawals -= wd.gross_amount / num_iterations; // Negative (outflow)
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
