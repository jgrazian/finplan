//! Golden equivalence: a run's results read back from the `run_*` tables (the
//! SQL read path) must equal what `finplan_plan::results::project` builds
//! straight from the engine's summary.
//!
//! The run goes through the real pipeline (enqueue, runner, `store::persist`,
//! `GET /runs/{id}/results` and `/ledger`). The projection is then built from
//! the very inputs the run captured: its snapshot is compiled again and the
//! same seeded Monte Carlo is run in-process, which is deterministic for a
//! seed. Every response body is compared as a `serde_json::Value`, so floats
//! must be bit-equal and any rounding is part of the comparison.
//!
//! Nothing is excluded: the run id and scenario id are arguments of the
//! projection's `results`, and the endpoints carry no timestamps.

use finplan_core::model::{ConvergenceConfig, MonteCarloConfig};
use finplan_core::simulation::monte_carlo_simulate_with_config;
use finplan_plan::compile::{CompiledScenario, compile};
use finplan_plan::graph::ScenarioGraph;
use finplan_plan::results::{RunResults, RunSettings, project};

use super::*;

/// The first place two JSON values differ, as a path, so a failure on a body of
/// thousands of numbers says where.
fn first_difference(path: &str, got: &Value, want: &Value) -> Option<String> {
    match (got, want) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()) {
                let here = format!("{path}.{key}");
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => {
                        if let Some(found) = first_difference(&here, x, y) {
                            return Some(found);
                        }
                    }
                    (x, y) => return Some(format!("{here}: server {x:?}, projection {y:?}")),
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!(
                    "{path}: server has {} items, projection {}",
                    a.len(),
                    b.len()
                ));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (x, y))| first_difference(&format!("{path}[{i}]"), x, y))
        }
        _ if got == want => None,
        _ => Some(format!("{path}: server {got}, projection {want}")),
    }
}

/// A projection as the test sees a response body: serialised, then parsed by
/// the same `serde_json` that parsed the body. Without its `float_roundtrip`
/// feature that parser can land a ULP off on some doubles, so the server's
/// exact text and the projection's exact value would differ after parsing even
/// when they are the same number. Parsing both texts the same way compares
/// exactly the text the server sent.
fn as_served<T: serde::Serialize>(value: &T) -> Value {
    serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap()
}

fn assert_same(what: &str, server: &Value, projection: &Value) {
    if server != projection {
        panic!(
            "{what} differs between the SQL read path and the projection at {}",
            first_difference("$", server, projection).unwrap_or_default()
        );
    }
}

/// A finished run and the projection of the same inputs.
struct Golden {
    run_id: i64,
    scenario_id: i64,
    projection: RunResults,
}

/// The compile and Monte Carlo configuration the server captured for a run.
async fn captured_inputs(app: &TestApp, run_id: i64) -> (CompiledScenario, MonteCarloConfig) {
    let (status, inputs) = app.get(&format!("/api/runs/{run_id}/inputs")).await;
    assert_eq!(status, StatusCode::OK, "{inputs}");
    let graph: ScenarioGraph = serde_json::from_value(inputs["snapshot"].clone()).unwrap();
    let compiled = compile(&graph).expect("the captured inputs compile");

    let converge = inputs["converge"].as_bool().unwrap();
    let iterations = inputs["iterations"].as_u64().unwrap() as usize;
    let percentiles: Vec<f64> = inputs["percentiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_f64().unwrap())
        .collect();
    // Built the way `runner::execute` builds it.
    let config = MonteCarloConfig {
        iterations,
        percentiles,
        compute_mean: inputs["compute_mean"].as_bool().unwrap(),
        convergence: converge.then(|| ConvergenceConfig {
            max_iterations: inputs["max_iterations"]
                .as_u64()
                .map_or(iterations, |m| m as usize),
            relative_threshold: 0.01,
            ..ConvergenceConfig::default()
        }),
        batch_size: inputs["batch_size"].as_u64().unwrap() as usize,
        parallel_batches: inputs["parallel_batches"].as_u64().unwrap() as usize,
        seed: inputs["seed"].as_i64().map(|s| s as u64),
    };
    (compiled, config)
}

/// Enqueue `body` and wait for the run to succeed.
async fn run_to_success(app: &TestApp, scenario_id: i64, body: Value) -> i64 {
    let (status, run) = app
        .post(&format!("/api/scenarios/{scenario_id}/runs"), body)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");
    run_id
}

/// Run `body` against `scenario_id` through the server, then project the same
/// run in-process from the inputs the server captured for it: compile the
/// snapshot and run the same seeded Monte Carlo again.
///
/// This compares two simulations, so it needs the engine to reproduce a seed
/// to the bit. It does for plans whose accounts hold one position each, but
/// not in general (see [`run_and_repersist`]): a plan with several positions in
/// an account sums them in an order that can differ between runs, and the last
/// digit of a balance then differs too.
async fn run_and_project(app: &TestApp, scenario_id: i64, body: Value) -> Golden {
    let run_id = run_to_success(app, scenario_id, body).await;
    let (compiled, config) = captured_inputs(app, run_id).await;
    let summary = monte_carlo_simulate_with_config(&compiled.config, &config).expect("simulates");
    Golden {
        run_id,
        scenario_id,
        projection: project(&compiled, &summary, &RunSettings::default()),
    }
}

/// [`run_and_project`] for a plan the engine does not reproduce bit for bit:
/// after the run has gone through the real pipeline, its results are written
/// again, with the real `store::persist`, from a summary this test holds, and
/// the served results are compared with the projection of that same summary.
/// What is compared is then exactly the persist-then-read path against
/// `project`, with no second simulation to disagree in the last digit.
async fn run_and_repersist(
    app: &TestApp,
    db: &finplan_server::db::Db,
    scenario_id: i64,
    body: Value,
) -> Golden {
    let run_id = run_to_success(app, scenario_id, body).await;
    let (compiled, config) = captured_inputs(app, run_id).await;
    let summary = monte_carlo_simulate_with_config(&compiled.config, &config).expect("simulates");
    let projection = project(&compiled, &summary, &RunSettings::default());

    // `persist` claims a running run and replaces what the run stored.
    sqlx::query("UPDATE runs SET status = 'running' WHERE id = ?1")
        .bind(run_id)
        .execute(db)
        .await
        .unwrap();
    finplan_server::runner::store::persist(db, run_id, &projection)
        .await
        .unwrap();
    Golden {
        run_id,
        scenario_id,
        projection,
    }
}

/// A test app that also hands out the server state, for its database.
pub(super) async fn app_with_state() -> (TestApp, finplan_server::state::AppState) {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = ServerConfig {
        mail: Default::default(),
        review_ai: Default::default(),
        draft: finplan_server::suggest::ai::DraftConfig {
            temp_dir: Some(dir.path().join("draft-files")),
            ..Default::default()
        },
        plan_chat: Default::default(),
        log_format: Default::default(),
        metrics_bind: None,
        bind: "127.0.0.1:0".into(),
        database_url: format!("sqlite://{}", dir.path().join("test.db").display()),
        db_pool_size: 4,
        sim_workers: 1,
        max_iterations: 50_000,
        secure_cookies: false,
        hosted: false,
        access_mode: Default::default(),
        registration_open: true,
        guest_access: true,
        guest_max_iterations: 100,
        guest_retention_days: 30,
        local_mode: true,
        offload: Default::default(),
        local_mail_sink: None,
        cors_origins: vec!["http://localhost:3000".into()],
    };
    let (router, state) = finplan_server::build_with(config, None)
        .await
        .expect("build app");
    (
        TestApp {
            router,
            cookie: None,
            _dir: dir,
        },
        state,
    )
}

impl Golden {
    /// `GET /runs/{id}/results[?series=..]` against `RunResults::results`.
    async fn check_results(&self, app: &TestApp, series: Option<&str>) -> Value {
        let path = match series {
            Some(series) => format!("/api/runs/{}/results?series={series}", self.run_id),
            None => format!("/api/runs/{}/results", self.run_id),
        };
        let (status, served) = app.get(&path).await;
        let what = format!("results (series {series:?})");
        match self
            .projection
            .results(self.run_id, self.scenario_id, series)
        {
            Ok(results) => {
                assert_eq!(status, StatusCode::OK, "{what}: {served}");
                assert_same(&what, &served, &as_served(&results));
            }
            Err(error) => {
                assert!(status.is_client_error(), "{what}: {status} {served}");
                assert_eq!(served["error"]["message"], error.to_string(), "{what}");
            }
        }
        served
    }

    /// `GET /runs/{id}/ledger?..` against `RunResults::ledger_page`.
    async fn check_ledger(
        &self,
        app: &TestApp,
        series: Option<&str>,
        year: Option<i64>,
        category: Option<&str>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) {
        let mut query = Vec::new();
        if let Some(series) = series {
            query.push(format!("series={series}"));
        }
        if let Some(year) = year {
            query.push(format!("year={year}"));
        }
        if let Some(category) = category {
            query.push(format!("category={category}"));
        }
        if let Some(limit) = limit {
            query.push(format!("limit={limit}"));
        }
        if let Some(offset) = offset {
            query.push(format!("offset={offset}"));
        }
        let (status, served) = app
            .get(&format!(
                "/api/runs/{}/ledger?{}",
                self.run_id,
                query.join("&")
            ))
            .await;
        let what = format!("ledger ({})", query.join("&"));
        match self
            .projection
            .ledger_page(self.run_id, series, year, category, limit, offset)
        {
            Ok(page) => {
                assert_eq!(status, StatusCode::OK, "{what}: {served}");
                assert_same(&what, &served, &as_served(&page));
            }
            Err(error) => {
                assert!(status.is_client_error(), "{what}: {status} {served}");
                assert_eq!(served["error"]["message"], error.to_string(), "{what}");
            }
        }
    }

    /// Everything the read path serves for this run, against the projection.
    async fn check_everything(&self, app: &TestApp) {
        let default = self.check_results(app, None).await;
        for series in [
            "mean", "0.1", "0.5", "0.9", "0.05", "0.97", "nonsense", "1.5",
        ] {
            self.check_results(app, Some(series)).await;
        }

        // The ledger, a year and a bucket at a time, over the paths stored.
        let years: Vec<i64> = default["ledger_years"]
            .as_array()
            .unwrap()
            .iter()
            .map(|y| y["year"].as_i64().unwrap())
            .collect();
        for series in [None, Some("mean"), Some("0.1"), Some("0.9")] {
            self.check_ledger(app, series, None, None, None, None).await;
            self.check_ledger(app, series, None, None, Some(7), Some(3))
                .await;
            self.check_ledger(app, series, None, Some("nonsense"), None, None)
                .await;
            for category in ["cash", "asset", "tax", "event"] {
                self.check_ledger(app, series, None, Some(category), Some(40), None)
                    .await;
            }
        }
        for year in years.iter().step_by(3.max(years.len() / 8)) {
            self.check_ledger(app, None, Some(*year), None, None, None)
                .await;
            self.check_ledger(app, None, Some(*year), Some("cash"), None, Some(1))
                .await;
        }
        self.check_ledger(app, None, Some(1900), None, Some(0), Some(-5))
            .await;
    }
}

/// The anonymised default plan (`finplan_plan/testdata`), imported through the
/// archive route so it is an ordinary scenario, then run.
#[tokio::test]
async fn the_default_snapshot_projects_to_the_served_results() {
    let (mut app, state) = app_with_state().await;
    app.login_as("golden-default@example.com").await;
    let graph: Value = serde_json::from_str(include_str!(
        "../../../finplan_plan/testdata/default_snapshot.json"
    ))
    .unwrap();
    let (status, imported) = app
        .post(
            "/api/archives/import",
            json!({
                "archive": {"format": "finplan.inputs", "version": 3, "plans": [graph]},
                "name_prefix": "Golden ",
                "request_id": "golden-default",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let scenario_id = imported["scenario_ids"][0].as_i64().unwrap();

    // Its accounts hold several positions, which the engine does not sum in a
    // repeatable order, so the run's results are rewritten from a held summary.
    let golden = run_and_repersist(
        &app,
        &state.db,
        scenario_id,
        json!({"iterations": 150, "seed": 42, "percentiles": [0.1, 0.5, 0.9]}),
    )
    .await;
    golden.check_everything(&app).await;

    let served = golden.check_results(&app, None).await;
    assert_eq!(
        served["bands"].as_array().unwrap().len(),
        4,
        "mean and three"
    );
    // The percentile rows keep the seed that replays each path, and the median
    // helper finds the 50th's.
    let band_seed = |series: &str| {
        served["bands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["path_id"] == series)
            .unwrap()["seed"]
            .clone()
    };
    assert!(band_seed("mean").is_null());
    let median = band_seed("0.5");
    let median = median.as_str().expect("the median band carries its seed");
    assert_eq!(
        finplan_server::db::median_seed(&state.db, golden.run_id)
            .await
            .unwrap(),
        Some(median.parse().unwrap())
    );
    assert!(!served["ledger_years"].as_array().unwrap().is_empty());
    assert!(!served["real_net_worth"].is_null());
    assert!(!served["funding_diagnostics"].is_null());
}

/// A scenario built through the API, with events that leave a ledger worth
/// reading: income, one-offs that tag a year, and a spend large enough to leave
/// the plan short of cash so there are warnings and funding diagnostics.
async fn plan_with_a_history(app: &TestApp) -> i64 {
    let (scenario_id, checking, brokerage) = app.seed_scenario().await;
    for event in [
        json!({
            "name": "Salary",
            "trigger": {"kind": "Repeating", "interval": "Monthly"},
            "effects": [{
                "kind": "Income", "to_account_id": checking, "income_type": "Taxable",
                "amount": {"kind": "Fixed", "value": 5000.0}
            }]
        }),
        json!({
            "name": "Buy the boat",
            "fires_once": true,
            "trigger": {"kind": "Age", "years": 45},
            "effects": [{
                "kind": "Expense", "from_account_id": checking,
                "amount": {"kind": "Fixed", "value": 20_000.0}
            }]
        }),
        json!({
            "name": "Sell the car",
            "fires_once": true,
            "trigger": {"kind": "Age", "years": 45},
            "effects": [{
                "kind": "Income", "to_account_id": checking, "income_type": "TaxFree",
                "amount": {"kind": "Fixed", "value": 8_000.0}
            }]
        }),
        json!({
            "name": "Move into the brokerage",
            "fires_once": true,
            "trigger": {"kind": "Age", "years": 39},
            "effects": [{
                "kind": "CashTransfer", "from_account_id": checking, "to_account_id": brokerage,
                "amount": {"kind": "Fixed", "value": 5_000.0}
            }]
        }),
        json!({
            "name": "Draw down the brokerage",
            "trigger": {"kind": "Repeating", "interval": "Monthly", "start_condition": {"kind": "Age", "years": 50}},
            "effects": [{
                "kind": "Sweep", "to_account_id": checking, "income_type": "Taxable",
                "amount": {"kind": "Fixed", "value": 1_500.0}
            }]
        }),
        json!({
            "name": "Big house repair",
            "fires_once": true,
            "trigger": {"kind": "Age", "years": 41},
            "effects": [{
                "kind": "Expense", "from_account_id": checking,
                "amount": {"kind": "Fixed", "value": 400_000.0}
            }]
        }),
    ] {
        let (status, created) = app
            .post(&format!("/api/scenarios/{scenario_id}/events"), event)
            .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
    }
    scenario_id
}

#[tokio::test]
async fn an_api_built_plan_projects_to_the_served_results() {
    let mut app = TestApp::new().await;
    app.login_as("golden-api@example.com").await;
    let scenario_id = plan_with_a_history(&app).await;

    let golden = run_and_project(
        &app,
        scenario_id,
        json!({"iterations": 120, "seed": 7, "percentiles": [0.1, 0.5, 0.9]}),
    )
    .await;
    golden.check_everything(&app).await;

    // The history above has to have produced the parts worth comparing.
    let served = golden.check_results(&app, None).await;
    assert!(
        !served["warnings"].as_array().unwrap().is_empty(),
        "the large repair should leave the plan short of cash"
    );
    assert!(!served["ledger_years"].as_array().unwrap().is_empty());
    assert!(!served["funding_diagnostics"].is_null());
    assert!(!served["real_net_worth"].is_null());
}

/// Other run shapes: a converging run, a run with no mean, one percentile.
#[tokio::test]
async fn other_run_shapes_project_to_the_served_results() {
    let mut app = TestApp::new().await;
    app.login_as("golden-shapes@example.com").await;
    let scenario_id = plan_with_a_history(&app).await;

    for body in [
        json!({"iterations": 60, "seed": 1, "percentiles": [0.5], "compute_mean": false}),
        json!({"iterations": 50, "seed": 2, "percentiles": [0.05, 0.25, 0.5, 0.75, 0.95],
               "batch_size": 17, "parallel_batches": 3}),
        json!({"iterations": 100, "seed": 3, "converge": true}),
    ] {
        let golden = run_and_project(&app, scenario_id, body).await;
        golden.check_everything(&app).await;
    }
}

/// Dropping the ledger is the one setting the projection has; the rest of the
/// run is untouched.
#[tokio::test]
async fn a_run_projected_without_its_ledger_keeps_everything_else() {
    let mut app = TestApp::new().await;
    app.login_as("golden-no-ledger@example.com").await;
    let scenario_id = plan_with_a_history(&app).await;
    let golden = run_and_project(
        &app,
        scenario_id,
        json!({"iterations": 30, "seed": 5, "percentiles": [0.5]}),
    )
    .await;

    let (compiled, config) = captured_inputs(&app, golden.run_id).await;
    let summary = monte_carlo_simulate_with_config(&compiled.config, &config).unwrap();
    let bare = project(
        &compiled,
        &summary,
        &RunSettings {
            include_ledger: false,
        },
    );

    let full = serde_json::to_value(
        golden
            .projection
            .results(golden.run_id, golden.scenario_id, None)
            .unwrap(),
    )
    .unwrap();
    let mut without = serde_json::to_value(
        bare.results(golden.run_id, golden.scenario_id, None)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(without["ledger_years"], json!([]));
    without["ledger_years"] = full["ledger_years"].clone();
    assert_same("a ledgerless projection", &without, &full);
}

/// `PUT /scenarios/{id}/funding` writes the policy, and the scenario reads it back.
#[tokio::test]
async fn the_funding_policy_is_set_read_back_and_cleared() {
    let mut app = TestApp::new().await;
    app.login_as("funding-policy@example.com").await;
    let (scenario_id, checking, brokerage) = app.seed_scenario().await;
    let path = format!("/api/scenarios/{scenario_id}");
    let (_, scenario) = app.get(&path).await;
    assert!(scenario["funding"].is_null());

    let policy = json!({"funding": {"strategy": "BracketFilling", "bracket_ceiling": 0.22,
                                    "exclude_accounts": [brokerage, brokerage]}});
    let (status, set) = app.put(&format!("{path}/funding"), policy).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let expected = json!({"strategy": "BracketFilling", "bracket_ceiling": 0.22,
                          "exclude_accounts": [brokerage]});
    assert_eq!(set["funding"], expected);
    assert_eq!(app.get(&path).await.1["funding"], expected);
    let (status, report) = app.post(&format!("{path}/compile"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    // A bank account cannot be excluded, and a refusal leaves the policy alone.
    let (status, _) = app
        .put(
            &format!("{path}/funding"),
            json!({"funding": {"strategy": "ProRata", "exclude_accounts": [checking]}}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(app.get(&path).await.1["funding"], expected);

    let (status, off) = app
        .put(&format!("{path}/funding"), json!({"funding": null}))
        .await;
    assert_eq!(status, StatusCode::OK, "{off}");
    assert!(off["funding"].is_null());
}
