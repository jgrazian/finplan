//! Every template, applied through the routes' SQL halves to a stored plan and
//! compiled. The rest of the template tests (parameter checks, the individual
//! expansions) run on an in-memory plan in `finplan_plan::templates`.

use serde_json::{Value, json};

use super::*;
use crate::compile::rows::ScenarioGraph;
use crate::suggest::{Change, ChangeOp, ChangeTarget, Created, RefKind, apply_steps_sql};

struct Fixture {
    db: crate::db::Db,
    user: String,
    scenario: i64,
    profile: i64,
}

async fn scalar(db: &crate::db::Db, sql: &str, binds: &[Value]) -> i64 {
    let mut q = sqlx::query_scalar::<_, i64>(sql);
    for b in binds {
        q = match b {
            Value::String(s) => q.bind(s.clone()),
            Value::Number(n) if n.is_i64() => q.bind(n.as_i64()),
            Value::Number(n) => q.bind(n.as_f64()),
            other => panic!("unsupported bind {other}"),
        };
    }
    q.fetch_one(db).await.unwrap()
}

impl Fixture {
    async fn new() -> Self {
        let db = crate::db::connect("sqlite::memory:", 1).await.unwrap();
        let user = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES (?,?,'unused')")
            .bind(&user)
            .bind(format!("{user}@example.test"))
            .execute(&db)
            .await
            .unwrap();
        let u = json!(user);
        let dist = scalar(
            &db,
            "INSERT INTO distributions(user_id,kind,mean,std_dev) VALUES (?,'Normal',0.06,0.12) RETURNING id",
            std::slice::from_ref(&u),
        )
        .await;
        let profile = scalar(
            &db,
            "INSERT INTO return_profiles(user_id,name,distribution_id) VALUES (?,'Equity',?) RETURNING id",
            &[u.clone(), json!(dist)],
        )
        .await;
        let scenario = scalar(
            &db,
            "INSERT INTO scenarios(user_id,name,start_date,birth_date,duration_years)
             VALUES (?,'Templates','2026-01-01','1990-06-15',60) RETURNING id",
            &[u],
        )
        .await;
        Fixture {
            db,
            user,
            scenario,
            profile,
        }
    }

    /// Apply the steps in order to the stored plan.
    async fn apply(&self, steps: &[Vec<Change>]) -> Result<Created, crate::suggest::StepProblems> {
        let mut tx = self.db.begin().await.unwrap();
        let created = apply_steps_sql(&mut tx, self.scenario, &self.user, steps, &Created::new())
            .await
            .unwrap()?;
        tx.commit().await.unwrap();
        Ok(created)
    }

    async fn graph(&self) -> ScenarioGraph {
        crate::db::graph::load(&self.db, self.scenario, &self.user)
            .await
            .unwrap()
    }

    /// Checking, a 401(k) and one fund, referred to as `checking`, `k401`, `fund`.
    fn base(&self) -> Vec<Change> {
        vec![
            bank_account(
                "checking",
                "Checking",
                30_000.,
                RowRef::Id(self.profile),
                None,
            ),
            allocation_asset("fund", "Fund", RowRef::Id(self.profile), None),
            investment_account(
                "k401",
                "401(k)",
                "TaxDeferred",
                RowRef::Id(self.profile),
                None,
                vec![position(&RowRef::new("fund"), 100_000., 100_000.)],
            ),
        ]
    }
}

fn request(value: Value) -> TemplateRequest {
    serde_json::from_value(value).unwrap()
}

fn event(g: &ScenarioGraph, name: &str) -> crate::api::events::Event {
    let id = g.events.iter().find(|e| e.name == name).unwrap().id;
    crate::api::events::read_event(g, id).unwrap()
}

#[tokio::test]
async fn every_template_applies_and_compiles_and_prefixes_keep_expansions_apart() {
    let f = Fixture::new().await;
    let expand = |value: Value| request(value).expand().unwrap();
    let mut steps = vec![f.base()];

    let salary = expand(json!({
        "kind": "salary", "key_prefix": "job_", "name": "Salary",
        "to_account_id": {"$new": "checking"}, "annual_amount": 142000,
        "end": {"kind": "AgeParameter", "parameter_id": {"$new": "retire_age"}},
        "employee_401k": {"account_id": {"$new": "k401"}, "annual_amount": 23500,
                          "allocation": [{"asset_id": {"$new": "fund"}, "fraction": 1.0}]},
    }));
    assert_eq!(
        salary.keys,
        vec![CreatedKey {
            key: "job_salary".into(),
            kind: RefKind::Event
        }]
    );
    let parameter = Change {
        op: ChangeOp::Add,
        target: ChangeTarget::NewParameter("retire_age".into()),
        path: String::new(),
        expect: None,
        value: Some(
            json!({"name": "retire_age", "value": {"kind": "Age", "years": 65, "months": 0}}),
        ),
    };
    steps.push(vec![parameter]);
    steps.push(salary.changes);

    let templates = [
        json!({"kind": "employer_match", "key_prefix": "match_",
               "to_account_id": {"$new": "k401"}, "salary": 142000,
               "match_rate": 0.5, "up_to_percent": 6, "employee_percent": 16.5,
               "allocation": [{"asset_id": {"$new": "fund"}, "fraction": 1.0}],
               "end": {"kind": "AgeParameter", "parameter_id": {"$new": "retire_age"}}}),
        json!({"kind": "recurring_expense", "key_prefix": "rent_", "name": "Rent",
               "from_account_id": {"$new": "checking"}, "amount": 2400, "interval": "Monthly"}),
        json!({"kind": "recurring_expense", "key_prefix": "food_", "name": "Food",
               "from_account_id": {"$new": "checking"}, "amount": 900, "interval": "Monthly",
               "fund_from_investments": true}),
        json!({"kind": "retirement", "key_prefix": "ret_",
               "retirement": {"kind": "AgeParameter", "parameter_id": {"$new": "retire_age"}},
               "spending": {"from_account_id": {"$new": "checking"}, "annual_amount": 70000,
                            "fund_from_investments": true}}),
        json!({"kind": "social_security", "key_prefix": "ss_",
               "to_account_id": {"$new": "checking"}, "annual_benefit": 34000,
               "claim": {"kind": "Age", "years": 67}}),
        json!({"kind": "home_purchase", "key_prefix": "home_", "price": 450000,
               "down_payment": 90000, "mortgage_rate": 0.065,
               "from_account_id": {"$new": "checking"}, "when": {"kind": "Age", "years": 38}}),
        json!({"kind": "market_crash", "key_prefix": "crash_", "drop": 0.35,
               "when": {"kind": "Date", "on_date": "2035-03-01"}}),
        json!({"kind": "large_expense", "key_prefix": "care_", "name": "Long-term care",
               "from_account_id": {"$new": "checking"}, "amount": 120000,
               "when": {"kind": "Age", "years": 82}}),
        json!({"kind": "job_loss", "key_prefix": "loss_",
               "salary_event_id": {"$new": "job_salary"}, "months": 9,
               "when": {"kind": "Age", "years": 45}}),
    ];
    for template in templates {
        steps.push(expand(template).changes);
    }
    f.apply(&steps).await.unwrap_or_else(|p| panic!("{p:?}"));

    let g = f.graph().await;
    crate::compile::compile(&g).expect("the plan compiles");
    for name in [
        "Salary",
        "Employer 401(k) match",
        "Rent",
        "Food",
        "Retirement",
        "Retirement spending",
        "Social Security",
        "Home purchase",
        "Market crash",
        "Long-term care",
        "Job loss",
        "Job loss: back to work",
    ] {
        assert!(g.events.iter().any(|e| e.name == name), "{name}");
    }
    for name in ["Home", "Home mortgage", "Home (value)"] {
        assert!(
            g.accounts.iter().any(|a| a.name == name) || g.assets.iter().any(|a| a.name == name),
            "{name}"
        );
    }
    let salary = event(&g, "Salary");
    assert_eq!(salary.effects.len(), 3, "taxable pay, deferral, purchase");
}
