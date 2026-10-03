//! Whole-plan simulation of a draft: no run, no pairing.

use serde_json::{Value, json};

use super::*;
use crate::suggest::ChangeOp;
use crate::suggest::templates::{
    RowRef, allocation_asset, bank_account, investment_account, position,
};

async fn state() -> AppState {
    #[derive(clap::Parser)]
    struct Args {
        #[command(flatten)]
        config: crate::config::ServerConfig,
    }
    let mut config = <Args as clap::Parser>::parse_from(["draft-simulation-test"]).config;
    config.database_url = "sqlite::memory:".into();
    config.db_pool_size = 1;
    config.hosted = false;
    let (_, state) = crate::build(config).await.unwrap();
    state
}

struct Draft {
    state: AppState,
    user: CurrentUser,
    scenario: i64,
    profile: i64,
}

async fn scalar(db: &Db, sql: &str, binds: &[Value]) -> i64 {
    let mut q = sqlx::query_scalar::<_, i64>(sql);
    for b in binds {
        q = match b {
            Value::String(s) => q.bind(s.clone()),
            Value::Number(n) if n.is_i64() => q.bind(n.as_i64()),
            Value::Number(n) => q.bind(n.as_f64()),
            Value::Null => q.bind(None::<i64>),
            other => panic!("unsupported bind {other}"),
        };
    }
    q.fetch_one(db).await.unwrap()
}

impl Draft {
    async fn new(birth_date: Option<&str>) -> Self {
        let state = state().await;
        let id = "draft-user";
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES (?,?,'unused')")
            .bind(id)
            .bind("draft-user@example.test")
            .execute(&state.db)
            .await
            .unwrap();
        let u = json!(id);
        let dist = scalar(
            &state.db,
            "INSERT INTO distributions(user_id,kind,mean,std_dev) VALUES (?,'Normal',0.06,0.12) RETURNING id",
            std::slice::from_ref(&u),
        )
        .await;
        let profile = scalar(
            &state.db,
            "INSERT INTO return_profiles(user_id,name,distribution_id) VALUES (?,'Equity',?) RETURNING id",
            &[u.clone(), json!(dist)],
        )
        .await;
        let scenario = scalar(
            &state.db,
            "INSERT INTO scenarios(user_id,name,start_date,birth_date,duration_years,status)
             VALUES (?,'Draft','2026-01-01',?,40,'draft') RETURNING id",
            &[u, birth_date.map_or(Value::Null, |d| json!(d))],
        )
        .await;
        Draft {
            user: CurrentUser {
                id: id.into(),
                email: "draft-user@example.test".into(),
                session_id: "s".into(),
                guest: false,
            },
            state,
            scenario,
            profile,
        }
    }

    fn holdings(&self, cash: f64, invested: f64) -> Vec<Change> {
        vec![
            bank_account("checking", "Checking", cash, RowRef::Id(self.profile), None),
            allocation_asset("fund", "Fund", RowRef::Id(self.profile), None),
            investment_account(
                "k401",
                "401(k)",
                "TaxDeferred",
                RowRef::Id(self.profile),
                None,
                vec![position(&RowRef::new("fund"), invested, invested)],
            ),
        ]
    }

    async fn simulate(&self, steps: &[Vec<Change>]) -> DraftSimulation {
        simulate_draft(&self.state, &self.user, self.scenario, steps, Some(50))
            .await
            .unwrap()
    }
}

fn spending(amount: f64) -> Vec<Change> {
    serde_json::from_value::<crate::suggest::templates::TemplateRequest>(json!({
        "kind": "recurring_expense", "name": "Living", "from_account_id": {"$new": "checking"},
        "amount": amount, "fund_from_investments": true,
    }))
    .unwrap()
    .expand()
    .unwrap()
    .changes
}

#[tokio::test]
async fn a_draft_with_steps_applied_simulates_without_a_run() {
    let d = Draft::new(Some("1980-01-01")).await;
    // The steps are applied in memory: the stored draft stays empty.
    let comfortable = d
        .simulate(&[d.holdings(50_000., 900_000.), spending(40_000.)])
        .await;
    assert_eq!(comfortable.blocked, None);
    assert_eq!(comfortable.iterations, 50);
    let stats = comfortable.stats.unwrap();
    assert!((0.0..=1.0).contains(&stats.success_rate));
    assert!(stats.funding_success_rate.is_some());
    assert!(stats.real_final.is_some());
    let accounts: i64 = scalar(
        &d.state.db,
        "SELECT count(*) FROM accounts WHERE scenario_id=?",
        &[json!(d.scenario)],
    )
    .await;
    assert_eq!(accounts, 0);

    // Spending far past what the holdings can carry does worse.
    let strained = d
        .simulate(&[d.holdings(50_000., 900_000.), spending(400_000.)])
        .await;
    assert!(strained.stats.unwrap().success_rate < stats.success_rate);

    // Same plan, same answer: the seed is fixed.
    let again = d
        .simulate(&[d.holdings(50_000., 900_000.), spending(40_000.)])
        .await;
    assert_eq!(
        again.stats.unwrap().real_final.unwrap().p50,
        stats.real_final.unwrap().p50
    );
}

#[tokio::test]
async fn a_step_that_cannot_apply_is_a_result_naming_the_step() {
    let d = Draft::new(Some("1980-01-01")).await;
    let missing = vec![Change {
        op: ChangeOp::Remove,
        target: ChangeTarget::Event(9_999),
        path: String::new(),
        expect: None,
        value: None,
    }];
    let result = d.simulate(&[d.holdings(1_000., 0.), missing]).await;
    assert_eq!(result.iterations, 0);
    assert!(result.stats.is_none());
    match result.blocked {
        Some(DraftBlocked::Steps { step, problems }) => {
            assert_eq!(step, 1);
            assert!(matches!(problems[0], ChangeProblem::UnknownTarget { .. }));
        }
        other => panic!("expected a step problem, got {other:?}"),
    }
}

#[tokio::test]
async fn a_plan_that_does_not_compile_says_why() {
    // An age-based end with no birth date to count from.
    let d = Draft::new(None).await;
    let salary = serde_json::from_value::<crate::suggest::templates::TemplateRequest>(json!({
        "kind": "salary", "to_account_id": {"$new": "checking"}, "annual_amount": 80000,
        "end": {"kind": "Age", "years": 65},
    }))
    .unwrap()
    .expand()
    .unwrap()
    .changes;
    let result = d.simulate(&[d.holdings(1_000., 0.), salary]).await;
    assert!(result.stats.is_none());
    match result.blocked {
        Some(DraftBlocked::Compile { message }) => assert!(!message.is_empty()),
        other => panic!("expected a compile problem, got {other:?}"),
    }
}

/// The draft's holdings and a spending event whose amount is `$spend`, resolved
/// in memory: a plan with one parameter that matters.
async fn plan_with_spend_parameter(d: &Draft) -> ScenarioGraph {
    fn swap(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if map.get("kind") == Some(&json!("Fixed"))
                    && map.get("value") == Some(&json!(40_000.0))
                {
                    *value = json!({"kind": "Expression", "source": "$spend"});
                } else {
                    map.values_mut().for_each(swap);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(swap),
            _ => {}
        }
    }
    let template = serde_json::from_value::<crate::suggest::templates::TemplateRequest>(json!({
        "kind": "recurring_expense", "name": "Living", "from_account_id": {"$new": "checking"},
        "amount": 40_000.0, "inflation_adjusted": false,
    }))
    .unwrap()
    .expand()
    .unwrap()
    .changes;
    let mut expense = serde_json::to_value(&template).unwrap();
    swap(&mut expense);
    let mut expense: Vec<Change> = serde_json::from_value(expense).unwrap();
    expense.push(
        serde_json::from_value(json!({
            "op": "add", "target": {"new_parameter": "spend"}, "path": "",
            "value": {"name": "spend", "value": {"kind": "Money", "value": 40_000.0}}
        }))
        .unwrap(),
    );
    let mut graph = ScenarioGraph::load(&d.state.db, d.scenario, &d.user.id)
        .await
        .unwrap();
    load_profiles(&d.state.db, &d.user.id, &mut graph, [d.profile].into())
        .await
        .unwrap();
    suggest::resolve_steps(
        &graph,
        &[d.holdings(1_000_000., 0.), expense],
        &Created::new(),
    )
    .unwrap()
    .unwrap_or_else(|failed| panic!("the plan did not resolve: {failed:?}"))
    .graph
}

#[tokio::test]
async fn an_ai_goal_seek_finds_the_spending_that_reaches_the_target_and_spends_no_quota() {
    use crate::suggest::ai::tools::goal_seek::{Direction, GoalSeekRequest, Metric};

    let d = Draft::new(Some("1980-01-01")).await;
    let graph = plan_with_spend_parameter(&d).await;
    // Compute admission is per user and shared by the tests of this process.
    let user = CurrentUser {
        id: "goal-seek-spend".into(),
        ..d.user.clone()
    };
    let seek = |parameter: &str, target: f64| GoalSeekRequest {
        parameter: parameter.into(),
        metric: Metric::SuccessRate,
        target,
        direction: Some(Direction::Largest),
        min: Some(10_000.0),
        max: Some(600_000.0),
    };

    // By name, ignoring case.
    let found = crate::api::analysis::ai_goal_seek(&d.state, &user, &graph, seek("SPEND", 0.9))
        .await
        .unwrap();
    assert_eq!(found["found"], true, "{found}");
    assert_eq!(found["method"], "bisection");
    assert_eq!(found["parameter"]["name"], "spend");
    let value = found["result"]["value"].as_f64().unwrap();
    assert!((10_000.0..600_000.0).contains(&value), "{found}");
    assert!(found["result"]["success_rate"].as_f64().unwrap() >= 0.9);

    // A stricter target allows less spending, by id.
    let id = found["parameter"]["id"].as_str().unwrap().to_owned();
    let stricter = crate::api::analysis::ai_goal_seek(&d.state, &user, &graph, seek(&id, 0.999))
        .await
        .unwrap();
    if stricter["found"] == true {
        assert!(stricter["result"]["value"].as_f64().unwrap() <= value);
    }

    // Nothing in range reaches 100% at a spend this high: the closest is named.
    let mut hopeless = seek("spend", 1.0);
    hopeless.min = Some(550_000.0);
    let missed = crate::api::analysis::ai_goal_seek(&d.state, &user, &graph, hopeless)
        .await
        .unwrap();
    assert_eq!(missed["found"], false, "{missed}");
    assert!(missed["closest"]["success_rate"].is_number());

    // An unknown parameter is answered with the ones the plan has.
    let unknown = crate::api::analysis::ai_goal_seek(&d.state, &user, &graph, seek("Nope", 0.9))
        .await
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("spend"), "{unknown}");

    // The user's monthly goal-seek quota is not the model's to spend.
    let used: i64 = sqlx::query_scalar("SELECT count(*) FROM monthly_goal_seeks")
        .fetch_one(&d.state.db)
        .await
        .unwrap();
    assert_eq!(used, 0);
}

#[tokio::test]
async fn an_ai_sensitivity_ranks_the_plans_parameters_and_refuses_unknown_ones() {
    use crate::suggest::ai::tools::sensitivity::{SensitivityError, SensitivityRequest};

    let d = Draft::new(Some("1980-01-01")).await;
    let graph = plan_with_spend_parameter(&d).await;
    let user = CurrentUser {
        id: "sensitivity-spend".into(),
        ..d.user.clone()
    };
    let ranked = crate::api::analysis::ai_sensitivity(
        &d.state,
        &user,
        &graph,
        SensitivityRequest {
            parameters: Vec::new(),
            fraction: Some(0.5),
            metric: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(ranked["metric"], "funding_success_rate", "{ranked}");
    assert_eq!(ranked["simulations"], 3);
    let row = &ranked["ranking"][0];
    assert_eq!(row["parameter"]["name"], "spend");
    assert_eq!(row["low"]["value"], 20_000.0);
    assert_eq!(row["high"]["value"], 60_000.0);
    // Less spending never funds the plan worse.
    assert!(
        row["low"]["success_rate"].as_f64().unwrap()
            >= row["high"]["success_rate"].as_f64().unwrap(),
        "{ranked}"
    );
    assert!(row["span_points"].as_f64().unwrap() >= 0.0);

    let unknown = crate::api::analysis::ai_sensitivity(
        &d.state,
        &user,
        &graph,
        SensitivityRequest {
            parameters: vec!["Nope".into()],
            fraction: None,
            metric: None,
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&unknown, SensitivityError::Refused(m) if m.contains("spend")),
        "{unknown:?}"
    );
}

#[tokio::test]
async fn an_ai_goal_seek_over_an_age_searches_a_grid_and_reports_years() {
    use crate::suggest::ai::tools::goal_seek::{GoalSeekRequest, Metric};

    let d = Draft::new(Some("1980-01-01")).await;
    // Spending of $70,000 a year that starts when the person reaches the
    // `retire` age parameter: the later, the more the savings carry.
    let template = serde_json::from_value::<crate::suggest::templates::TemplateRequest>(json!({
        "kind": "recurring_expense", "name": "Living", "from_account_id": {"$new": "checking"},
        "amount": 70_000.0, "inflation_adjusted": false,
        "start": {"kind": "Age", "years": 50},
    }))
    .unwrap()
    .expand()
    .unwrap()
    .changes;
    let mut expense = serde_json::to_string(&template).unwrap();
    let start = r#""start_condition":{"kind":"Age""#;
    assert!(expense.contains(start), "{expense}");
    expense = expense.replacen(
        &expense[expense.find(start).unwrap() + r#""start_condition":"#.len()..][..expense
            [expense.find(start).unwrap() + r#""start_condition":"#.len()..]
            .find('}')
            .unwrap()
            + 1],
        r#"{"kind":"AgeParameter","parameter_id":{"$new":"retire"}}"#,
        1,
    );
    let mut expense: Vec<Change> = serde_json::from_str(&expense).unwrap();
    expense.push(
        serde_json::from_value(json!({
            "op": "add", "target": {"new_parameter": "retire"}, "path": "",
            "value": {"name": "retire", "value": {"kind": "Age", "years": 60, "months": 0}}
        }))
        .unwrap(),
    );
    let mut graph = ScenarioGraph::load(&d.state.db, d.scenario, &d.user.id)
        .await
        .unwrap();
    load_profiles(&d.state.db, &d.user.id, &mut graph, [d.profile].into())
        .await
        .unwrap();
    let graph = suggest::resolve_steps(
        &graph,
        &[d.holdings(1_500_000., 0.), expense],
        &Created::new(),
    )
    .unwrap()
    .unwrap_or_else(|failed| panic!("the plan did not resolve: {failed:?}"))
    .graph;

    let user = CurrentUser {
        id: "goal-seek-age".into(),
        ..d.user.clone()
    };
    let found = crate::api::analysis::ai_goal_seek(
        &d.state,
        &user,
        &graph,
        GoalSeekRequest {
            parameter: "retire".into(),
            metric: Metric::SuccessRate,
            target: 0.9,
            direction: None,
            min: Some(50.0),
            max: Some(70.0),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        found["direction"], "smallest",
        "an age defaults to the earliest"
    );
    assert_eq!(found["method"], "grid_search");
    assert_eq!(found["parameter"]["unit"], "years of age");
    assert_eq!(found["found"], true, "{found}");
    let age = found["result"]["value"].as_f64().unwrap();
    assert!((50.0..=70.0).contains(&age), "{found}");
    assert!(
        found["result"]["value_text"]
            .as_str()
            .unwrap()
            .starts_with("age ")
    );
}

#[test]
fn a_previews_iterations_are_clamped_to_the_callers_cap() {
    let limit = |cap| Limit {
        cap,
        tier: Tier::Free,
    };
    assert_eq!(draft_iterations(None, limit(50_000)), 400);
    assert_eq!(draft_iterations(None, limit(100)), 100);
    assert_eq!(draft_iterations(Some(5_000), limit(1_000)), 1_000);
    assert_eq!(draft_iterations(Some(50_000), limit(50_000)), 5_000);
    assert_eq!(draft_iterations(Some(1), limit(100)), 1);
    // A zero cap never produces a zero-iteration simulation.
    assert_eq!(draft_iterations(Some(5), limit(0)), 1);
    assert_eq!(limit(1_000).iterations(), 1_000);
    assert_eq!(limit(50_000).iterations(), MAX_PREVIEW_ITERATIONS);
}
