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
