//! Each template expands, applies through the change model to a real plan,
//! and compiles; expansions with different key prefixes share a plan.

use serde_json::{Value, json};

use super::*;
use crate::compile::rows::ScenarioGraph;
use crate::suggest::{ChangeProblem, Created, apply_steps_sql};

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
        ScenarioGraph::load(&self.db, self.scenario, &self.user)
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

#[test]
fn requests_deserialize_from_tool_json_with_refs() {
    let r = request(json!({
        "kind": "employer_match", "key_prefix": "m_",
        "to_account_id": {"$new": "k401"}, "salary": 120000,
        "match_rate": 0.5, "up_to_percent": 6,
        "end": {"kind": "AgeParameter", "parameter_id": {"$new": "retire_age"}},
    }));
    assert_eq!(r.key_prefix, "m_");
    let Template::EmployerMatch(p) = &r.template else {
        panic!("kind")
    };
    assert_eq!(p.to_account_id, RowRef::new("k401"));
    assert_eq!(r.template.kind(), TemplateKind::EmployerMatch);
    let plain = request(json!({"kind": "salary", "to_account_id": 7, "annual_amount": 90000}));
    assert_eq!(plain.key_prefix, "");
    let Template::Salary(p) = plain.template else {
        panic!("kind")
    };
    assert_eq!(p.to_account_id, RowRef::Id(7));
}

#[test]
fn bad_parameters_are_refused_before_anything_is_written() {
    let bad = |value: Value| request(value).expand().unwrap_err().to_string();
    assert!(
        bad(json!({"kind": "salary", "to_account_id": 1, "annual_amount": 0})).contains("positive")
    );
    assert!(
        bad(
            json!({"kind": "salary", "to_account_id": 1, "annual_amount": 1000,
                   "employee_401k": {"account_id": 2, "annual_amount": 2000}})
        )
        .contains("exceeds")
    );
    assert!(
        bad(json!({"kind": "market_crash", "drop": 1.5,
                       "when": {"kind": "Age", "years": 60}}))
        .contains("between 0 and 1")
    );
    assert!(
        bad(
            json!({"kind": "home_purchase", "price": 500000, "down_payment": 100000,
                       "from_account_id": 1, "when": {"kind": "Age", "years": 40}})
        )
        .contains("mortgage_rate")
    );
    assert!(
        bad(
            json!({"kind": "large_expense", "from_account_id": 1, "amount": 5,
                       "when": {"kind": "Date", "on_date": "soon"}})
        )
        .contains("YYYY-MM-DD")
    );
    assert!(
        bad(
            json!({"kind": "recurring_expense", "name": "Rent", "from_account_id": 1,
                       "amount": 5, "interval": "Never"})
        )
        .contains("repeats")
    );
    assert!(
        bad(
            json!({"kind": "job_loss", "salary_event_id": 1, "months": 0,
                       "when": {"kind": "Age", "years": 50}})
        )
        .contains("months")
    );
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

#[tokio::test]
async fn the_match_is_tax_free_income_scaled_from_salary() {
    let f = Fixture::new().await;
    let m = request(json!({
        "kind": "employer_match", "to_account_id": {"$new": "k401"}, "salary": 100000,
        "match_rate": 0.5, "up_to_percent": 6,
    }))
    .expand()
    .unwrap();
    f.apply(&[f.base(), m.changes]).await.unwrap();
    let g = f.graph().await;
    let e = event(&g, "Employer 401(k) match");
    let effect = serde_json::to_value(&e.effects[0]).unwrap();
    assert_eq!(effect["kind"], "Income");
    assert_eq!(effect["income_type"], "TaxFree");
    assert_eq!(effect["amount"]["kind"], "Scale");
    assert_eq!(effect["amount"]["factor"], 0.03);
    assert_eq!(effect["amount"]["inner"]["inner"]["value"], 100000.);
}

#[tokio::test]
async fn a_match_can_follow_a_salary_parameter() {
    let f = Fixture::new().await;
    let m = request(json!({
        "kind": "employer_match", "to_account_id": {"$new": "k401"}, "salary": 0,
        "salary_parameter": "salary", "match_rate": 1.0, "up_to_percent": 4,
    }))
    .expand()
    .unwrap();
    let parameter = Change {
        op: ChangeOp::Add,
        target: ChangeTarget::NewParameter("salary".into()),
        path: String::new(),
        expect: None,
        value: Some(json!({"name": "salary", "value": {"kind": "Money", "value": 120000.0}})),
    };
    f.apply(&[f.base(), vec![parameter], m.changes])
        .await
        .unwrap();
    crate::compile::compile(&f.graph().await).expect("compiles");
}

#[tokio::test]
async fn a_cash_home_purchase_makes_no_mortgage() {
    let f = Fixture::new().await;
    let h = request(json!({
        "kind": "home_purchase", "price": 200000, "down_payment": 200000,
        "from_account_id": {"$new": "checking"}, "when": {"kind": "Age", "years": 40},
    }))
    .expand()
    .unwrap();
    assert!(h.keys.iter().all(|k| k.key != "mortgage"));
    f.apply(&[f.base(), h.changes]).await.unwrap();
    let g = f.graph().await;
    assert!(g.accounts.iter().all(|a| a.name != "Home mortgage"));
    crate::compile::compile(&g).expect("compiles");
}

#[tokio::test]
async fn two_expansions_without_a_prefix_collide_on_their_keys() {
    let f = Fixture::new().await;
    let rent = |name: &str| {
        request(json!({"kind": "recurring_expense", "name": name,
                       "from_account_id": {"$new": "checking"}, "amount": 100}))
        .expand()
        .unwrap()
        .changes
    };
    let mut both = rent("Rent");
    both.extend(rent("Food"));
    let problems = f.apply(&[f.base(), both]).await.unwrap_err();
    // The second `add` at "" lands on the first one's target.
    assert!(matches!(
        problems.problems[0],
        ChangeProblem::UnsupportedOp { .. } | ChangeProblem::DuplicateKey { .. }
    ));
    let mut namespaced = request(json!({"kind": "recurring_expense", "key_prefix": "a_",
        "name": "Rent", "from_account_id": {"$new": "checking"}, "amount": 100}))
    .expand()
    .unwrap()
    .changes;
    namespaced.extend(
        request(
            json!({"kind": "recurring_expense", "key_prefix": "b_", "name": "Food",
            "from_account_id": {"$new": "checking"}, "amount": 100}),
        )
        .expand()
        .unwrap()
        .changes,
    );
    f.apply(&[f.base(), namespaced]).await.unwrap();
}
