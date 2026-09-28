//! The review loop against a scripted transport — no network — over the
//! anonymized default snapshot (`../testdata/`) and a small hand-built run.
//!
//! Ids: events 1 Salary, 5 Home Purchase (effect 0 is a Sweep), 6 Sweep;
//! accounts 1 Vanguard … 6 USAA (bank), 7 Mortgage, 8 House.

use std::collections::VecDeque;
use std::iter::once;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tracing::Instrument;

use super::*;
use crate::api::funding::{AccountCount, FundingDiagnostics, YearCount};
use crate::api::runs::{
    AccountSeries, Band, CashFlow, InflationPoint, RealNetWorthSummary, RealQuantilePoint,
    RealTerminalStats, Results, Stats,
};
use crate::compile::rows::ScenarioGraph;
use crate::suggest::rules::{Draft, DraftPath};
use crate::suggest::{read, resolve};

const KEY: &str = "sk-or-v1-test-SECRET-123";

fn graph() -> ScenarioGraph {
    serde_json::from_str(include_str!("../testdata/default_snapshot.json")).unwrap()
}

fn results() -> Results {
    let dates: Vec<String> = once("2026-09-03".to_owned())
        .chain((2026..=2095).map(|y| format!("{y}-12-31")))
        .chain(once("2096-09-03".to_owned()))
        .collect();
    Results {
        run_id: 7,
        scenario_id: 1,
        stats: Stats {
            num_iterations: 2000,
            success_rate: 0.909,
            funding_success_rate: Some(0.9055),
            mean_final_net_worth: 0.0,
            std_dev_final_net_worth: 0.0,
            min_final_net_worth: 0.0,
            max_final_net_worth: 0.0,
            lifetime_taxes: 0.0,
            converged: None,
            convergence_metric: None,
            convergence_value: None,
            percentile_values: Vec::new(),
        },
        bands: vec![Band {
            path_id: "0.5".into(),
            percentile: Some(0.5),
            dates: dates.clone(),
            net_worth: vec![0.0; dates.len()],
            inflation: vec![1.0; dates.len()],
        }],
        real_net_worth: Some(RealNetWorthSummary {
            terminal: RealTerminalStats {
                base_date: "2026-09-03".into(),
                num_iterations: 2000,
                mean: 0.0,
                std_dev: 0.0,
                min: 0.0,
                max: 0.0,
            },
            points: vec![RealQuantilePoint {
                date: "2096-09-03".into(),
                p5: -1_600_000.0,
                p10: Some(1_370_000.0),
                p25: Some(20_900_000.0),
                p50: 71_900_000.0,
                p75: Some(208_000_000.0),
                p90: Some(472_000_000.0),
                p95: 788_000_000.0,
            }],
        }),
        path_details: true,
        series_id: "0.5".into(),
        account_series: (1..=8)
            .map(|id| AccountSeries {
                account_id: id,
                label: format!("account {id}"),
                values: vec![1000.0 * id as f64; dates.len()],
            })
            .collect(),
        series_percentile: Some(0.5),
        cash_flows: (2026..=2095)
            .map(|year| CashFlow {
                year,
                income: if year < 2036 { 205_000.0 } else { 0.0 },
                expenses: 100_000.0,
                contributions: 0.0,
                withdrawals: 0.0,
                appreciation: 0.0,
                net_cash_flow: 0.0,
                taxes: 0.0,
            })
            .collect(),
        warnings: Vec::new(),
        inflation: (2026..=2095)
            .map(|year| InflationPoint { year, factor: 1.0 })
            .collect(),
        ledger_years: Vec::new(),
        funding_diagnostics: Some(FundingDiagnostics {
            iterations: 2000,
            failed: 189,
            cash_shortfall: 180,
            event_failure: 9,
            iteration_limit: 0,
            failed_solvent: 12,
            first_shortfall_years: vec![YearCount {
                year: 2074,
                count: 180,
            }],
            median_first_shortfall_year: Some(2074),
            shortfall_accounts: vec![AccountCount {
                account_id: Some(6),
                count: 170,
            }],
            event_failures: Vec::new(),
            liquid_depleted_years: Vec::new(),
            median_max_deficit: Some(38_200.0),
            median_shortfall_years: Some(6.0),
            worst_seed: Some("123".into()),
        }),
    }
}

/// Remove Home Purchase's Sweep, `expect` copied from the plan.
fn remove_sweep(graph: &ScenarioGraph) -> Value {
    let body = read::event(graph, 5).unwrap();
    let effect = body["effects"][0].clone();
    assert_eq!(effect["kind"], "Sweep");
    json!([{"op": "remove", "target": {"event": 5}, "path": "/effects/0", "expect": effect}])
}

fn settings() -> Settings {
    Settings {
        model: DEFAULT_MODEL.into(),
        max_turns: 12,
        max_suggestions: 6,
        max_previews: 8,
        max_tokens: 16_000,
        thinking: true,
        effort: "high",
        max_retries: 3,
        retry_base: Duration::ZERO,
        retry_cap: Duration::ZERO,
        materiality: Materiality::default(),
    }
}

// ── doubles ─────────────────────────────────────────────────────────────────

/// Plays back replies written as JSON, recording each request as the JSON
/// that would have gone on the wire.
#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<Result<Value, TransportError>>>,
    requests: Mutex<Vec<Value>>,
    /// What `price` answers, and how often it was asked.
    price: Option<ModelPrice>,
    price_lookups: Mutex<u32>,
}

impl Script {
    fn new(replies: Vec<Result<Value, TransportError>>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            ..Self::default()
        })
    }

    fn priced(replies: Vec<Result<Value, TransportError>>, price: ModelPrice) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            price: Some(price),
            ..Self::default()
        })
    }

    fn price_lookups(&self) -> u32 {
        *self.price_lookups.lock().unwrap()
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}

impl Transport for Script {
    fn send<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Reply, TransportError>> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap()
                .push(serde_json::to_value(request).unwrap());
            let next = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(TransportError::Network("script ran out".into())))?;
            Ok(serde_json::from_value(next).expect("scripted reply is a Messages reply"))
        })
    }

    fn price<'a>(
        &'a self,
        _model: &'a str,
    ) -> BoxFuture<'a, Result<Option<ModelPrice>, TransportError>> {
        Box::pin(async move {
            *self.price_lookups.lock().unwrap() += 1;
            Ok(self.price)
        })
    }
}

struct Tools {
    graph: ScenarioGraph,
    previews: Mutex<u32>,
    /// The `base` and `edited` statistics every clean preview reports.
    stats: (Value, Value),
}

impl Tools {
    fn new() -> Self {
        Self::reporting(
            json!({"funding_success_rate": 0.9055}),
            json!({"funding_success_rate": 0.908}),
        )
    }

    fn reporting(base: Value, edited: Value) -> Self {
        Self {
            graph: graph(),
            previews: Mutex::new(0),
            stats: (base, edited),
        }
    }
}

impl ReviewTools for Tools {
    fn preview<'a>(&'a self, changes: Vec<Change>) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            *self.previews.lock().unwrap() += 1;
            Ok(match resolve(&self.graph, &changes) {
                Ok(_) => json!({
                    "problems": [], "paired": true,
                    "base": self.stats.0, "edited": self.stats.1
                }),
                Err(problems) => json!({"problems": problems}),
            })
        })
    }

    fn resolve_steps(
        &self,
        steps: &[Vec<Change>],
    ) -> Result<Vec<Vec<DiffLine>>, (usize, Vec<ChangeProblem>)> {
        match crate::suggest::resolve_steps(&self.graph, steps, &Default::default()).unwrap() {
            Ok(stepped) => Ok(stepped
                .steps
                .iter()
                .map(|s| s.diff(std::iter::empty()))
                .collect()),
            Err(failed) => Err((failed.step, failed.problems)),
        }
    }
}

fn reply(stop: &str, content: Value) -> Result<Value, TransportError> {
    Ok(json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": DEFAULT_MODEL,
        "content": content, "stop_reason": stop,
        "usage": {"input_tokens": 100, "output_tokens": 20,
                  "cache_creation_input_tokens": 10, "cache_read_input_tokens": 50}
    }))
}

fn call(id: &str, name: &str, input: Value) -> Value {
    json!({"type": "tool_use", "id": id, "name": name, "input": input})
}

/// One path of one step making `changes`.
fn path_of(changes: Value) -> Value {
    json!({
        "key": "a", "label": "Remove the sweep", "recommended": true,
        "steps": [{"key": "a", "title": "Remove the sweep", "changes": changes}]
    })
}

fn good_note(changes: Value) -> Value {
    json!({
        "kind": "fix", "section": "plan", "motive": "correctness",
        "title": "Home Purchase sells investments while USAA already holds the down payment",
        "reasoning": "The Sweep sells from Vanguard before the down payment even though USAA covers it. Removing it lifts funding from 90.6% to 90.8% in a paired preview.",
        "evidence": [
            {"ref": "ledger", "year": 2031, "event_id": 5, "account_id": null},
            {"ref": "account_series", "account_id": 6, "date": "2030-12-31", "value": 6000},
            {"ref": "diagnostic", "field": "cash_shortfall", "value": 180}
        ],
        "paths": [path_of(changes)]
    })
}

fn context(rules: &[Draft]) -> ReviewContext {
    ReviewContext::build(&graph(), &results(), rules)
}

// ── the loop ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn preview_then_a_rejected_then_an_accepted_note() {
    let g = graph();
    let changes = remove_sweep(&g);
    let script = Script::new(vec![
        Err(TransportError::Status {
            status: 529,
            error_type: None,
            message: "Overloaded".into(),
        }),
        reply(
            "tool_use",
            json!([
                {"type": "thinking", "thinking": "", "signature": "sig-1"},
                call("t1", "preview_changes", json!({"changes": changes}))
            ]),
        ),
        reply(
            "tool_use",
            json!([call(
                "t2",
                "submit_suggestion",
                json!({
                    "kind": "read", "section": "plan",
                    "title": "x".repeat(200),
                    "reasoning": "Too long a title, a read note with changes, an unknown account.",
                    "evidence": [
                        {"ref": "ledger", "year": 1999},
                        {"ref": "ledger", "year": 2031, "event_id": 404}
                    ],
                    "paths": [path_of(changes.clone())]
                })
            )]),
        ),
        reply(
            "tool_use",
            json!([call("t3", "submit_suggestion", good_note(changes.clone()))]),
        ),
        reply(
            "end_turn",
            json!([{"type": "text", "text": "One note submitted."}]),
        ),
    ]);
    let client = AiClient::new(settings(), script.clone(), Some(KEY.into()));
    let tools = Tools::new();

    let outcome = generate(&client, &context(&[]), &tools).await.unwrap();

    assert_eq!(outcome.stop, Stop::Finished);
    assert_eq!(outcome.summary.as_deref(), Some("One note submitted."));
    assert_eq!(outcome.model.as_deref(), Some(DEFAULT_MODEL));
    assert_eq!(outcome.drafts.len(), 1);
    let draft = &outcome.drafts[0];
    assert_eq!(draft.kind, Kind::Fix);
    let [path] = draft.paths.as_slice() else {
        panic!("one path");
    };
    assert!(path.recommended);
    assert!(path.previewed);
    assert_eq!(
        path.preview.as_ref().unwrap()["edited"]["funding_success_rate"],
        0.908
    );
    assert!(path.estimate.is_none());
    let diff = &path.steps[0].diff;
    assert!(!diff.is_empty(), "accepted notes carry the server's diff");
    assert!(diff[0].label.contains("Home Purchase"));

    assert_eq!(outcome.usage.turns, 4, "retries are not turns");
    assert_eq!(outcome.usage.previews, 1);
    assert_eq!(outcome.usage.rejected, 1);
    assert_eq!(outcome.usage.input_tokens, 400);
    assert_eq!(outcome.usage.cache_read_input_tokens, 200);
    assert_eq!(*tools.previews.lock().unwrap(), 1);

    let requests = script.requests();
    assert_eq!(requests.len(), 5, "one retry, then four turns");
    let first = &requests[0];
    assert_eq!(first["model"], DEFAULT_MODEL);
    assert_eq!(first["thinking"]["type"], "adaptive");
    assert_eq!(first["output_config"]["effort"], "high");
    assert!(first.get("fallbacks").is_none());
    assert_eq!(first["provider"]["require_parameters"], true);
    assert_eq!(first["provider"]["data_collection"], "deny");
    assert_eq!(first["system"][1]["cache_control"]["type"], "ephemeral");
    assert!(first["system"][0].get("cache_control").is_none());
    assert_eq!(first["tools"][0]["name"], "preview_changes");
    assert_eq!(first["tools"][1]["name"], "submit_suggestion");
    assert!(first["tools"][0]["input_schema"]["properties"]["steps"].is_object());
    assert!(first["tools"][1]["input_schema"]["properties"]["paths"].is_object());
    // The newest block carries the conversation's breakpoint.
    assert_eq!(
        first["messages"][0]["content"][1]["cache_control"]["type"],
        "ephemeral"
    );
    // The retry resent the same body.
    assert_eq!(requests[0], requests[1]);

    // Turn 2 echoes the assistant turn, thinking block included, then the result.
    let second = &requests[2]["messages"];
    assert_eq!(second[1]["role"], "assistant");
    assert_eq!(second[1]["content"][0]["type"], "thinking");
    assert_eq!(second[1]["content"][0]["signature"], "sig-1");
    assert_eq!(second[1]["content"][1]["type"], "tool_use");
    assert_eq!(second[2]["content"][0]["tool_use_id"], "t1");
    assert!(second[2]["content"][0].get("is_error").is_none());
    // Only the newest block is a breakpoint: the first message's moved on.
    assert!(second[0]["content"][1].get("cache_control").is_none());
    assert_eq!(
        second[2]["content"][0]["cache_control"]["type"],
        "ephemeral"
    );

    // The bad submission came back as an error listing every problem.
    let rejected = &requests[3]["messages"][4]["content"][0];
    assert_eq!(rejected["tool_use_id"], "t2");
    assert_eq!(rejected["is_error"], true);
    let text = rejected["content"].as_str().unwrap();
    for needle in [
        "title must be",
        "a read note offers no paths",
        "motive is required",
        "year 1999",
        "no event #404",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in {text}");
    }

    // The static prefix is identical across turns, so the cache can hit.
    for r in &requests {
        assert_eq!(r["system"], first["system"]);
        assert_eq!(r["tools"], first["tools"]);
    }
}

#[tokio::test]
async fn unpreviewed_changes_are_sent_back_until_the_budget_is_spent() {
    let g = graph();
    let changes = remove_sweep(&g);
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call("t1", "submit_suggestion", good_note(changes.clone()))]),
        ),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script.clone(), None);
    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    assert!(outcome.drafts.is_empty());
    let result = &script.requests()[1]["messages"][2]["content"][0];
    assert!(
        result["content"]
            .as_str()
            .unwrap()
            .contains("preview exactly these steps")
    );

    // With no previews to spend, an estimate stands in.
    let mut note = good_note(changes);
    note["paths"][0]["estimate"] = json!({"success_rate": null, "funding_success_rate": 0.91});
    let script = Script::new(vec![
        reply("tool_use", json!([call("t1", "submit_suggestion", note)])),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(
        Settings {
            max_previews: 0,
            ..settings()
        },
        script,
        None,
    );
    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    assert_eq!(outcome.drafts.len(), 1);
    let path = &outcome.drafts[0].paths[0];
    assert!(!path.previewed);
    assert!(path.preview.is_none());
    assert_eq!(
        path.estimate.as_ref().unwrap().funding_success_rate,
        Some(0.91)
    );
}

/// Halve Home Purchase's Sweep instead of removing it.
fn halve_sweep(graph: &ScenarioGraph) -> Value {
    let body = read::event(graph, 5).unwrap();
    let value = body["effects"][0]["amount"]["inner"]["value"].clone();
    assert!(value.is_number(), "{body}");
    json!([{"op": "replace", "target": {"event": 5},
            "path": "/effects/0/amount/inner/value", "expect": value,
            "value": value.as_f64().unwrap() / 2.0}])
}

#[tokio::test]
async fn a_note_offers_several_paths_each_previewed() {
    let g = graph();
    let remove = remove_sweep(&g);
    let halve = halve_sweep(&g);
    let mut note = good_note(remove.clone());
    note["paths"] = json!([
        path_of(remove.clone()),
        {"key": "halve", "label": "Sweep half as much", "recommended": false,
         "reasoning": "Keeps some of the sale.",
         "steps": [{"key": "a", "title": "Halve the sweep", "changes": halve}]}
    ]);
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call("t1", "preview_changes", json!({"steps": [remove]}))]),
        ),
        // The second path was never previewed: sent back.
        reply(
            "tool_use",
            json!([call("t2", "submit_suggestion", note.clone())]),
        ),
        reply(
            "tool_use",
            json!([call("t3", "preview_changes", json!({"steps": [halve]}))]),
        ),
        reply("tool_use", json!([call("t4", "submit_suggestion", note)])),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script.clone(), None);
    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();

    let rejected = &script.requests()[2]["messages"][4]["content"][0];
    assert_eq!(rejected["is_error"], true);
    let text = rejected["content"].as_str().unwrap();
    assert!(
        text.contains(r#"path \"halve\": preview exactly these steps"#),
        "{text}"
    );

    let [draft] = outcome.drafts.as_slice() else {
        panic!("one note");
    };
    let [a, halve] = draft.paths.as_slice() else {
        panic!("two paths");
    };
    assert!(a.recommended && !halve.recommended);
    assert!(a.previewed && halve.previewed);
    assert!(a.preview.is_some() && halve.preview.is_some());
    assert_eq!(halve.reasoning.as_deref(), Some("Keeps some of the sale."));
    assert!(halve.steps[0].diff[0].label.contains("Home Purchase"));
    assert_eq!(outcome.usage.previews, 2);
    assert_eq!(outcome.usage.rejected, 1);
}

#[tokio::test]
async fn identical_paths_are_refused() {
    let g = graph();
    let remove = remove_sweep(&g);
    let mut note = good_note(remove.clone());
    let mut twin = path_of(remove.clone());
    twin["key"] = json!("b");
    twin["recommended"] = json!(false);
    note["paths"].as_array_mut().unwrap().push(twin);
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call("t1", "preview_changes", json!({"steps": [remove]}))]),
        ),
        reply("tool_use", json!([call("t2", "submit_suggestion", note)])),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script.clone(), None);
    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    assert!(outcome.drafts.is_empty());
    let text = script.requests()[2]["messages"][4]["content"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(text.contains("make the same changes"), "{text}");
}

#[tokio::test]
async fn notes_the_rules_already_wrote_are_refused() {
    let g = graph();
    let changes = remove_sweep(&g);
    let rule = Draft {
        rule: "sweep_sells_while_cash",
        kind: Kind::Fix,
        section: Section::Plan,
        title: "Something else entirely".into(),
        reasoning: String::new(),
        evidence: Vec::new(),
        paths: vec![DraftPath::only(
            "Remove the sweep",
            serde_json::from_value(changes.clone()).unwrap(),
        )],
    };
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call("t1", "preview_changes", json!({"changes": changes}))]),
        ),
        reply(
            "tool_use",
            json!([call("t2", "submit_suggestion", good_note(changes))]),
        ),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script.clone(), None);
    let outcome = generate(&client, &context(&[rule]), &Tools::new())
        .await
        .unwrap();
    assert!(outcome.drafts.is_empty());
    let result = &script.requests()[2]["messages"][4]["content"][0];
    assert!(
        result["content"]
            .as_str()
            .unwrap()
            .contains("repeats an existing note")
    );
}

#[tokio::test]
async fn caps_end_the_conversation() {
    let g = graph();
    let changes = remove_sweep(&g);
    let preview = || {
        reply(
            "tool_use",
            json!([call(
                "t",
                "preview_changes",
                json!({"changes": changes.clone()})
            )]),
        )
    };

    // Turns.
    let script = Script::new(vec![preview(), preview(), preview()]);
    let client = AiClient::new(
        Settings {
            max_turns: 2,
            ..settings()
        },
        script.clone(),
        None,
    );
    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    assert_eq!(outcome.stop, Stop::TurnLimit);
    assert_eq!(script.requests().len(), 2);

    // Previews: the third is refused without running.
    let script = Script::new(vec![
        preview(),
        preview(),
        preview(),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(
        Settings {
            max_previews: 2,
            ..settings()
        },
        script.clone(),
        None,
    );
    let tools = Tools::new();
    let outcome = generate(&client, &context(&[]), &tools).await.unwrap();
    assert_eq!(outcome.usage.previews, 2);
    assert_eq!(*tools.previews.lock().unwrap(), 2);
    let third = &script.requests()[3]["messages"][6]["content"][0];
    assert_eq!(third["is_error"], true);

    // Notes: the loop stops once the last allowed note is accepted.
    let script = Script::new(vec![
        preview(),
        reply(
            "tool_use",
            json!([call("t2", "submit_suggestion", good_note(changes.clone()))]),
        ),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(
        Settings {
            max_suggestions: 1,
            ..settings()
        },
        script.clone(),
        None,
    );
    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    assert_eq!(outcome.stop, Stop::SuggestionLimit);
    assert_eq!(outcome.drafts.len(), 1);
    assert_eq!(script.requests().len(), 2);
}

#[tokio::test]
async fn refusals_and_truncated_replies_stop_without_running_tools() {
    let script = Script::new(vec![Ok(json!({
        "model": DEFAULT_MODEL, "content": [], "stop_reason": "refusal",
        "stop_details": {"type": "refusal", "category": "cyber", "explanation": null},
        "usage": {"input_tokens": 1, "output_tokens": 0}
    }))]);
    let client = AiClient::new(settings(), script, None);
    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    assert_eq!(
        outcome.stop,
        Stop::Refused {
            category: Some("cyber".into())
        }
    );

    let g = graph();
    let script = Script::new(vec![reply(
        "max_tokens",
        json!([call(
            "t1",
            "preview_changes",
            json!({"changes": remove_sweep(&g)})
        )]),
    )]);
    let client = AiClient::new(settings(), script, None);
    let tools = Tools::new();
    let outcome = generate(&client, &context(&[]), &tools).await.unwrap();
    assert_eq!(outcome.stop, Stop::MaxTokens);
    assert_eq!(*tools.previews.lock().unwrap(), 0);
}

#[tokio::test]
async fn failures_before_and_after_the_first_turn() {
    // A 400 is not retried and, on the first turn, is the review's error.
    let script = Script::new(vec![Err(TransportError::Status {
        status: 400,
        error_type: None,
        message: "bad".into(),
    })]);
    let client = AiClient::new(settings(), script.clone(), None);
    let error = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap_err();
    assert!(matches!(error, AiError::Api(_)));
    assert_eq!(script.requests().len(), 1);

    // After an accepted note, a failure keeps the note.
    let g = graph();
    let changes = remove_sweep(&g);
    let overloaded = || {
        Err(TransportError::Status {
            status: 502,
            error_type: Some("provider:Anthropic".into()),
            message: "Overloaded".into(),
        })
    };
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call("t1", "preview_changes", json!({"changes": changes}))]),
        ),
        reply(
            "tool_use",
            json!([call("t2", "submit_suggestion", good_note(changes.clone()))]),
        ),
        overloaded(),
        overloaded(),
        overloaded(),
        overloaded(),
    ]);
    let client = AiClient::new(settings(), script.clone(), None);
    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    assert_eq!(outcome.drafts.len(), 1);
    assert!(matches!(outcome.stop, Stop::Interrupted { .. }));
    assert_eq!(script.requests().len(), 6, "two turns, then 1 + 3 retries");
}

/// Captures what the subscriber writes, to check the key never reaches logs.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

/// Route this thread's events into `captured` for the guard's lifetime.
///
/// A thread-local subscriber alone is racy: tests on other threads, with no
/// subscriber, can register a callsite while none is interested and cache it
/// as `never`, and this thread then misses that event (seen as a missing
/// `review_ai.retry` about one run in four). A global subscriber that answers
/// `sometimes` and records nothing keeps every callsite re-checked per event.
fn capture(captured: &Captured) -> tracing::subscriber::DefaultGuard {
    static KEEPALIVE: std::sync::Once = std::sync::Once::new();
    KEEPALIVE.call_once(|| {
        let _ = tracing::subscriber::set_global_default(Keepalive);
    });
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    tracing::callsite::rebuild_interest_cache();
    guard
}

/// Interested in everything, records nothing: see [`capture`].
struct Keepalive;

impl tracing::Subscriber for Keepalive {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        false
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn the_api_key_never_reaches_errors_or_logs() {
    let captured = Captured::default();
    let _guard = capture(&captured);

    // An error that echoes the key: retried (logged), then fatal.
    let echoing = |status| {
        Err(TransportError::Status {
            status,
            error_type: None,
            message: format!("invalid key {KEY}"),
        })
    };
    let script = Script::new(vec![echoing(529), echoing(401)]);
    let client = AiClient::new(settings(), script, Some(KEY.into()));
    let error = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap_err();

    let message = error.to_string();
    assert!(!message.contains(KEY), "{message}");
    assert!(message.contains("<redacted>"));
    // Whatever reached the subscriber is clean. (Whether the retry line
    // reaches it at all depends on other test threads' subscribers, so the
    // scrub every logged error goes through is checked directly.)
    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(!logs.contains(KEY), "{logs}");
    assert_eq!(client.scrub(&format!("a {KEY} b")), "a <redacted> b");

    let config = AiConfig {
        enabled: true,
        openrouter_api_key: Some(KEY.into()),
        ..AiConfig::default()
    };
    assert!(!format!("{config:?}").contains(KEY));
}

// ── pieces ──────────────────────────────────────────────────────────────────

#[test]
fn the_context_reads_like_the_plan_and_run() {
    let text = context(&[]).text;
    for needle in [
        "<plan>",
        "Scenario \"Default\"",
        "Born 1996-01-01",
        "Inflation: US Historical (stochastic) (Normal mean 3.47%",
        "Federal brackets: 10.00% from $0, 12.00% from $11,600",
        "#9 S&P 500 Bootstrap: Bootstrap of historical sp500 returns, block 5 years",
        "\"name\":\"Home Purchase\"",
        "\"kind\":\"Sweep\"",
        "#6 USAA: {",
        "Bank: cash $250,000.",
        "Liability: principal $0 at 6.00% interest, no repayment term",
        "units x $708.60 =",
        "unrealized gain",
        "<run id=\"7\">",
        "Success rate 90.90%. Funding success rate 90.55%.",
        "P50 $71,900,000",
        "189 of 2000 failed the funding check",
        "USAA (#6) 170",
        "2074 (age 78): 180",
        "Median largest deficit $38,200",
        "year,age,income,expenses",
        "2036,40,0,100000",
        "date,Vanguard (#1)",
        "2030-12-31,1000,",
        "<existing_notes>\nNone.",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    // Every fifth year end, plus the ends; not every year.
    assert!(!text.contains("\n2031-12-31,"));
    assert!(text.contains("\n2096-09-03,"));
}

#[test]
fn existing_notes_are_listed_with_what_they_change() {
    let g = graph();
    let rule = Draft {
        rule: "r",
        kind: Kind::Fix,
        section: Section::Plan,
        title: "Home Purchase sweep".into(),
        reasoning: String::new(),
        evidence: Vec::new(),
        paths: vec![DraftPath::only(
            "Remove the sweep",
            serde_json::from_value(remove_sweep(&g)).unwrap(),
        )],
    };
    let text = context(&[rule]).text;
    assert!(
        text.contains(
            "- [fix / plan] Home Purchase sweep\n  path \"a\", recommended: Remove the sweep (changes: event:5/effects/0)"
        ),
        "{text}"
    );
}

#[test]
fn the_static_prefix_is_stable_and_names_the_body_types() {
    let reference = prompt::reference();
    assert_eq!(reference, prompt::reference());
    for needle in [
        "no correlation between profiles",
        "not inflation-indexed",
        "type EventBody",
        "type EffectSpec",
        "type Change",
        "type Evidence",
    ] {
        assert!(reference.contains(needle), "missing {needle:?}");
    }
    assert_eq!(prompt::tools(), prompt::tools());
    assert!(
        !prompt::SYSTEM_PROMPT.contains('{'),
        "no templating in the system prompt"
    );
}

#[test]
fn evidence_is_checked_against_the_run() {
    let ctx = context(&[]);
    let client = AiClient::new(settings(), Script::new(Vec::new()), None);
    let tools = Tools::new();
    let session = Session {
        client: &client,
        context: &ctx,
        tools: &tools,
        observer: &NoObserver,
        drafts: Vec::new(),
        usage: Usage::default(),
        previewed: HashMap::new(),
    };
    let ok = |e: Value| session.check_evidence(&serde_json::from_value(e).unwrap());
    assert!(ok(json!({"ref": "ledger", "year": 2040, "event_id": 5, "account_id": 6})).is_ok());
    assert!(ok(json!({"ref": "ledger", "year": 2200})).is_err());
    assert!(
        ok(json!({"ref": "account_series", "account_id": 6, "date": "2031-06-30", "value": 1}))
            .is_err()
    );
    assert!(
        ok(json!({"ref": "account_series", "account_id": 42, "date": "2030-12-31", "value": 1}))
            .is_err()
    );
    assert!(ok(json!({"ref": "stat", "name": "withdrawal rate 2038", "value": 0.056})).is_ok());
    assert!(ok(json!({"ref": "diagnostic", "field": "failed_solvent", "value": 12})).is_ok());
    assert!(ok(json!({"ref": "diagnostic", "field": "api_key", "value": 1})).is_err());
}

#[test]
fn config_switches_on_only_with_a_key() {
    let off = AiConfig::default();
    assert!(off.active_key().is_none());
    assert!(!off.missing_key());
    assert!(AiClient::from_config(&off).unwrap().is_none());
    off.validate().unwrap();

    let keyless = AiConfig {
        enabled: true,
        openrouter_api_key: Some("  ".into()),
        ..AiConfig::default()
    };
    assert!(keyless.missing_key());
    assert!(AiClient::from_config(&keyless).unwrap().is_none());

    let on = AiConfig {
        enabled: true,
        openrouter_api_key: Some(KEY.into()),
        ..AiConfig::default()
    };
    let client = AiClient::from_config(&on).unwrap().unwrap();
    assert_eq!(client.settings().model, config::DEFAULT_MODEL);
    assert_eq!(client.settings().effort, "high");
    assert!(
        client.settings().thinking,
        "auto thinks on Anthropic models"
    );

    for bad in [
        AiConfig {
            max_turns: 0,
            ..AiConfig::default()
        },
        AiConfig {
            max_tokens: 10,
            ..AiConfig::default()
        },
        AiConfig {
            openrouter_base_url: "http://example.com".into(),
            ..AiConfig::default()
        },
        AiConfig {
            openrouter_app_title: " ".into(),
            ..AiConfig::default()
        },
        AiConfig {
            openrouter_referer: Some("finplan.example".into()),
            ..AiConfig::default()
        },
    ] {
        assert!(bad.validate().is_err());
    }
}

#[test]
fn config_defaults_point_at_openrouter() {
    let cfg = AiConfig::default();
    assert_eq!(cfg.openrouter_base_url, "https://openrouter.ai/api/v1");
    assert_eq!(cfg.openrouter_app_title, "FinPlan");
    assert_eq!(cfg.openrouter_referer, None);
    assert_eq!(cfg.model, config::DEFAULT_MODEL);
    assert_eq!(cfg.thinking, ThinkingMode::Auto);
    AiConfig {
        openrouter_base_url: "http://127.0.0.1:9/api/v1".into(),
        openrouter_referer: Some("https://finplan.example".into()),
        ..cfg
    }
    .validate()
    .unwrap();

    // The flags parse from the environment names operators set.
    #[derive(clap::Parser)]
    struct Cli {
        #[command(flatten)]
        ai: AiConfig,
    }
    use clap::Parser;
    let cli = Cli::try_parse_from([
        "finplan-server",
        "--review-ai",
        "true",
        "--openrouter-api-key",
        KEY,
        "--review-model",
        "openai/gpt-5",
        "--review-thinking",
        "off",
    ])
    .unwrap();
    assert!(cli.ai.enabled);
    assert_eq!(cli.ai.active_key(), Some(KEY));
    assert_eq!(cli.ai.thinking, ThinkingMode::Off);
}

#[test]
fn thinking_is_sent_to_anthropic_models_unless_overridden() {
    assert!(ThinkingMode::Auto.applies_to("anthropic/claude-opus-5.5"));
    assert!(!ThinkingMode::Auto.applies_to("openai/gpt-5"));
    assert!(!ThinkingMode::Auto.applies_to("google/gemini-3-pro"));
    assert!(ThinkingMode::On.applies_to("openai/gpt-5"));
    assert!(!ThinkingMode::Off.applies_to("anthropic/claude-opus-5.5"));

    let other = AiConfig {
        enabled: true,
        openrouter_api_key: Some(KEY.into()),
        model: "openai/gpt-5".into(),
        ..AiConfig::default()
    };
    assert!(!Settings::from_config(&other).thinking);

    // Without thinking, neither field goes on the wire.
    let script = Script::new(vec![reply("end_turn", json!([]))]);
    let client = AiClient::new(
        Settings {
            model: "openai/gpt-5".into(),
            thinking: false,
            ..settings()
        },
        script.clone(),
        None,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime
        .block_on(generate(&client, &context(&[]), &Tools::new()))
        .unwrap();
    let request = &script.requests()[0];
    assert_eq!(request["model"], "openai/gpt-5");
    assert!(request.get("thinking").is_none());
    assert!(request.get("output_config").is_none());
    assert_eq!(request["provider"]["require_parameters"], true);
}

#[test]
fn openrouter_errors_map_onto_transport_errors() {
    use openrouter_rs::error::{ApiErrorContext, ApiErrorKind, HttpRequestError, OpenRouterError};

    let api = |status: u16, kind: ApiErrorKind| {
        OpenRouterError::Api(Box::new(ApiErrorContext {
            status: axum::http::StatusCode::from_u16(status).unwrap(),
            api_code: Some(status as i64),
            message: "upstream said no".into(),
            request_id: None,
            metadata: None,
            kind,
        }))
    };

    let rate_limited = TransportError::from(api(429, ApiErrorKind::Generic));
    assert_eq!(
        rate_limited,
        TransportError::Status {
            status: 429,
            error_type: None,
            message: "upstream said no".into(),
        }
    );
    assert!(rate_limited.retryable());

    let provider = TransportError::from(api(
        502,
        ApiErrorKind::Provider {
            provider_name: "Anthropic".into(),
            raw: json!({}),
        },
    ));
    assert!(provider.retryable());
    assert!(provider.to_string().contains("provider:Anthropic"));

    let moderation = TransportError::from(api(
        403,
        ApiErrorKind::Moderation {
            reasons: vec!["x".into()],
            flagged_input: String::new(),
            provider_name: String::new(),
            model_slug: String::new(),
        },
    ));
    assert!(!moderation.retryable());
    assert!(matches!(
        &moderation,
        TransportError::Status { error_type: Some(t), .. } if t == "moderation"
    ));
    assert!(!TransportError::from(api(401, ApiErrorKind::Generic)).retryable());

    let network = TransportError::from(OpenRouterError::HttpRequest(HttpRequestError::new(
        "connection reset",
    )));
    assert_eq!(network, TransportError::Network("connection reset".into()));
    assert!(network.retryable());

    let decode = TransportError::from(OpenRouterError::Unknown(
        "Failed to deserialize messages API response".into(),
    ));
    assert!(matches!(decode, TransportError::Decode(_)));
    assert!(!decode.retryable());

    let unset = TransportError::from(OpenRouterError::KeyNotConfigured);
    assert!(matches!(unset, TransportError::Request(_)));
    assert!(!unset.retryable());
}

#[test]
fn the_real_transport_builds_from_config() {
    let on = AiConfig {
        enabled: true,
        openrouter_api_key: Some(KEY.into()),
        openrouter_referer: Some("https://finplan.example".into()),
        ..AiConfig::default()
    };
    // Builds the client only; no request is sent.
    assert!(AiClient::from_config(&on).unwrap().is_some());
}

#[test]
fn retries_back_off_with_jitter_under_the_cap() {
    let client = AiClient::new(
        Settings {
            retry_base: Duration::from_secs(2),
            retry_cap: Duration::from_secs(30),
            ..settings()
        },
        Script::new(Vec::new()),
        None,
    );
    for _ in 0..50 {
        let first = client.backoff(0);
        assert!(first >= Duration::from_secs(1) && first <= Duration::from_secs(2));
        let late = client.backoff(10);
        assert!(late >= Duration::from_secs(15) && late <= Duration::from_secs(30));
    }
}

// ── observability ───────────────────────────────────────────────────────────

/// Records what the loop reports, in order.
#[derive(Default)]
struct Recorder {
    turns: Mutex<Vec<TurnReport>>,
    tools: Mutex<Vec<(AiTool, AiToolOutcome)>>,
    retries: Mutex<Vec<AiRetryReason>>,
    submissions: Mutex<Vec<bool>>,
    motives: Mutex<Vec<(AiMotive, bool)>>,
}

impl Observer for Recorder {
    fn turn(&self, turn: &TurnReport) {
        self.turns.lock().unwrap().push(turn.clone());
    }
    fn tool(&self, tool: AiTool, outcome: AiToolOutcome, seconds: f64) {
        assert!(seconds >= 0.0);
        self.tools.lock().unwrap().push((tool, outcome));
    }
    fn retry(&self, reason: AiRetryReason) {
        self.retries.lock().unwrap().push(reason);
    }
    fn submission(&self, accepted: bool) {
        self.submissions.lock().unwrap().push(accepted);
    }
    fn motive(&self, motive: AiMotive, accepted: bool) {
        self.motives.lock().unwrap().push((motive, accepted));
    }
}

/// A reply whose usage carries OpenRouter's `cost`.
fn costing(stop: &str, content: Value, cost: f64) -> Result<Value, TransportError> {
    let mut reply = reply(stop, content)?;
    reply["usage"]["cost"] = json!(cost);
    Ok(reply)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[tokio::test]
async fn the_observer_sees_every_turn_tool_retry_and_note_with_reported_cost() {
    let g = graph();
    let changes = remove_sweep(&g);
    let script = Script::priced(
        vec![
            Err(TransportError::Status {
                status: 429,
                error_type: None,
                message: "slow down".into(),
            }),
            costing(
                "tool_use",
                json!([call("t1", "preview_changes", json!({"changes": changes}))]),
                0.01,
            ),
            costing(
                "tool_use",
                json!([call(
                    "t2",
                    "submit_suggestion",
                    json!({"kind": "read", "section": "plan", "title": "t",
                           "reasoning": "r", "paths": [path_of(changes.clone())]})
                )]),
                0.02,
            ),
            costing(
                "tool_use",
                json!([call("t3", "submit_suggestion", good_note(changes.clone()))]),
                0.03,
            ),
            costing("end_turn", json!([]), 0.04),
        ],
        ModelPrice {
            prompt: 1.0,
            completion: 1.0,
            cache_read: 1.0,
            cache_write: 1.0,
        },
    );
    let client = AiClient::new(settings(), script.clone(), None);
    let recorder = Recorder::default();

    let outcome = generate_observed(&client, &context(&[]), &Tools::new(), &recorder)
        .await
        .unwrap();

    assert_eq!(outcome.stop, Stop::Finished);
    let turns = recorder.turns.lock().unwrap().clone();
    assert_eq!(turns.len(), 4, "retries are not turns");
    assert_eq!(
        turns.iter().map(|t| t.turn).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert!(turns.iter().all(|t| t.seconds >= 0.0
        && t.input_tokens == 100
        && t.output_tokens == 20
        && t.cache_read_input_tokens == 50
        && t.cache_creation_input_tokens == 10));
    assert!(
        turns
            .iter()
            .all(|t| matches!(t.cost, Some((_, AiCostSource::Reported)))),
        "OpenRouter's own figure wins over the listed prices"
    );
    assert_eq!(
        script.price_lookups(),
        0,
        "prices only matter without a cost"
    );
    assert!(close(outcome.usage.cost_usd, 0.10));
    assert!(close(outcome.usage.cost_reported_usd, 0.10));
    assert_eq!(outcome.usage.cost_estimated_usd, 0.0);
    assert_eq!(outcome.usage.cost_source(), "reported");

    assert_eq!(
        *recorder.tools.lock().unwrap(),
        vec![
            (AiTool::Preview, AiToolOutcome::Ok),
            (AiTool::Submit, AiToolOutcome::Rejected),
            (AiTool::Submit, AiToolOutcome::Accepted),
        ]
    );
    assert_eq!(*recorder.submissions.lock().unwrap(), vec![false, true]);
    assert_eq!(
        *recorder.motives.lock().unwrap(),
        vec![(AiMotive::Missing, false), (AiMotive::Correctness, true)],
        "the read note gave no motive; the accepted one did"
    );
    assert_eq!(
        *recorder.retries.lock().unwrap(),
        vec![AiRetryReason::RateLimit]
    );
}

#[tokio::test]
async fn cost_is_estimated_from_listed_prices_when_replies_report_none() {
    let price = ModelPrice {
        prompt: 0.000_004,
        completion: 0.000_02,
        cache_read: 0.000_000_2,
        cache_write: 0.000_005,
    };
    let per_turn = 100.0 * price.prompt
        + 20.0 * price.completion
        + 50.0 * price.cache_read
        + 10.0 * price.cache_write;
    let script = Script::priced(
        vec![
            reply(
                "tool_use",
                json!([call("t1", "preview_changes", json!({"changes": []}))]),
            ),
            reply("end_turn", json!([])),
            reply("end_turn", json!([])),
        ],
        price,
    );
    let client = AiClient::new(settings(), script.clone(), None);
    let recorder = Recorder::default();

    let outcome = generate_observed(&client, &context(&[]), &Tools::new(), &recorder)
        .await
        .unwrap();
    assert!(close(outcome.usage.cost_estimated_usd, 2.0 * per_turn));
    assert!(close(outcome.usage.cost_usd, 2.0 * per_turn));
    assert_eq!(outcome.usage.cost_source(), "estimated");
    assert!(
        recorder
            .turns
            .lock()
            .unwrap()
            .iter()
            .all(|t| matches!(t.cost, Some((c, AiCostSource::Estimated)) if close(c, per_turn)))
    );
    // A bad preview call is still a tool call, counted as invalid.
    assert_eq!(
        *recorder.tools.lock().unwrap(),
        vec![(AiTool::Preview, AiToolOutcome::Invalid)]
    );

    // The price is looked up once and kept, across turns and passes.
    generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    assert_eq!(script.price_lookups(), 1);
}

#[tokio::test]
async fn without_a_cost_or_a_price_turns_are_counted_as_unpriced() {
    let script = Script::new(vec![reply("end_turn", json!([]))]);
    let client = AiClient::new(settings(), script.clone(), None);
    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    assert_eq!(outcome.usage.cost_usd, 0.0);
    assert_eq!(outcome.usage.unpriced_turns, 1);
    assert_eq!(outcome.usage.cost_source(), "unknown");
    assert_eq!(script.price_lookups(), 1);
}

#[test]
fn openrouter_prices_parse_from_their_listing() {
    let listed = json!({
        "prompt": "0.000004", "completion": "0.00002", "web_search": "0.01",
        "input_cache_read": "0.0000002", "input_cache_write": "0.000005",
        "input_cache_write_1h": "0.000008"
    });
    let price = transport::price_from_json(listed).unwrap();
    assert!(close(price.prompt, 0.000_004));
    assert!(close(price.completion, 0.000_02));
    assert!(close(price.cache_read, 0.000_000_2));
    assert!(close(price.cache_write, 0.000_005));
    assert!(close(price.cost(1_000, 100, 0, 0), 0.006));

    // No cache prices: cached tokens bill as prompt tokens.
    let plain =
        transport::price_from_json(json!({"prompt": "0.001", "completion": "0.002"})).unwrap();
    assert!(close(plain.cache_read, 0.001) && close(plain.cache_write, 0.001));
    // Unparseable or negative: no estimate at all.
    assert!(transport::price_from_json(json!({"prompt": "n/a", "completion": "1"})).is_none());
    assert!(transport::price_from_json(json!({"prompt": "-1", "completion": "1"})).is_none());
}

#[test]
fn retries_are_labelled_by_what_went_wrong() {
    let status = |status, error_type: Option<&str>| TransportError::Status {
        status,
        error_type: error_type.map(str::to_owned),
        message: String::new(),
    };
    assert_eq!(retry_reason(&status(429, None)), AiRetryReason::RateLimit);
    assert_eq!(retry_reason(&status(408, None)), AiRetryReason::Timeout);
    assert_eq!(retry_reason(&status(504, None)), AiRetryReason::Timeout);
    assert_eq!(
        retry_reason(&status(502, Some("provider:Anthropic"))),
        AiRetryReason::Provider
    );
    assert_eq!(retry_reason(&status(529, None)), AiRetryReason::Server);
    assert_eq!(
        retry_reason(&TransportError::Network("operation timed out".into())),
        AiRetryReason::Timeout
    );
    assert_eq!(
        retry_reason(&TransportError::Network("connection reset".into())),
        AiRetryReason::Network
    );
    assert_eq!(stop_label("tool_use"), "tool_use");
    assert_eq!(
        stop_label("<script>anything a provider says</script>"),
        "other"
    );
}

#[tokio::test]
async fn logs_follow_the_callers_span_and_carry_no_model_or_plan_text() {
    let captured = Captured::default();
    let _guard = capture(&captured);

    let g = graph();
    let changes = remove_sweep(&g);
    let script = Script::new(vec![
        Err(TransportError::Status {
            status: 503,
            error_type: None,
            message: "unavailable".into(),
        }),
        costing(
            "tool_use",
            json!([
                {"type": "text", "text": "PRIVATE-MODEL-TEXT thinking out loud"},
                call("t1", "preview_changes", json!({"changes": changes}))
            ]),
            0.01,
        ),
        costing(
            "tool_use",
            json!([call("t2", "submit_suggestion", {
                let mut note = good_note(changes.clone());
                note["title"] = json!("PRIVATE-NOTE-TITLE about Home Purchase");
                note
            })]),
            0.01,
        ),
        costing(
            "end_turn",
            json!([{"type": "text", "text": "PRIVATE-MODEL-TEXT done"}]),
            0.01,
        ),
    ]);
    let client = AiClient::new(settings(), script, Some(KEY.into()));
    let ctx = context(&[]);
    let span = tracing::info_span!("job", request_id = "req-trace-123");
    let outcome = generate(&client, &ctx, &Tools::new())
        .instrument(span)
        .await
        .unwrap();
    assert_eq!(outcome.drafts.len(), 1);

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    for event in [
        "review_ai.started",
        "review_ai.retry",
        "review_ai.turn",
        "review_ai.tool",
        "review_ai.finished",
    ] {
        assert!(logs.contains(event), "no {event} in:\n{logs}");
    }
    for line in logs.lines().filter(|l| l.contains("review_ai.")) {
        assert!(line.contains("req-trace-123"), "uncorrelated: {line}");
    }
    assert!(
        logs.contains("tool=\"submit_suggestion\" outcome=\"accepted\"")
            || logs.contains("tool=submit_suggestion outcome=accepted"),
        "{logs}"
    );
    assert!(
        logs.contains("cost_source=\"reported\"") || logs.contains("cost_source=reported"),
        "{logs}"
    );
    assert!(
        logs.contains("motive=\"correctness\"") || logs.contains("motive=correctness"),
        "{logs}"
    );
    // Nothing the model wrote, nothing of the plan, never the key.
    for private in [
        "PRIVATE-MODEL-TEXT",
        "PRIVATE-NOTE-TITLE",
        "Vanguard",
        "Home Purchase",
        KEY,
    ] {
        assert!(
            !logs.contains(private),
            "{private} leaked into logs:\n{logs}"
        );
    }
}

// ── motives and materiality ─────────────────────────────────────────────────

/// Base and edited statistics shaped like a lot-method switch: funding
/// 99.90% -> 99.95%, the real median $11,768,266 -> $11,816,164.
fn marginal() -> (Value, Value) {
    let real = |p10: f64, p50: f64| {
        json!({"p5": 1.0e6, "p10": p10, "p25": 7.0e6, "p50": p50,
               "p75": 1.6e7, "p90": 2.2e7, "p95": 2.6e7})
    };
    (
        json!({"success_rate": 0.999, "funding_success_rate": 0.999,
               "real_final": real(4.0e6, 11_768_266.0)}),
        json!({"success_rate": 0.9995, "funding_success_rate": 0.9995,
               "real_final": real(4.01e6, 11_816_164.0)}),
    )
}

/// The text of the tool result the `n`th request carried last.
fn last_result(requests: &[Value], n: usize) -> String {
    let messages = requests[n]["messages"].as_array().unwrap();
    let last = messages.last().unwrap();
    last["content"][0]["content"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn a_marginal_optimization_is_sent_back_with_its_deltas() {
    let g = graph();
    let changes = remove_sweep(&g);
    let with_motive = |motive: &str| {
        let mut note = good_note(changes.clone());
        note["motive"] = json!(motive);
        note
    };
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call("t1", "preview_changes", json!({"changes": changes}))]),
        ),
        reply(
            "tool_use",
            json!([call("t2", "submit_suggestion", with_motive("optimization"))]),
        ),
        reply(
            "tool_use",
            json!([call("t3", "submit_suggestion", with_motive("realism"))]),
        ),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script.clone(), None);
    let (base, edited) = marginal();
    let recorder = Recorder::default();

    let outcome = generate_observed(
        &client,
        &context(&[]),
        &Tools::reporting(base, edited),
        &recorder,
    )
    .await
    .unwrap();

    assert_eq!(outcome.drafts.len(), 1, "the realism note is exempt");
    assert_eq!(outcome.usage.rejected, 1);
    let rejected = last_result(&script.requests(), 2);
    for needle in [
        "\"accepted\":false",
        "success +0.05 pts",
        "funding +0.05 pts",
        "real median +0.4%",
        "at least +1 pts on success or funding success, +5% on the real median, or +10% on the real P10",
        "correctness or realism",
    ] {
        assert!(
            rejected.contains(needle),
            "missing {needle:?} in {rejected}"
        );
    }
    assert_eq!(
        *recorder.motives.lock().unwrap(),
        vec![(AiMotive::Optimization, false), (AiMotive::Realism, true)]
    );
}

#[tokio::test]
async fn a_risk_note_clears_a_lower_configured_floor() {
    let g = graph();
    let changes = remove_sweep(&g);
    let mut note = good_note(changes.clone());
    note["motive"] = json!("risk");
    let run = |floor: f64| {
        let note = note.clone();
        let changes = changes.clone();
        async move {
            let script = Script::new(vec![
                reply(
                    "tool_use",
                    json!([call("t1", "preview_changes", json!({"changes": changes}))]),
                ),
                reply("tool_use", json!([call("t2", "submit_suggestion", note)])),
                reply("end_turn", json!([])),
            ]);
            let client = AiClient::new(
                Settings {
                    materiality: Materiality {
                        rate_pts: floor,
                        ..Materiality::default()
                    },
                    ..settings()
                },
                script,
                None,
            );
            // Funding 90.55% -> 90.80%: +0.25 pts.
            generate(&client, &context(&[]), &Tools::new())
                .await
                .unwrap()
        }
    };
    assert_eq!(run(1.0).await.drafts.len(), 0, "+0.25 pts is below 1 pt");
    assert_eq!(run(0.2).await.drafts.len(), 1, "and above 0.2 pts");
}

#[tokio::test]
async fn a_check_needs_a_path_or_a_reason() {
    let bare = json!({
        "kind": "check", "section": "portfolio", "motive": "realism",
        "title": "Every lot's cost basis equals its value on the start date",
        "reasoning": "All lots were bought on the start date at that day's price, so the plan assumes no built-in gains.",
        "evidence": [], "paths": []
    });
    let mut reasoned = bare.clone();
    reasoned["no_change_reason"] = json!("Only the brokerage statements know the real basis");
    let script = Script::new(vec![
        reply("tool_use", json!([call("t1", "submit_suggestion", bare)])),
        reply(
            "tool_use",
            json!([call("t2", "submit_suggestion", reasoned)]),
        ),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script.clone(), None);

    let outcome = generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();

    let rejected = last_result(&script.requests(), 1);
    assert!(rejected.contains("no_change_reason"), "{rejected}");
    assert!(rejected.contains("new_event"), "{rejected}");
    let [draft] = outcome.drafts.as_slice() else {
        panic!("the reasoned check is accepted");
    };
    assert!(draft.paths.is_empty());
    assert!(
        draft
            .reasoning
            .ends_with("no built-in gains. Only the brokerage statements know the real basis."),
        "{}",
        draft.reasoning
    );
}

#[test]
fn materiality_comes_from_config_and_measures_improvements_only() {
    let defaults = Settings::from_config(&AiConfig::default()).materiality;
    assert_eq!(defaults, Materiality::default());
    assert_eq!(
        (defaults.rate_pts, defaults.median_pct, defaults.p10_pct),
        (1.0, 5.0, 10.0)
    );

    let cfg = AiConfig {
        min_rate_pts: 0.01,
        min_median_pct: 0.1,
        min_p10_pct: 50.0,
        ..AiConfig::default()
    };
    assert!(cfg.validate().is_ok());
    let floor = Settings::from_config(&cfg).materiality;
    assert_eq!(
        floor,
        Materiality {
            rate_pts: 0.01,
            median_pct: 0.1,
            p10_pct: 50.0
        }
    );

    let (base, edited) = marginal();
    let marginal = Deltas::of(&json!({"base": base, "edited": edited}));
    assert!(!marginal.clears(&Materiality::default()));
    assert!(marginal.clears(&floor), "+0.05 pts clears a 0.01 pt floor");

    // A drop is never material, however large.
    let worse = Deltas::of(&json!({"base": edited, "edited": base}));
    assert!(!worse.clears(&floor));
    // From debt to a positive P10 clears any percent floor.
    let rescued = Deltas::of(&json!({
        "base": {"success_rate": 0.9, "real_final": {"p10": -5.0e4, "p50": 1.0e6}},
        "edited": {"success_rate": 0.9, "real_final": {"p10": 2.0e4, "p50": 1.0e6}}
    }));
    assert!(rescued.clears(&Materiality::default()));
    assert!(
        rescued
            .describe()
            .contains("real P10 from zero to positive")
    );
    // Nothing comparable: not material.
    assert!(!Deltas::of(&json!({"problems": []})).clears(&floor));

    for bad in [
        AiConfig {
            min_rate_pts: -1.0,
            ..AiConfig::default()
        },
        AiConfig {
            min_rate_pts: f64::NAN,
            ..AiConfig::default()
        },
        AiConfig {
            min_rate_pts: 101.0,
            ..AiConfig::default()
        },
        AiConfig {
            min_median_pct: 2_000.0,
            ..AiConfig::default()
        },
        AiConfig {
            min_p10_pct: f64::INFINITY,
            ..AiConfig::default()
        },
    ] {
        assert!(bad.validate().is_err(), "{bad:?}");
    }
}

#[test]
fn the_submit_tool_requires_a_motive_and_offers_a_no_change_reason() {
    let tools = prompt::tools();
    let submit = &tools[1]["input_schema"];
    let required: Vec<&str> = submit["required"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(required.contains(&"motive"));
    assert!(!required.contains(&"no_change_reason"));
    assert_eq!(
        submit["properties"]["motive"]["enum"],
        json!(["correctness", "realism", "risk", "optimization"])
    );
    assert!(submit["properties"]["no_change_reason"].is_object());
    // The priority order the notes are chosen by is in the cached prompt.
    for needle in [
        "1. correctness",
        "2. realism",
        "3. risk",
        "4. optimization",
        "99.90% -> 99.95%",
        "SSA statement",
    ] {
        assert!(prompt::SYSTEM_PROMPT.contains(needle), "{needle}");
    }
}
