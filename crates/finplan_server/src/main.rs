use axum::{
    Json, Router,
    routing::{get, post},
};
use finplan::models::{MonteCarloResult, SimulationParameters};
use finplan::simulation::monte_carlo_simulate;
use jiff::civil::Date;
use serde::Serialize;
use std::collections::HashMap;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/api/simulate", post(run_simulation))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn run_simulation(Json(params): Json<SimulationParameters>) -> Json<AggregatedResult> {
    let result = tokio::task::spawn_blocking(move || monte_carlo_simulate(&params, 100))
        .await
        .unwrap();

    let aggregated = aggregate_results(result);
    Json(aggregated)
}

#[derive(Serialize)]
struct AggregatedResult {
    accounts: HashMap<u64, Vec<TimePointStats>>,
    total_portfolio: Vec<TimePointStats>,
}

#[derive(Serialize)]
struct TimePointStats {
    date: String,
    p10: f64,
    p50: f64,
    p90: f64,
}

fn aggregate_results(mc_result: MonteCarloResult) -> AggregatedResult {
    let mut account_values: HashMap<u64, HashMap<Date, Vec<f64>>> = HashMap::new();
    let mut portfolio_values: HashMap<Date, Vec<f64>> = HashMap::new();

    for sim_result in mc_result.iterations {
        let mut iteration_portfolio: HashMap<Date, f64> = HashMap::new();

        for history in sim_result.account_histories {
            for snapshot in history.values() {
                // Account aggregation
                account_values
                    .entry(history.account_id)
                    .or_default()
                    .entry(snapshot.date)
                    .or_default()
                    .push(snapshot.balance);

                // Portfolio aggregation (summing up for this iteration)
                *iteration_portfolio.entry(snapshot.date).or_default() += snapshot.balance;
            }
        }

        // Add iteration totals to global portfolio stats
        for (date, total) in iteration_portfolio {
            portfolio_values.entry(date).or_default().push(total);
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

    AggregatedResult {
        accounts: accounts_result,
        total_portfolio: process_stats(portfolio_values),
    }
}
