//! Each template expands, applies through the change model to an in-memory
//! plan, and compiles; expansions with different key prefixes share a plan.
//! The server repeats the full round trip through SQL (`finplan_server`'s
//! `suggest::templates` tests).

use serde_json::{Value, json};

use super::*;
use crate::graph::ScenarioGraph;
use crate::suggest::{ChangeProblem, Created, StepProblems, resolve_steps};

/// The default snapshot's assumptions (a scenario, return profiles, tax
/// config) with every account, asset and event removed.
fn blank_graph() -> ScenarioGraph {
    let mut plan: Value =
        serde_json::from_str(include_str!("../../testdata/default_snapshot.json")).unwrap();
    let root = plan.as_object_mut().unwrap();
    for table in [
        "accounts",
        "assets",
        "events",
        "parameters",
        "effect_children",
    ] {
        root.insert(table.into(), json!([]));
    }
    for table in [
        "amounts",
        "bank",
        "effects",
        "event_effects",
        "event_trigger",
        "investment",
        "liability",
        "positions",
        "property",
        "trigger_children",
        "triggers",
        "withdrawal_items",
        "withdrawal_sources",
    ] {
        root.insert(table.into(), json!({}));
    }
    serde_json::from_value(plan).unwrap()
}

struct Fixture {
    graph: ScenarioGraph,
    profile: i64,
}

impl Fixture {
    fn new() -> Self {
        let graph = blank_graph();
        let profile = *graph.return_profiles.keys().min().expect("a profile");
        Fixture { graph, profile }
    }

    /// Apply the steps in order to the plan, returning it as they leave it.
    fn apply(&self, steps: &[Vec<Change>]) -> Result<ScenarioGraph, StepProblems> {
        Ok(resolve_steps(&self.graph, steps, &Created::new())
            .unwrap()?
            .graph)
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

fn event(g: &ScenarioGraph, name: &str) -> crate::specs::events::Event {
    let id = g.events.iter().find(|e| e.name == name).unwrap().id;
    crate::specs::events::read_event(g, id).unwrap()
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

#[test]
fn every_template_applies_and_compiles_and_prefixes_keep_expansions_apart() {
    let f = Fixture::new();
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
    let g = f.apply(&steps).unwrap_or_else(|p| panic!("{p:?}"));

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

#[test]
fn the_match_is_tax_free_income_scaled_from_salary() {
    let f = Fixture::new();
    let m = request(json!({
        "kind": "employer_match", "to_account_id": {"$new": "k401"}, "salary": 100000,
        "match_rate": 0.5, "up_to_percent": 6,
    }))
    .expand()
    .unwrap();
    let g = f.apply(&[f.base(), m.changes]).unwrap();
    let e = event(&g, "Employer 401(k) match");
    let effect = serde_json::to_value(&e.effects[0]).unwrap();
    assert_eq!(effect["kind"], "Income");
    assert_eq!(effect["income_type"], "TaxFree");
    assert_eq!(effect["amount"]["kind"], "Scale");
    assert_eq!(effect["amount"]["factor"], 0.03);
    assert_eq!(effect["amount"]["inner"]["inner"]["value"], 100000.);
}

#[test]
fn a_match_can_follow_a_salary_parameter() {
    let f = Fixture::new();
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
    let g = f.apply(&[f.base(), vec![parameter], m.changes]).unwrap();
    crate::compile::compile(&g).expect("compiles");
}

#[test]
fn an_expense_can_follow_a_monthly_parameter() {
    let f = Fixture::new();
    let e = request(json!({
        "kind": "recurring_expense", "name": "Living", "from_account_id": {"$new": "checking"},
        "amount": 36000, "amount_parameter": "Monthly spending", "parameter_interval": "Monthly",
        "fund_from_investments": true,
    }))
    .expand()
    .unwrap();
    let parameter = parameter(
        "monthly",
        "Monthly spending",
        ParameterValueSpec::Money { value: 3000. },
    );
    let g = f.apply(&[f.base(), vec![parameter], e.changes]).unwrap();
    let living = serde_json::to_value(event(&g, "Living")).unwrap();
    assert_eq!(
        living["effects"][0]["amount"]["source"],
        "top_up(inflation($\"Monthly spending\" * 12))"
    );
    assert_eq!(
        living["effects"][1]["amount"]["source"],
        "inflation($\"Monthly spending\" * 12)"
    );
    crate::compile::compile(&g).expect("compiles");
}

#[test]
fn a_cash_home_purchase_makes_no_mortgage() {
    let f = Fixture::new();
    let h = request(json!({
        "kind": "home_purchase", "price": 200000, "down_payment": 200000,
        "from_account_id": {"$new": "checking"}, "when": {"kind": "Age", "years": 40},
    }))
    .expand()
    .unwrap();
    assert!(h.keys.iter().all(|k| k.key != "mortgage"));
    let g = f.apply(&[f.base(), h.changes]).unwrap();
    assert!(g.accounts.iter().all(|a| a.name != "Home mortgage"));
    crate::compile::compile(&g).expect("compiles");
}

#[test]
fn two_expansions_without_a_prefix_collide_on_their_keys() {
    let f = Fixture::new();
    let rent = |name: &str| {
        request(json!({"kind": "recurring_expense", "name": name,
                       "from_account_id": {"$new": "checking"}, "amount": 100}))
        .expand()
        .unwrap()
        .changes
    };
    let mut both = rent("Rent");
    both.extend(rent("Food"));
    let problems = f.apply(&[f.base(), both]).unwrap_err();
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
    f.apply(&[f.base(), namespaced]).unwrap();
}

/// The base plan plus a Roth, `roth`.
fn with_roth(f: &Fixture) -> Vec<Change> {
    let mut base = f.base();
    base.push(investment_account(
        "roth",
        "Roth IRA",
        "TaxFree",
        RowRef::Id(f.profile),
        None,
        vec![],
    ));
    base
}

#[test]
fn roth_conversions_fire_each_dec_30_until_rmds_and_go_last() {
    let f = Fixture::new();
    let expansion = request(json!({
        "kind": "roth_conversions", "key_prefix": "conv_",
        "from_account_id": {"$new": "k401"}, "to_account_id": {"$new": "roth"},
        "ceiling_rate": 0.22, "pay_tax_from_account_id": {"$new": "checking"},
        "start": {"kind": "Date", "on_date": "2030-06-15"},
    }))
    .expand()
    .unwrap();
    let rent = request(json!({"kind": "recurring_expense", "key_prefix": "rent_",
        "name": "Rent", "from_account_id": {"$new": "checking"}, "amount": 2000}))
    .expand()
    .unwrap();
    let g = f
        .apply(&[with_roth(&f), rent.changes, expansion.changes])
        .unwrap_or_else(|p| panic!("{p:?}"));

    let conversions = event(&g, "Roth conversions");
    assert_eq!(
        g.events.iter().max_by_key(|e| e.sort_order).unwrap().name,
        "Roth conversions"
    );
    let trigger = serde_json::to_value(&conversions.trigger).unwrap();
    assert_eq!(trigger["interval"], "Yearly");
    assert_eq!(trigger["start_condition"]["on_date"], "2030-12-30");
    assert_eq!(trigger["end_condition"]["kind"], "Age");
    assert_eq!(trigger["end_condition"]["years"], 73);
    let effect = serde_json::to_value(&conversions.effects[0]).unwrap();
    assert_eq!(effect["kind"], "RothConversion");
    assert_eq!(effect["amount"]["source"], "bracket_room(22%)");
    let checking = g.accounts.iter().find(|a| a.name == "Checking").unwrap().id;
    assert_eq!(effect["pay_tax_from_account_id"], checking);

    // It compiles and runs: the first conversion is on Dec 30, 2030.
    let compiled = crate::compile::compile(&g).expect("the plan compiles");
    let result = finplan_core::simulation::simulate(&compiled.config, 1).unwrap();
    assert!(result.ledger.iter().any(|e| matches!(
        e.event,
        finplan_core::model::StateEvent::RothConversion { .. }
    ) && e.date == jiff::civil::date(2030, 12, 30)));
}

#[test]
fn roth_conversions_start_from_the_plan_when_expanded_against_it() {
    let f = Fixture::new();
    let mut g = f.apply(&[with_roth(&f)]).unwrap();
    let params = |start: Option<When>| RothConversionsParams {
        name: None,
        from_account_id: RowRef::Id(1),
        to_account_id: RowRef::Id(2),
        ceiling_rate: 0.12,
        start,
        end: None,
        pay_tax_from_account_id: None,
    };
    let start = |p: RothConversionsParams| match p.start {
        Some(When::Date { on_date }) => on_date,
        other => panic!("{other:?}"),
    };

    // No retirement parameter: the plan's first year.
    assert_eq!(start(params(None).for_plan(&g).unwrap()), "2026-09-03");
    // An age: the day it is reached (born 1996-01-01).
    assert_eq!(
        start(params(Some(When::age(55))).for_plan(&g).unwrap()),
        "2051-01-01"
    );
    // The retirement-age parameter, by default and by id.
    g.parameters.push(crate::graph::ParameterRow {
        id: 900,
        name: "Retirement age".into(),
        kind: "Age".into(),
        number_value: None,
        date_value: None,
        age_years: Some(50),
        age_months: Some(6),
    });
    assert_eq!(start(params(None).for_plan(&g).unwrap()), "2046-07-01");
    let by_id = params(Some(When::AgeParameter {
        parameter_id: RowRef::Id(900),
    }));
    assert_eq!(start(by_id.for_plan(&g).unwrap()), "2046-07-01");
    // A date is kept as given.
    let dated = params(Some(When::Date {
        on_date: "2040-01-01".into(),
    }));
    assert_eq!(start(dated.for_plan(&g).unwrap()), "2040-01-01");
}

#[test]
fn roth_conversion_parameters_are_checked() {
    let bad = |value: Value| request(value).expand().unwrap_err().to_string();
    let date = json!({"kind": "Date", "on_date": "2030-01-01"});
    assert!(
        bad(
            json!({"kind": "roth_conversions", "from_account_id": 1, "to_account_id": 2,
                   "ceiling_rate": 22, "start": date})
        )
        .contains("between 0 and 1")
    );
    assert!(
        bad(
            json!({"kind": "roth_conversions", "from_account_id": 1, "to_account_id": 1,
                   "ceiling_rate": 0.22, "start": date})
        )
        .contains("two accounts")
    );
    assert!(
        bad(
            json!({"kind": "roth_conversions", "from_account_id": 1, "to_account_id": 2,
                   "ceiling_rate": 0.22, "start": {"kind": "Age", "years": 60}})
        )
        .contains("against the plan")
    );
}

#[test]
fn a_conversion_into_an_account_that_is_not_a_roth_does_not_apply() {
    let f = Fixture::new();
    let expansion = request(json!({
        "kind": "roth_conversions",
        "from_account_id": {"$new": "k401"}, "to_account_id": {"$new": "checking"},
        "ceiling_rate": 0.22, "start": {"kind": "Date", "on_date": "2030-01-01"},
    }))
    .expand()
    .unwrap();
    assert!(f.apply(&[with_roth(&f), expansion.changes]).is_err());
}
