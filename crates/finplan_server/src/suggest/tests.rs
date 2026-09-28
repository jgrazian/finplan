//! Changes against the default fixture's run snapshot (`testdata/`), the graph
//! a preview starts from. Ids: events 5 Home Purchase, 6 Sweep, 3 Retirement;
//! asset 2 House (unmapped), 14 GOOG; accounts 6 USAA (Bank), 2 Vanguard Roth
//! IRA (Investment). The snapshot kept only profiles something used, so 5
//! (REITs) is absent.

use serde_json::{Value, json};

use super::*;
use crate::api::events::EventBody;
use crate::api::specs::{AmountSpec, EffectSpec};

fn graph() -> ScenarioGraph {
    serde_json::from_str(include_str!("testdata/default_snapshot.json")).unwrap()
}

fn changes(value: Value) -> Vec<Change> {
    serde_json::from_value(value).unwrap()
}

fn resolved(value: Value) -> Resolved {
    resolve(&graph(), &changes(value)).unwrap()
}

fn problems(value: Value) -> Vec<ChangeProblem> {
    resolve(&graph(), &changes(value)).unwrap_err()
}

fn replaced_event(r: &Resolved) -> (i64, &EventBody) {
    match r.changes.as_slice() {
        [ResolvedChange::ReplaceEvent { id, body }] => (*id, body),
        other => panic!("expected one ReplaceEvent, got {other:?}"),
    }
}

#[test]
fn remove_the_home_purchase_sweep() {
    let r = resolved(json!([{
        "op": "remove",
        "target": {"event": 5},
        "path": "/effects/0",
        "expect": {
            "kind": "Sweep", "to_account_id": 6, "amount_mode": "Net", "lot_method": "Fifo",
            "income_type": "Taxable",
            "amount": {"kind": "InflationAdjusted", "inner": {"kind": "Fixed", "value": 200000.0}},
            "sources": {"mode": "Strategy", "strategy": "PenaltyAware",
                        "exclude_accounts": [], "bracket_ceiling": null}
        }
    }]));
    let (id, body) = replaced_event(&r);
    assert_eq!(id, 5);
    assert_eq!(body.name, "Home Purchase");
    assert_eq!(body.effects.len(), 3);
    assert!(matches!(body.effects[0], EffectSpec::Expense { .. }));
    // The event keeps its place in the list.
    assert_eq!(body.sort_order, Some(4));

    assert_eq!(
        r.diff(&Names::from_graph(&graph())),
        vec![DiffLine {
            label: "Plan › Home Purchase › effects › Sweep".into(),
            from: Some("Sweep $200,000, inflation-adjusted → USAA".into()),
            to: None,
        }]
    );
}

#[test]
fn raise_the_sweep_amount() {
    let r = resolved(json!([{
        "op": "replace",
        "target": {"event": 6},
        "path": "/effects/0/amount/inner/value",
        "expect": 20000,
        "value": 100000
    }]));
    let (id, body) = replaced_event(&r);
    assert_eq!(id, 6);
    let EffectSpec::Sweep { amount, .. } = &body.effects[0] else {
        panic!("still a sweep");
    };
    let AmountSpec::InflationAdjusted { inner } = amount else {
        panic!("still inflation-adjusted");
    };
    assert!(matches!(**inner, AmountSpec::Fixed { value } if value == 100000.0));

    assert_eq!(
        r.diff(&Names::from_graph(&graph())),
        vec![DiffLine {
            label: "Plan › Sweep › effects › Sweep › amount".into(),
            from: Some("$20,000, inflation-adjusted".into()),
            to: Some("$100,000, inflation-adjusted".into()),
        }]
    );
}

#[test]
fn map_the_house_onto_a_profile_the_snapshot_pruned() {
    let r = resolved(json!([{
        "op": "replace",
        "target": {"asset": 2},
        "path": "/return_profile_id",
        "expect": null,
        "value": 5
    }]));
    match r.changes.as_slice() {
        [ResolvedChange::UpdateAsset { id: 2, body }] => {
            assert_eq!(body.return_profile_id, Some(Some(5)));
            assert!(body.name.is_none() && body.initial_price.is_none());
        }
        other => panic!("expected one UpdateAsset, got {other:?}"),
    }
    // Profile 5 is not in the snapshot: not an error, but reported so the
    // preview can load it.
    assert!(!graph().return_profiles.contains_key(&5));
    assert_eq!(
        r.referenced_profiles().into_iter().collect::<Vec<_>>(),
        vec![5]
    );

    let names = Names::from_graph(&graph());
    assert_eq!(
        r.diff(&names)[0],
        DiffLine {
            label: "Portfolio › House › return profile".into(),
            from: Some("none".into()),
            to: Some("profile #5".into()),
        }
    );
    let names = names.with_profiles([(5, "REITs".to_string())]);
    assert_eq!(r.diff(&names)[0].to.as_deref(), Some("REITs"));
}

#[test]
fn unmap_an_asset() {
    let r = resolved(json!([{
        "op": "replace", "target": {"asset": 14}, "path": "/return_profile_id", "value": null
    }]));
    let [ResolvedChange::UpdateAsset { body, .. }] = r.changes.as_slice() else {
        panic!("one UpdateAsset");
    };
    assert_eq!(body.return_profile_id, Some(None));
}

#[test]
fn a_stale_expect_reports_the_current_value() {
    let found = problems(json!([{
        "op": "replace",
        "target": {"event": 6},
        "path": "/effects/0/amount/inner/value",
        "expect": 25000,
        "value": 100000
    }]));
    assert_eq!(
        found,
        vec![ChangeProblem::Stale {
            change: 0,
            path: "/effects/0/amount/inner/value".into(),
            expected: Box::new(json!(25000)),
            actual: Box::new(json!(20000.0)),
        }]
    );
}

#[test]
fn bad_paths() {
    let found = problems(json!([
        {"op": "replace", "target": {"event": 6}, "path": "/effects/7/amount", "value": 1},
        {"op": "replace", "target": {"asset": 14}, "path": "/id", "value": 99},
        {"op": "replace", "target": {"account": 6}, "path": "no-slash", "value": 1},
        {"op": "replace", "target": {"account": 2}, "path": "/flavor", "value": "Bank"}
    ]));
    assert_eq!(found.len(), 4);
    assert!(
        found
            .iter()
            .all(|p| matches!(p, ChangeProblem::BadPath { .. }))
    );
    let ChangeProblem::BadPath { reason, .. } = &found[1] else {
        unreachable!()
    };
    assert_eq!(reason, "id is read-only");
}

#[test]
fn an_invalid_body_after_patching() {
    let found = problems(json!([{
        "op": "replace", "target": {"event": 6}, "path": "/effects/0/kind", "value": "Teleport"
    }]));
    assert!(matches!(
        found.as_slice(),
        [ChangeProblem::InvalidBody { change: 0, target: ChangeTarget::Event(6), message }]
            if message.contains("Teleport")
    ));
}

#[test]
fn unknown_targets_and_unsupported_ops() {
    let found = problems(json!([
        {"op": "remove", "target": {"event": 999}},
        {"op": "replace", "target": {"new_event": "x"}, "path": "/name", "value": "y"},
        {"op": "add", "target": {"event": 3}, "path": "", "value": {}},
        {"op": "replace", "target": {"asset": 14}, "path": "/name"}
    ]));
    assert!(matches!(
        found[0],
        ChangeProblem::UnknownTarget { change: 0, .. }
    ));
    assert!(matches!(
        found[1],
        ChangeProblem::UnsupportedOp { change: 1, .. }
    ));
    assert!(matches!(
        found[2],
        ChangeProblem::UnsupportedOp { change: 2, .. }
    ));
    assert!(matches!(
        found[3],
        ChangeProblem::UnsupportedOp { change: 3, .. }
    ));
}

#[test]
fn create_then_patch_a_new_event_and_delete_another() {
    let r = resolved(json!([
        {"op": "add", "target": {"new_event": "crash"}, "path": "", "value": {
            "name": "Crash 2038",
            "fires_once": true,
            "trigger": {"kind": "Date", "on_date": "2038-01-01"},
            "effects": [{"kind": "MarketShock", "drop": 0.4}]
        }},
        {"op": "replace", "target": {"new_event": "crash"}, "path": "/effects/0/drop",
         "expect": 0.4, "value": 0.45},
        {"op": "remove", "target": {"event": 3}, "path": ""}
    ]));
    match r.changes.as_slice() {
        [
            ResolvedChange::CreateEvent { key, body },
            ResolvedChange::DeleteEvent { id: 3 },
        ] => {
            assert_eq!(key, "crash");
            assert_eq!(body.name, "Crash 2038");
            assert!(body.fires_once && body.enabled);
            assert!(matches!(body.effects[0], EffectSpec::MarketShock { drop } if drop == 0.45));
        }
        other => panic!("unexpected {other:?}"),
    }

    let diff = r.diff(&Names::from_graph(&graph()));
    assert_eq!(diff.len(), 2);
    assert_eq!(diff[0].label, "Plan › + Crash 2038");
    assert_eq!(diff[0].from, None);
    assert_eq!(
        diff[0].to.as_deref(),
        Some("Crash 2038 · on 2038-01-01 · Market shock −45%")
    );
    assert_eq!(diff[1].label, "Plan › Retirement");
    assert_eq!(diff[1].to, None);
}

#[test]
fn account_flavor_fields_replace_the_whole_detail() {
    let r = resolved(json!([
        {"op": "replace", "target": {"account": 6}, "path": "/cash_value",
         "expect": 250000, "value": 300000}
    ]));
    let [ResolvedChange::UpdateAccount { id: 6, body }] = r.changes.as_slice() else {
        panic!("one UpdateAccount");
    };
    assert!(body.name.is_none());
    assert!(matches!(
        body.flavor,
        Some(FlavorSpec::Bank { cash_value, return_profile_id: 6 }) if cash_value == 300000.0
    ));
    assert_eq!(
        r.referenced_profiles().into_iter().collect::<Vec<_>>(),
        vec![6]
    );
    assert_eq!(
        r.diff(&Names::from_graph(&graph())),
        vec![DiffLine {
            label: "Portfolio › USAA › cash".into(),
            from: Some("$250,000".into()),
            to: Some("$300,000".into()),
        }]
    );
}

#[test]
fn positions_patch_by_id() {
    let g = graph();
    let lot = &g.positions[&2][0];
    let r = resolved(json!([
        {"op": "replace", "target": {"account": 2}, "path": "/positions/0/cost_basis",
         "expect": lot.cost_basis, "value": 20000},
        {"op": "remove", "target": {"account": 2}, "path": "/positions/1"},
        {"op": "add", "target": {"account": 2}, "path": "/positions/-",
         "value": {"asset_id": 14, "units": 10, "cost_basis": 3000}}
    ]));
    match r.changes.as_slice() {
        [
            ResolvedChange::UpdatePosition {
                account_id: 2,
                position_id,
                body: update,
            },
            ResolvedChange::CreatePosition {
                account_id: 2,
                body: create,
            },
            ResolvedChange::DeletePosition {
                account_id: 2,
                position_id: deleted,
            },
        ] => {
            assert_eq!(*position_id, lot.id);
            assert_eq!(update.cost_basis, Some(20000.0));
            assert!(update.units.is_none());
            assert_eq!(create.asset_id, 14);
            assert_eq!(*deleted, g.positions[&2][1].id);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_no_op_change_writes_nothing() {
    let r = resolved(json!([
        {"op": "replace", "target": {"event": 6}, "path": "/name", "expect": "Sweep", "value": "Sweep"}
    ]));
    assert!(r.changes.is_empty());
    assert!(r.diff(&Names::default()).is_empty());
}

#[test]
fn changes_round_trip_as_json() {
    let batch = changes(json!([
        {"op": "replace", "target": {"asset": 2}, "path": "/return_profile_id",
         "expect": null, "value": 5},
        {"op": "remove", "target": {"new_event": "k"}}
    ]));
    assert_eq!(batch[0].expect, Some(Value::Null));
    assert_eq!(batch[1].path, "");
    assert_eq!(batch[1].value, None);
    let again: Vec<Change> = serde_json::from_value(serde_json::to_value(&batch).unwrap()).unwrap();
    assert_eq!(again[0].expect, Some(Value::Null));
}

// ── creating assets and accounts, and `$new` references ─────────────────────

/// A brokerage account holding a new ETF and an existing fund, funded monthly
/// from USAA: a batch whose later writes need the earlier ones' ids.
fn brokerage_batch() -> Value {
    json!([
        {"op": "add", "target": {"new_event": "contribution"}, "path": "", "value": {
            "name": "Monthly contribution",
            "trigger": {"kind": "Repeating", "interval": "Monthly", "start_condition": null,
                        "end_condition": null, "max_occurrences": null},
            "effects": [{"kind": "CashTransfer", "from_account_id": 6,
                         "to_account_id": {"$new": "brokerage"},
                         "amount": {"kind": "Fixed", "value": 1000.0}}]
        }},
        {"op": "add", "target": {"new_account": "brokerage"}, "path": "", "value": {
            "name": "Brokerage", "description": null, "flavor": "Investment",
            "tax_status": "Taxable", "cash_value": 50000.0, "cash_return_profile_id": 6,
            "contribution_limit": null, "contribution_period": null,
            "positions": [
                {"asset_id": {"$new": "vti"}, "units": 10.0, "cost_basis": 2500.0},
                {"asset_id": 1, "units": 2.0, "cost_basis": 1000.0}
            ]
        }},
        {"op": "add", "target": {"new_asset": "vti"}, "path": "", "value": {
            "name": "VTI", "description": "Vanguard Total Stock Market ETF",
            "initial_price": 250.0, "return_profile_id": 1, "tracking_error": null
        }}
    ])
}

#[test]
fn create_an_asset_an_account_holding_it_and_an_event_funding_it() {
    let r = resolved(brokerage_batch());
    // Temporary ids stand in for references, in the order keys were named.
    match r.changes.as_slice() {
        [
            ResolvedChange::CreateEvent {
                key: e,
                body: event,
            },
            ResolvedChange::CreateAccount {
                key: a,
                body: account,
                positions,
            },
            ResolvedChange::CreateAsset {
                key: v,
                body: asset,
            },
        ] => {
            assert_eq!(
                (e.as_str(), a.as_str(), v.as_str()),
                ("contribution", "brokerage", "vti")
            );
            assert!(matches!(
                event.effects[0],
                EffectSpec::CashTransfer {
                    from_account_id: 6,
                    to_account_id: -2,
                    ..
                }
            ));
            assert_eq!(account.name, "Brokerage");
            assert_eq!(positions.len(), 2);
            assert_eq!(positions[0].asset_id, -3);
            assert_eq!(positions[1].asset_id, 1);
            assert_eq!(asset.initial_price, 250.0);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(r.referenced_profiles(), [1, 6].into());

    let diff = r.diff(&Names::from_graph(&graph()));
    let line = |label: &str| {
        diff.iter()
            .find(|l| l.label == label)
            .unwrap_or_else(|| panic!("no {label} in {diff:?}"))
    };
    assert_eq!(
        line("Portfolio › + VTI").to.as_deref(),
        Some("$250.00 · US Total Market")
    );
    assert_eq!(
        line("Portfolio › + Brokerage").to.as_deref(),
        Some("Taxable investment · $50,000 cash · 2 lots")
    );
    let contribution = line("Plan › + Monthly contribution").to.clone().unwrap();
    assert!(contribution.contains("Brokerage"), "{contribution}");

    // Written in dependency order: the asset, the account, then the event —
    // each with the real ids of what came before.
    let mut plan = graph();
    let mut created = Created::new();
    apply_to_graph(&mut plan, &r, &mut created)
        .unwrap()
        .unwrap();
    let id = |key: &str| created[key].id;
    assert_eq!(created["vti"].kind, RefKind::Asset);
    assert_eq!(id("vti"), 15);
    assert_eq!(id("brokerage"), 9);
    assert_eq!(id("contribution"), 8);
    assert_eq!(plan.positions[&9][0].asset_id, 15);
    let event = crate::api::events::read_event(&plan, 8).unwrap();
    assert!(matches!(
        event.effects[0],
        EffectSpec::CashTransfer {
            to_account_id: 9,
            ..
        }
    ));
    crate::compile::compile(&plan).expect("the edited plan compiles");
}

#[test]
fn reference_problems() {
    let asset = |name: &str| json!({"name": name, "initial_price": 10.0, "return_profile_id": 1});
    let found = problems(json!([
        {"op": "add", "target": {"new_asset": "x"}, "path": "", "value": asset("X")},
        {"op": "add", "target": {"new_account": "x"}, "path": "", "value": {
            "name": "Twin", "flavor": "Bank", "cash_value": 0.0, "return_profile_id": 6}},
    ]));
    assert!(matches!(&found[..], [ChangeProblem::DuplicateKey { change: 1, key }] if key == "x"));

    let found = problems(json!([
        {"op": "replace", "target": {"event": 6}, "path": "/effects/0/to_account_id",
         "value": {"$new": "nowhere"}}
    ]));
    assert!(matches!(
        &found[..],
        [ChangeProblem::UnknownReference { change: 0, key }] if key == "nowhere"
    ));

    // An account where an asset belongs, and a reference in a field that
    // holds no id at all.
    let found = problems(json!([
        {"op": "add", "target": {"new_account": "cash"}, "path": "", "value": {
            "name": "Cash", "flavor": "Bank", "cash_value": 0.0, "return_profile_id": 6}},
        {"op": "replace", "target": {"account": 2}, "path": "/positions/0/asset_id",
         "value": {"$new": "cash"}},
        {"op": "replace", "target": {"event": 6}, "path": "/name", "value": {"$new": "cash"}},
    ]));
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(matches!(
        &found[0],
        ChangeProblem::WrongReferenceKind {
            change: 1,
            expected: Some(RefKind::Asset),
            found: RefKind::Account,
            ..
        }
    ));
    assert!(matches!(
        &found[1],
        ChangeProblem::WrongReferenceKind { change: 2, expected: None, field, .. } if field == "name"
    ));

    // Created, then removed again: nothing left to refer to.
    let found = problems(json!([
        {"op": "add", "target": {"new_asset": "gone"}, "path": "", "value": asset("Gone")},
        {"op": "remove", "target": {"new_asset": "gone"}, "path": ""},
        {"op": "replace", "target": {"account": 2}, "path": "/positions/0/asset_id",
         "value": {"$new": "gone"}},
    ]));
    assert!(matches!(
        &found[..],
        [ChangeProblem::UnknownReference { change: 2, .. }]
    ));

    // A malformed reference names nothing.
    let found = problems(json!([
        {"op": "replace", "target": {"event": 6}, "path": "/effects/0/to_account_id",
         "value": {"$new": "x", "extra": 1}}
    ]));
    assert!(matches!(
        &found[..],
        [ChangeProblem::UnknownReference { .. }]
    ));
}

fn relative_event(name: &str, to: Value) -> Value {
    json!({
        "name": name, "fires_once": true,
        "trigger": {"kind": "RelativeToEvent", "event_id": to, "unit": "Years", "value": 1},
        "effects": []
    })
}

#[test]
fn new_events_are_written_after_the_new_events_they_follow() {
    // Named second-first, written first-second.
    let r = resolved(json!([
        {"op": "add", "target": {"new_event": "second"}, "path": "",
         "value": relative_event("Second", json!({"$new": "first"}))},
        {"op": "add", "target": {"new_event": "first"}, "path": "",
         "value": relative_event("First", json!(3))},
    ]));
    let mut plan = graph();
    let mut created = Created::new();
    apply_to_graph(&mut plan, &r, &mut created)
        .unwrap()
        .unwrap();
    assert!(created["first"].id < created["second"].id);
    let second = crate::api::events::read_event(&plan, created["second"].id).unwrap();
    assert_eq!(
        serde_json::to_value(&second.trigger).unwrap()["event_id"],
        json!(created["first"].id)
    );

    let found = problems(json!([
        {"op": "add", "target": {"new_event": "a"}, "path": "",
         "value": relative_event("A", json!({"$new": "b"}))},
        {"op": "add", "target": {"new_event": "b"}, "path": "",
         "value": relative_event("B", json!({"$new": "a"}))},
    ]));
    assert!(matches!(
        &found[..],
        [ChangeProblem::ReferenceCycle { change: 0, keys }] if keys == &["a", "b"]
    ));
}

#[test]
fn a_new_accounts_lots_must_fit_its_flavor() {
    let found = problems(json!([
        {"op": "add", "target": {"new_account": "cash"}, "path": "", "value": {
            "name": "Cash", "flavor": "Bank", "cash_value": 0.0, "return_profile_id": 6,
            "positions": [{"asset_id": 1, "units": 1.0, "cost_basis": 1.0}]}},
    ]));
    assert!(matches!(
        &found[..],
        [ChangeProblem::InvalidBody { change: 0, .. }]
    ));
}

#[test]
fn steps_resolve_against_the_plan_the_earlier_steps_left() {
    let step = |value: Value| changes(value);
    let steps = vec![
        step(json!([
            {"op": "add", "target": {"new_asset": "vti"}, "path": "", "value": {
                "name": "VTI", "initial_price": 250.0, "return_profile_id": 1}},
        ])),
        // Edits what step 1 created, by its key, with `expect` read from it,
        // and refers to it from an existing account.
        step(json!([
            {"op": "replace", "target": {"new_asset": "vti"}, "path": "/initial_price",
             "expect": 250.0, "value": 260.0},
            {"op": "add", "target": {"account": 2}, "path": "/positions/-",
             "value": {"asset_id": {"$new": "vti"}, "units": 3.0, "cost_basis": 750.0}},
        ])),
    ];
    let stepped = resolve_steps(&graph(), &steps, &Created::new())
        .unwrap()
        .unwrap();
    let vti = stepped.created["vti"].id;
    let asset = stepped.graph.assets.iter().find(|a| a.id == vti).unwrap();
    assert_eq!(asset.initial_price, 260.0);
    assert!(
        stepped.graph.positions[&2]
            .iter()
            .any(|p| p.asset_id == vti)
    );
    let diff = stepped.steps[1].diff([]);
    assert!(
        diff.iter().any(|l| l.label == "Portfolio › VTI › price"),
        "{diff:?}"
    );
    assert!(
        diff.iter()
            .any(|l| l.to.as_deref().is_some_and(|t| t.contains("VTI"))
                && l.label.starts_with("Portfolio › Vanguard Roth IRA")),
        "{diff:?}"
    );

    // A stale step is reported as that step's.
    let mut stale = steps.clone();
    stale[1][0].expect = Some(json!(999.0));
    let problems = resolve_steps(&graph(), &stale, &Created::new())
        .unwrap()
        .unwrap_err();
    assert_eq!(problems.step, 1);
    assert!(matches!(
        &problems.problems[..],
        [ChangeProblem::Stale { change: 0, .. }]
    ));

    // Applied in separate requests: the second step alone, seeded with what
    // the first created.
    let first = resolve_steps(&graph(), &steps[..1], &Created::new())
        .unwrap()
        .unwrap();
    let second = resolve_steps(&first.graph, &steps[1..], &first.created)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&second.graph).unwrap(),
        serde_json::to_value(&stepped.graph).unwrap()
    );

    // A seeded key cannot be reused as another kind.
    let found = resolve_with(
        &first.graph,
        &changes(json!([{"op": "replace", "target": {"new_account": "vti"},
                          "path": "/name", "value": "x"}])),
        &first.created,
    )
    .unwrap_err();
    assert!(matches!(&found[..], [ChangeProblem::UnknownTarget { .. }]));
}

#[test]
fn profiles_named_finds_every_profile_a_change_points_at() {
    let named = profiles_named(&changes(json!([
        {"op": "replace", "target": {"asset": 2}, "path": "/return_profile_id", "value": 5},
        {"op": "add", "target": {"new_account": "b"}, "path": "", "value": {
            "name": "B", "flavor": "Bank", "cash_value": 0.0, "return_profile_id": 7}},
        {"op": "add", "target": {"new_asset": "a"}, "path": "", "value": {
            "name": "A", "initial_price": 1.0, "return_profile_id": 5}},
    ])));
    assert_eq!(named, vec![5, 7]);
}
