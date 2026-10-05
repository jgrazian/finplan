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
use finplan_plan::graph::ScenarioGraph;
use finplan_plan::results::funding::{AccountCount, FundingDiagnostics, YearCount};
use finplan_plan::results::view::{
    AccountSeries, Band, CashFlow, InflationPoint, RealNetWorthSummary, RealQuantilePoint,
    RealTerminalStats, Results, Stats,
};
use finplan_plan::rules::{Draft, DraftPath};
use finplan_plan::suggest::{read, resolve};

const KEY: &str = "sk-or-v1-test-SECRET-123";

fn graph() -> ScenarioGraph {
    serde_json::from_str(include_str!(
        "../../../../finplan_plan/testdata/default_snapshot.json"
    ))
    .unwrap()
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
            seed: None,
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
                cash: None,
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
                ordinary_income: 0.0,
                early_withdrawal_penalties: 0.0,
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
        thinking: ThinkingMode::On,
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
    /// What `model_info` answers, and how often it was asked.
    info: Option<ModelInfo>,
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
        Self::listed(
            replies,
            ModelInfo {
                price: Some(price),
                reasons: false,
            },
        )
    }

    fn listed(replies: Vec<Result<Value, TransportError>>, info: ModelInfo) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            info: Some(info),
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

    fn model_info<'a>(
        &'a self,
        _model: &'a str,
    ) -> BoxFuture<'a, Result<Option<ModelInfo>, TransportError>> {
        Box::pin(async move {
            *self.price_lookups.lock().unwrap() += 1;
            Ok(self.info)
        })
    }
}

struct Tools {
    graph: ScenarioGraph,
    previews: Mutex<u32>,
    /// The `base` and `edited` statistics every clean preview reports.
    stats: (Value, Value),
    /// Goal seeks asked of the host, as `parameter/metric/target`.
    goal_seeks: Mutex<Vec<String>>,
    /// Sensitivity rankings asked of the host, as their parameter lists.
    sensitivities: Mutex<Vec<Vec<String>>>,
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
            goal_seeks: Mutex::new(Vec::new()),
            sensitivities: Mutex::new(Vec::new()),
        }
    }
}

impl ToolHost for Tools {
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

    fn preflight(&self) -> Result<Value, String> {
        Ok(json!({"issues": [{"code": "no_inflation", "severity": "warning"}], "can_run": true}))
    }

    fn expand_template(
        &self,
        request: &finplan_plan::templates::TemplateRequest,
    ) -> Result<finplan_plan::templates::Expansion, String> {
        request.expand_in(&self.graph).map_err(|e| e.to_string())
    }

    fn goal_seek<'a>(
        &'a self,
        request: tools::goal_seek::GoalSeekRequest,
    ) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            self.goal_seeks.lock().unwrap().push(format!(
                "{}/{}/{}",
                request.parameter,
                request.metric.as_str(),
                request.target
            ));
            if request.parameter == "nothing" {
                return Err("there is no parameter `nothing`".into());
            }
            Ok(json!({
                "found": true, "direction": "smallest",
                "result": {"value": 43.0, "value_text": "age 43", "success_rate": 0.912}
            }))
        })
    }

    fn inspect_path<'a>(
        &'a self,
        rank: tools::PathRank,
        years: Option<(i64, i64)>,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            match rank {
                tools::PathRank::Median => Ok(format!("median path, years {years:?}")),
                _ => Err("this run stored no percentile paths".into()),
            }
        })
    }

    fn cash_flow_breakdown<'a>(
        &'a self,
        rank: tools::PathRank,
        years: Option<(i64, i64)>,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("{} breakdown, years {years:?}", rank.as_str())) })
    }

    fn sensitivity<'a>(
        &'a self,
        request: tools::sensitivity::SensitivityRequest,
    ) -> BoxFuture<'a, Result<Value, tools::sensitivity::SensitivityError>> {
        Box::pin(async move {
            use tools::sensitivity::SensitivityError;
            if request.parameters.iter().any(|p| p == "nothing") {
                return Err(SensitivityError::Refused(
                    "there is no parameter `nothing`".into(),
                ));
            }
            self.sensitivities
                .lock()
                .unwrap()
                .push(request.parameters.clone());
            if request.parameters.iter().any(|p| p == "boom") {
                return Err(SensitivityError::Failed(
                    "the plan failed to simulate".into(),
                ));
            }
            Ok(json!({
                "metric": request.metric.map_or("funding_success_rate", |m| m.as_str()),
                "ranking": [{"parameter": {"name": "Spending"}, "span_points": 12.5}]
            }))
        })
    }

    fn plan_tax_config(&self) -> Option<finplan_core::model::TaxConfig> {
        Some(finplan_core::model::TaxConfig::default())
    }

    fn resolve_steps(
        &self,
        steps: &[Vec<Change>],
    ) -> Result<Vec<Vec<DiffLine>>, (usize, Vec<ChangeProblem>)> {
        match finplan_plan::suggest::resolve_steps(&self.graph, steps, &Default::default()).unwrap()
        {
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
        "summary": "The one-line lead.", "reasoning": "The Sweep sells from Vanguard before the down payment even though USAA covers it. Removing it lifts funding from 90.6% to 90.8% in a paired preview.",
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
                    "reasoning": "Too long a title, no summary, a read note with changes, an unknown account.",
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
    assert_eq!(draft.summary, "The one-line lead.");
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
    // The writing style, the instructions, then the cached reference.
    assert_eq!(first["system"][2]["cache_control"]["type"], "ephemeral");
    assert!(first["system"][0].get("cache_control").is_none());
    assert!(first["system"][1].get("cache_control").is_none());
    let tool = |name: &str| {
        first["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no tool {name}"))
            .clone()
    };
    assert_eq!(first["tools"][0]["name"], "preview_changes");
    assert_eq!(
        first["tools"].as_array().unwrap().last().unwrap()["name"],
        "submit_suggestion"
    );
    assert!(tool("preview_changes")["input_schema"]["properties"]["steps"].is_object());
    assert!(tool("submit_suggestion")["input_schema"]["properties"]["paths"].is_object());
    let required = &tool("submit_suggestion")["input_schema"]["required"];
    assert!(
        required.as_array().unwrap().contains(&json!("summary")),
        "{required}"
    );
    for name in tools::Registry::all().names() {
        assert!(tool(name)["input_schema"].is_object(), "{name}");
    }
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
        "summary must be",
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
        summary: String::new(),
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

/// Two notes that each create an event are told apart by what the events do.
#[test]
fn created_events_are_compared_by_what_they_do() {
    let event = |effects: Value| -> Vec<Change> {
        serde_json::from_value(json!([{
            "op": "add", "target": {"new_event": "e"}, "path": "",
            "value": {"name": "Anything", "trigger": {"kind": "Manual"}, "effects": effects}
        }]))
        .unwrap()
    };
    let social_security = event(json!([{
        "kind": "Income", "to_account_id": 6, "amount": {"kind": "Fixed", "value": 2000},
        "amount_mode": "Gross", "income_type": "Taxable"
    }]));
    let reinvest = event(json!([{
        "kind": "AssetPurchase", "from_account_id": 6, "to_account_id": 7, "asset_id": 3,
        "amount": {"kind": "Fixed", "value": 1}
    }]));
    let reworded = event(json!([{
        "kind": "AssetPurchase", "from_account_id": 6, "to_account_id": 7, "asset_id": 3,
        "amount": {"kind": "Fixed", "value": 999}
    }]));
    assert_ne!(context::edits(&social_security), context::edits(&reinvest));
    assert_eq!(context::edits(&reinvest), context::edits(&reworded));

    use crate::api::suggestions::fingerprint;
    let print = |changes: &[Change]| fingerprint(None, Kind::Fix, changes, "t");
    assert_ne!(print(&social_security), print(&reinvest));
    assert_eq!(print(&reinvest), print(&reworded));
}

#[tokio::test]
async fn notes_still_open_on_the_board_are_listed_and_refused() {
    let g = graph();
    let changes = remove_sweep(&g);
    let open = context(&[]).with_open_notes([(
        Kind::Fix,
        Section::Plan,
        "Home Purchase sweep sells too early",
        serde_json::from_value::<Vec<Change>>(changes.clone()).unwrap(),
    )]);
    assert!(open.text.contains("<open_notes>"), "{}", open.text);
    assert!(open.text.contains("Home Purchase sweep sells too early"));
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
    let outcome = generate(&client, &open, &Tools::new()).await.unwrap();
    assert!(outcome.drafts.is_empty());
    let result = &script.requests()[2]["messages"][4]["content"][0];
    assert!(
        result["content"]
            .as_str()
            .unwrap()
            .contains("repeats an existing note")
    );
}

/// One submission of `good_note` against `context`, after its preview, with
/// `replaces` set as given; the drafts it produced and what the submit call
/// was told.
async fn submit_against(context: ReviewContext, replaces: Option<i64>) -> (Vec<AiDraft>, String) {
    let g = graph();
    let changes = remove_sweep(&g);
    let mut note = good_note(changes.clone());
    if let Some(id) = replaces {
        note["replaces"] = json!(id);
    }
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call("t1", "preview_changes", json!({"changes": changes}))]),
        ),
        reply("tool_use", json!([call("t2", "submit_suggestion", note)])),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script.clone(), None);
    let outcome = generate(&client, &context, &Tools::new()).await.unwrap();
    let told = script.requests()[2]["messages"][4]["content"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    (outcome.drafts, told)
}

fn board_note(id: i64, editable: bool) -> chat::BoardNote<'static> {
    let g = graph();
    chat::BoardNote {
        id,
        kind: Kind::Fix,
        title: "Home Purchase sweep sells too early",
        changes: serde_json::from_value(remove_sweep(&g)).unwrap(),
        editable,
    }
}

#[tokio::test]
async fn an_open_note_can_be_rewritten_in_place_by_naming_it() {
    // Without `replaces`, the twin is refused — and told how to edit it.
    let (drafts, told) =
        submit_against(context(&[]).with_notes([board_note(71, true)]), None).await;
    assert!(drafts.is_empty());
    assert!(told.contains("repeats an existing note"), "{told}");
    assert!(told.contains("\\\"replaces\\\": 71"), "{told}");

    // Naming it rewrites it: the twin is the note itself, not a duplicate.
    let (drafts, told) =
        submit_against(context(&[]).with_notes([board_note(71, true)]), Some(71)).await;
    assert_eq!(drafts.len(), 1, "{told}");
    assert_eq!(drafts[0].replaces, Some(71));
}

#[tokio::test]
async fn only_an_editable_note_on_the_board_can_be_rewritten() {
    // Applied in part, or not open: listed, but not editable.
    let (drafts, told) =
        submit_against(context(&[]).with_notes([board_note(71, false)]), Some(71)).await;
    assert!(drafts.is_empty());
    assert!(told.contains("note #71 cannot be rewritten"), "{told}");
    assert!(
        !told.contains("\\\"replaces\\\": 71"),
        "an uneditable twin is not offered for rewriting: {told}"
    );

    // Not on the board at all — and a review pass lists no editable notes.
    let (drafts, told) = submit_against(context(&[]), Some(9)).await;
    assert!(drafts.is_empty());
    assert!(told.contains("note #9 cannot be rewritten"), "{told}");
}

#[tokio::test]
async fn dismissed_notes_are_listed_as_unwanted_and_refused() {
    let g = graph();
    let changes = remove_sweep(&g);
    let told = context(&[]).with_dismissed_notes([(
        Kind::Fix,
        Section::Plan,
        "Stop the Home Purchase sweep",
        serde_json::from_value::<Vec<Change>>(changes.clone()).unwrap(),
    )]);
    assert!(told.text.contains("<dismissed_notes>"), "{}", told.text);
    assert!(told.text.contains("The user dismissed these notes"));
    assert!(told.text.contains("Stop the Home Purchase sweep"));
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
    let outcome = generate(&client, &told, &Tools::new()).await.unwrap();
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
fn the_context_shows_parameters_yearly_bands_and_withdrawal_rates() {
    let mut g = graph();
    g.parameters.push(finplan_plan::graph::ParameterRow {
        id: 3,
        name: "monthly_expenses".into(),
        kind: "Money".into(),
        number_value: Some(9_500.0),
        date_value: None,
        age_years: None,
        age_months: None,
    });
    let mut r = results();
    r.real_net_worth.as_mut().unwrap().points =
        ["2026-09-03", "2030-12-31", "2031-12-31", "2096-09-03"]
            .iter()
            .enumerate()
            .map(|(i, date)| RealQuantilePoint {
                date: (*date).into(),
                p5: 100.0 * i as f64,
                p10: Some(200.0),
                p25: None,
                p50: 500.0,
                p75: Some(750.0),
                p90: Some(900.0),
                p95: 950.0,
            })
            .collect();
    // Investment accounts 1 to 5 hold 15,000 at every point on the path.
    r.cash_flows[0].withdrawals = 600.0;
    r.cash_flows[5].withdrawals = 1_500.0;
    let text = ReviewContext::build(&g, &r, &[]).text;
    for needle in [
        "Parameters (body as GET /parameters returns it",
        "{\"id\":3,\"name\":\"monthly_expenses\",\"value\":",
        "date,age,p5,p10,p25,p50,p75,p90,p95",
        "2026-09-03,30,0,200,-,500,750,900,950",
        "2030-12-31,34,100,200,-,500,750,900,950",
        "withdrawal_rate is the year's withdrawals",
        // The first year divides by the plan start, later ones by the
        // prior year end.
        "\n2026,30,205000,100000,0,600,0,0,0,4.0%",
        "\n2031,35,205000,100000,0,1500,0,0,0,10.0%",
        "\n2032,36,205000,100000,0,0,0,0,0,0.0%",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    // Every fifth year end, plus the ends.
    assert!(!text.contains("\n2031-12-31,35,"), "{text}");
    // A plan with no parameters says so rather than leaving the list out.
    assert!(context(&[]).text.contains("GET /parameters returns it; amounts and triggers refer to one as `$name`, a change targets it as {\"parameter\": id}):\nNone."));
}

#[test]
fn a_breakdown_names_each_source_and_its_shares() {
    let g = graph();
    let sum = |event_id: Option<i64>, account_id: Option<i64>, bucket: &str, amount: f64| {
        crate::suggest::ai::LedgerSum {
            event_id,
            account_id,
            bucket: bucket.into(),
            amount,
            first_year: 2026,
            last_year: 2040,
        }
    };
    let sums = [
        sum(Some(1), None, "income", 300_000.0),
        sum(Some(1), None, "taxes", -60_000.0),
        sum(Some(5), None, "expenses", -75_000.0),
        sum(None, Some(6), "expenses", -25_000.0),
        sum(Some(6), None, "withdrawals", 40_000.0),
    ];
    let text = render_breakdown(&g, &sums, "median", 0.5, Some((2026, 2040)));
    for needle in [
        "stored P50 path (asked for `median`), 2026 to 2040",
        "source,years,income,expenses,contributions,withdrawals,taxes",
        "Salary (#1),2026-2040,300000,0,0,0,60000",
        "Home Purchase (#5),2026-2040,0,75000,0,0,0",
        "no event: USAA (#6),2026-2040,0,25000,0,0,0",
        "total,-,300000,100000,0,40000,60000",
        "Share of expenses: Home Purchase (#5) 75.0%, no event: USAA (#6) 25.0%.",
        "Share of income: Salary (#1) 100.0%.",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    // The biggest source first.
    assert!(text.find("Salary (#1),").unwrap() < text.find("Home Purchase (#5),").unwrap());
    assert!(render_breakdown(&g, &[], "worst", 0.1, None).contains("No flows in the whole run."));
}

#[test]
fn existing_notes_are_listed_with_what_they_change() {
    let g = graph();
    let rule = Draft {
        rule: "r",
        kind: Kind::Fix,
        section: Section::Plan,
        title: "Home Purchase sweep".into(),
        summary: String::new(),
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
fn the_standard_checks_are_listed_with_what_each_rule_found() {
    use finplan_plan::rules::{CheckLayer, catalogue};
    let note = |title: &str| Draft {
        rule: "idle_bank_cash",
        kind: Kind::Check,
        section: Section::Portfolio,
        title: title.into(),
        summary: String::new(),
        reasoning: String::new(),
        evidence: Vec::new(),
        paths: Vec::new(),
    };
    let text = context(&[note("USAA holds too much"), note("Ally holds too much")]).text;
    let checks = &text[text.find("<plan_checks>").expect("listed")..];
    for check in catalogue() {
        let state = match (check.layer, check.id) {
            (CheckLayer::Rule, "idle_bank_cash") => "ran, wrote 2 notes",
            (CheckLayer::Rule, _) => "ran, found nothing",
            (CheckLayer::Reviewer, _) => "yours",
            (CheckLayer::Preflight, _) => "before every run",
        };
        let layer = match check.layer {
            CheckLayer::Rule => "rule",
            CheckLayer::Reviewer => "reviewer",
            CheckLayer::Preflight => "preflight",
        };
        let line = format!("- {} ({layer}, ", check.id);
        let at = checks
            .find(&line)
            .unwrap_or_else(|| panic!("missing {line:?} in:\n{checks}"));
        let rest = &checks[at..];
        let row = &rest[..rest.find('\n').unwrap()];
        assert!(
            row.contains(&format!("): {state}. Looks for: {}", check.looks_for)),
            "{row}"
        );
    }
    assert!(checks.contains(
        "- rmd_missing (rule, fix / plan): ran, found nothing. Looks for: Tax-deferred money"
    ));
    // Spec 21's conversion checks: the rule's state, and the reviewer's own.
    assert!(checks.contains(
        "- roth_conversion_opportunity (rule, fix / plan): ran, found nothing. Looks for: \
         Pre-tax money whose RMDs fall due inside the plan"
    ));
    assert!(checks.contains(
        "- early_withdrawal_penalties (reviewer, fix / plan): yours. Looks for: \
         Early-withdrawal penalties"
    ));
    let conversion = Draft {
        rule: "roth_conversion_opportunity",
        kind: Kind::Fix,
        section: Section::Plan,
        ..note("401(k)'s RMDs from 2071 follow years that leave $2M unused")
    };
    let text = context(&[conversion]).text;
    assert!(text.contains("- roth_conversion_opportunity (rule, fix / plan): ran, wrote 1 note."));
    // Tax checks are judged on what previews report for them.
    assert!(prompt::SYSTEM_PROMPT.contains("after_tax_final, lifetime_taxes"));
    // The model is told not to redo what ran.
    assert!(prompt::SYSTEM_PROMPT.contains("Do not redo a check marked ran"));
    assert!(!prompt::SYSTEM_PROMPT.contains("Do not repeat a note the rules already wrote"));
}

#[test]
fn the_static_prefix_is_stable_and_names_the_body_types() {
    let reference = prompt::reference();
    assert_eq!(reference, prompt::reference());
    for needle in [
        "no correlation between profiles",
        "indexed each year to the path's simulated inflation",
        "type EventBody",
        "type EffectSpec",
        "type Change",
        "type Evidence",
    ] {
        assert!(reference.contains(needle), "missing {needle:?}");
    }
    let registry = tools::Registry::all();
    assert_eq!(prompt::tools(&registry), prompt::tools(&registry));
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
        computed: HashMap::new(),
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
    assert_eq!(client.settings().thinking, ThinkingMode::Auto);

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
fn every_request_asks_for_compact_technical_writing_ahead_of_its_instructions() {
    let script = Script::new(vec![reply("end_turn", json!([]))]);
    let request = first_request("z-ai/glm-5.3-flash", ThinkingMode::On, script);
    let system = request["system"].as_array().unwrap();
    assert_eq!(system[0]["text"], prompt::WRITING_STYLE);
    assert!(prompt::WRITING_STYLE.contains("ASD-STE100"));
    assert!(
        system[1]["text"]
            .as_str()
            .unwrap()
            .starts_with("You review")
    );
}

#[test]
fn auto_thinking_follows_the_listing_and_falls_back_to_anthropic() {
    // Listed: whatever the listing says, whoever made the model.
    assert!(ThinkingMode::Auto.applies_to("qwen/qwen3.8-max-prime", Some(true)));
    assert!(ThinkingMode::Auto.applies_to("openai/gpt-6-sol", Some(true)));
    assert!(!ThinkingMode::Auto.applies_to("openai/gpt-4.1", Some(false)));
    // Unknown: only Anthropic models.
    assert!(ThinkingMode::Auto.applies_to("anthropic/claude-opus-5.5", None));
    assert!(!ThinkingMode::Auto.applies_to("z-ai/glm-5.3-flash", None));
    // Explicit settings ignore the listing.
    assert!(ThinkingMode::On.applies_to("openai/gpt-4.1", Some(false)));
    assert!(!ThinkingMode::Off.applies_to("anthropic/claude-opus-5.5", Some(true)));
}

/// The first request a one-turn review sends to `model` under `thinking`.
fn first_request(model: &str, thinking: ThinkingMode, script: Arc<Script>) -> Value {
    let client = AiClient::new(
        Settings {
            model: model.into(),
            thinking,
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
    script.requests()[0].clone()
}

#[test]
fn reasoning_models_get_thinking_and_effort_on_the_wire() {
    let listed = |reasons| {
        Script::listed(
            vec![reply("end_turn", json!([]))],
            ModelInfo {
                price: None,
                reasons,
            },
        )
    };
    for model in [
        "anthropic/claude-opus-5.5",
        "z-ai/glm-5.3-flash",
        "qwen/qwen3.8-max-prime",
        "openai/gpt-6-sol",
    ] {
        let request = first_request(model, ThinkingMode::Auto, listed(true));
        assert_eq!(request["model"], model);
        assert_eq!(request["thinking"]["type"], "adaptive", "{model}");
        // Claude takes high effort natively; others spend a share of
        // max_tokens on it, and at high they ran out before a tool call.
        let effort = if model.starts_with("anthropic/") {
            "high"
        } else {
            "medium"
        };
        assert_eq!(request["output_config"]["effort"], effort, "{model}");
        // `require_parameters` is why a non-reasoning model must not get them.
        assert_eq!(request["provider"]["require_parameters"], true);
    }

    // Listed without `reasoning`: neither field goes on the wire.
    let request = first_request("openai/gpt-4.1", ThinkingMode::Auto, listed(false));
    assert!(request.get("thinking").is_none());
    assert!(request.get("output_config").is_none());

    // Unlisted, or the lookup failed: Anthropic still thinks, others do not.
    let unlisted = || Script::new(vec![reply("end_turn", json!([]))]);
    let request = first_request("anthropic/claude-opus-5.5", ThinkingMode::Auto, unlisted());
    assert_eq!(request["thinking"]["type"], "adaptive");
    let request = first_request("z-ai/glm-5.3-flash", ThinkingMode::Auto, unlisted());
    assert!(request.get("thinking").is_none());

    // Off wins over the listing.
    let request = first_request("openai/gpt-6-sol", ThinkingMode::Off, listed(true));
    assert!(request.get("thinking").is_none());
    assert!(request.get("output_config").is_none());
}

// ── streaming ───────────────────────────────────────────────────────────────

/// Fold `events` (as they arrive on the wire) the way the transport does.
fn fold(events: &[Value]) -> Result<Reply, TransportError> {
    let mut reply = transport::Accumulator::default();
    for event in events {
        let event = serde_json::from_value(event.clone()).expect("a stream event");
        if reply.push(event)? {
            break;
        }
    }
    reply.finish()
}

fn message_start() -> Value {
    json!({"type": "message_start", "message": {
        "id": "msg_1", "type": "message", "role": "assistant", "model": "z-ai/glm-5.3-flash",
        "content": [], "stop_reason": null,
        "usage": {"input_tokens": 100, "output_tokens": 1, "cache_read_input_tokens": 50}
    }})
}

#[test]
fn a_streamed_reply_folds_into_the_reply_a_plain_request_returns() {
    let events = [
        message_start(),
        json!({"type": "ping"}),
        json!({"type": "content_block_start", "index": 0,
               "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
        json!({"type": "content_block_delta", "index": 0,
               "delta": {"type": "thinking_delta", "thinking": "The sweep "}}),
        json!({"type": "content_block_delta", "index": 0,
               "delta": {"type": "thinking_delta", "thinking": "is redundant."}}),
        json!({"type": "content_block_delta", "index": 0,
               "delta": {"type": "signature_delta", "signature": "sig-1"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1,
               "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 1,
               "delta": {"type": "text_delta", "text": "Checking "}}),
        json!({"type": "content_block_delta", "index": 1,
               "delta": {"type": "text_delta", "text": "it."}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "content_block_start", "index": 2,
               "content_block": {"type": "tool_use", "id": "t1", "name": "preview", "input": {}}}),
        json!({"type": "content_block_delta", "index": 2,
               "delta": {"type": "input_json_delta", "partial_json": "{\"changes\": [{\"op\""}}),
        json!({"type": "content_block_delta", "index": 2,
               "delta": {"type": "input_json_delta", "partial_json": ": \"remove\"}]}"}}),
        json!({"type": "content_block_stop", "index": 2}),
        json!({"type": "message_delta",
               "delta": {"stop_reason": "tool_use", "stop_sequence": null},
               "usage": {"output_tokens": 240, "cost": 0.0012}}),
        json!({"type": "message_stop"}),
    ];
    let reply = serde_json::to_value(fold(&events).unwrap()).unwrap();
    assert_eq!(reply["stop_reason"], "tool_use");
    assert_eq!(reply["model"], "z-ai/glm-5.3-flash");
    assert_eq!(
        reply["content"],
        json!([
            {"type": "thinking", "thinking": "The sweep is redundant.", "signature": "sig-1"},
            {"type": "text", "text": "Checking it."},
            {"type": "tool_use", "id": "t1", "name": "preview",
             "input": {"changes": [{"op": "remove"}]}},
        ])
    );
    // Running totals replace; what the delta leaves out is kept.
    assert_eq!(reply["usage"]["input_tokens"], 100);
    assert_eq!(reply["usage"]["cache_read_input_tokens"], 50);
    assert_eq!(reply["usage"]["output_tokens"], 240);
    assert_eq!(reply["usage"]["cost"], 0.0012);
}

#[test]
fn a_tool_call_with_no_arguments_keeps_its_empty_input() {
    let events = [
        message_start(),
        json!({"type": "content_block_start", "index": 0,
               "content_block": {"type": "tool_use", "id": "t1", "name": "plan", "input": {}}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"},
               "usage": {"output_tokens": 5}}),
        json!({"type": "message_stop"}),
    ];
    let reply = serde_json::to_value(fold(&events).unwrap()).unwrap();
    assert_eq!(reply["content"][0]["input"], json!({}));
}

#[test]
fn a_cut_off_stream_is_retried_and_a_stream_error_keeps_its_status() {
    let cut = fold(&[
        message_start(),
        json!({"type": "content_block_start", "index": 0,
               "content_block": {"type": "text", "text": ""}}),
    ])
    .unwrap_err();
    assert!(matches!(cut, TransportError::Network(_)), "{cut:?}");
    assert!(cut.retryable());

    let overloaded = fold(&[
        message_start(),
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy"}}),
    ])
    .unwrap_err();
    assert!(
        matches!(overloaded, TransportError::Status { status: 529, .. }),
        "{overloaded:?}"
    );
    assert!(overloaded.retryable());

    let invalid = fold(&[
        message_start(),
        json!({"type": "error", "error": {"type": "invalid_request_error", "message": "no"}}),
    ])
    .unwrap_err();
    assert!(!invalid.retryable(), "{invalid:?}");

    let torn = fold(&[
        message_start(),
        json!({"type": "content_block_start", "index": 0,
               "content_block": {"type": "tool_use", "id": "t1", "name": "plan", "input": {}}}),
        json!({"type": "content_block_delta", "index": 0,
               "delta": {"type": "input_json_delta", "partial_json": "{\"a\":"}}),
        json!({"type": "content_block_stop", "index": 0}),
    ])
    .unwrap_err();
    assert!(matches!(torn, TransportError::Decode(_)), "{torn:?}");
}

/// A local `/messages` that answers every request with `frames`, each sent
/// after its delay, and then holds the connection open for `hang`. Returns
/// the API root to point a transport at.
async fn sse_server(frames: Vec<(Duration, Value)>, hang: Duration) -> String {
    use axum::body::{Body, Bytes};
    use futures_util::StreamExt;

    let app = axum::Router::new().route(
        "/api/v1/messages",
        axum::routing::post(move || {
            let frames = frames.clone();
            async move {
                let body = futures_util::stream::iter(frames)
                    .then(|(delay, event)| async move {
                        tokio::time::sleep(delay).await;
                        let kind = event["type"].as_str().unwrap_or_default().to_owned();
                        Ok::<_, std::io::Error>(Bytes::from(format!(
                            "event: {kind}\ndata: {event}\n\n"
                        )))
                    })
                    .chain(futures_util::stream::once(async move {
                        tokio::time::sleep(hang).await;
                        Ok(Bytes::new())
                    }));
                axum::response::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(body))
                    .unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/api/v1")
}

fn streaming_transport(base_url: &str, idle: Duration) -> OpenRouterTransport {
    OpenRouterTransport::new(&OpenRouterSettings {
        base_url,
        api_key: KEY,
        app_title: "finplan tests",
        referer: None,
        timeout: Duration::from_secs(30),
        idle_timeout: idle,
    })
    .unwrap()
}

fn tiny_request() -> Request {
    let mut builder = Request::builder();
    builder
        .model("z-ai/glm-5.3-flash")
        .max_tokens(64u32)
        .messages(vec![AnthropicMessage::user("hello")]);
    builder.build().unwrap()
}

#[tokio::test]
async fn the_transport_streams_a_slow_reply_to_the_end() {
    // Each gap is under the idle timeout; together they are well over it, as
    // a long answer from a slow model would be.
    let gap = Duration::from_millis(120);
    let frames = vec![
        (Duration::ZERO, message_start()),
        (
            gap,
            json!({"type": "content_block_start", "index": 0,
                     "content_block": {"type": "text", "text": ""}}),
        ),
        (
            gap,
            json!({"type": "content_block_delta", "index": 0,
                     "delta": {"type": "text_delta", "text": "Slow "}}),
        ),
        (
            gap,
            json!({"type": "content_block_delta", "index": 0,
                     "delta": {"type": "text_delta", "text": "but alive."}}),
        ),
        (gap, json!({"type": "content_block_stop", "index": 0})),
        (
            gap,
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                     "usage": {"output_tokens": 4}}),
        ),
        (gap, json!({"type": "message_stop"})),
    ];
    let base = sse_server(frames, Duration::from_secs(5)).await;
    let transport = streaming_transport(&base, Duration::from_millis(400));
    let reply = transport.send(&tiny_request()).await.unwrap();
    let reply = serde_json::to_value(reply).unwrap();
    assert_eq!(reply["stop_reason"], "end_turn");
    assert_eq!(
        reply["content"],
        json!([{"type": "text", "text": "Slow but alive."}])
    );
    assert_eq!(reply["usage"]["input_tokens"], 100);
}

#[tokio::test]
async fn a_stream_that_goes_quiet_trips_the_idle_timeout() {
    let frames = vec![
        (Duration::ZERO, message_start()),
        (
            Duration::ZERO,
            json!({"type": "content_block_start", "index": 0,
                                "content_block": {"type": "text", "text": ""}}),
        ),
    ];
    let base = sse_server(frames, Duration::from_secs(30)).await;
    let transport = streaming_transport(&base, Duration::from_millis(300));
    let started = Instant::now();
    let error = transport.send(&tiny_request()).await.unwrap_err();
    assert!(matches!(error, TransportError::Network(_)), "{error:?}");
    assert!(error.retryable());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "abandoned on silence, not at the request timeout"
    );
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
    /// The progress hooks, as one line each.
    activity: Mutex<Vec<String>>,
}

impl Recorder {
    fn log(&self, line: String) {
        self.activity.lock().unwrap().push(line);
    }
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
    fn thinking(&self) {
        self.log("thinking".into());
    }
    fn tool_started(&self, name: &str) {
        self.log(format!("start {name}"));
    }
    fn tool_finished(&self, name: &str, failed: bool) {
        self.log(format!(
            "finish {name}{}",
            if failed { " failed" } else { "" }
        ));
    }
    fn narration(&self, text: &str) {
        self.log(format!("said {text}"));
    }
}

#[tokio::test]
async fn the_observer_hears_progress_as_it_happens() {
    let g = graph();
    let changes = remove_sweep(&g);
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                {"type": "text", "text": "Let me preview removing the sweep."},
                call("t1", "preview_changes", json!({"changes": changes})),
                call("t2", "no_such_tool", json!({})),
            ]),
        ),
        reply("end_turn", json!([{"type": "text", "text": "The answer."}])),
    ]);
    let client = AiClient::new(settings(), script, None);
    let recorder = Recorder::default();
    generate_observed(&client, &context(&[]), &Tools::new(), &recorder)
        .await
        .unwrap();
    assert_eq!(
        *recorder.activity.lock().unwrap(),
        vec![
            "thinking",
            "said Let me preview removing the sweep.",
            "start preview_changes",
            "finish preview_changes",
            "start no_such_tool",
            "finish no_such_tool failed",
            // The answer itself is the turn's reply, not narration.
            "thinking",
        ]
    );
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
                           "summary": "The one-line lead.", "reasoning": "r", "paths": [path_of(changes.clone())]})
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
        "at least +1 pts on success or funding success, +5% on the real median or the after-tax ending balance, 5% less lifetime tax, or +10% on the real P10",
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
        "summary": "The one-line lead.", "reasoning": "All lots were bought on the start date at that day's price, so the plan assumes no built-in gains.",
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

    // A Roth conversion: success and the real median stay put, while the
    // after-tax ending balance rises and lifetime tax falls; either clears.
    let stats = |after_tax: f64, tax: f64| {
        json!({"success_rate": 0.95, "funding_success_rate": 0.95,
               "real_final": {"p10": 1.0e6, "p50": 2.0e6},
               "after_tax_final": after_tax, "lifetime_taxes": tax})
    };
    let converted = Deltas::of(&json!({
        "base": stats(4.0e6, 1.0e6), "edited": stats(4.3e6, 1.0e6)
    }));
    assert!(converted.clears(&Materiality::default()));
    assert!(
        converted
            .describe()
            .contains("after-tax ending balance +7.5%"),
        "{}",
        converted.describe()
    );
    let saved = Deltas::of(&json!({
        "base": stats(4.0e6, 1.0e6), "edited": stats(4.0e6, 0.8e6)
    }));
    assert!(saved.clears(&Materiality::default()));
    assert!(saved.describe().contains("lifetime tax -20.0%"));
    // More tax is never material.
    let dearer = Deltas::of(&json!({
        "base": stats(4.0e6, 1.0e6), "edited": stats(4.0e6, 1.3e6)
    }));
    assert!(!dearer.clears(&Materiality::default()));

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
    let tools = prompt::tools(&tools::Registry::all());
    let submit = &tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "submit_suggestion")
        .unwrap()["input_schema"];
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

// ── routing privacy ─────────────────────────────────────────────────────────

/// A drafting client (documents in its context) requires zero-data-retention
/// providers on top of the review rules; the review client's routing is
/// exactly what it was.
#[test]
fn a_drafting_client_requires_zero_data_retention_and_review_does_not() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let wire = |client: &AiClient| {
        let request = runtime
            .block_on(client.request(&[AnthropicMessage::user("hello")]))
            .expect("request builds");
        serde_json::to_value(&request).unwrap()
    };
    let review = AiClient::new(settings(), Script::new(vec![]), None);
    let provider = &wire(&review)["provider"];
    assert_eq!(provider["require_parameters"], true);
    assert_eq!(provider["data_collection"], "deny");
    assert!(
        provider.get("zdr").is_none(),
        "review routing is unchanged: {provider}"
    );
    assert!(!review.zero_data_retention());

    let draft = AiClient::new(settings(), Script::new(vec![]), None).with_zero_data_retention(true);
    assert!(draft.zero_data_retention());
    let provider = &wire(&draft)["provider"];
    assert_eq!(provider["zdr"], true);
    assert_eq!(provider["require_parameters"], true);
    assert_eq!(provider["data_collection"], "deny");
}

// ── shared tools through the loop ───────────────────────────────────────────

/// The result the request at `n` carried for the call `id`, and whether it
/// was flagged an error.
fn result_for(requests: &[Value], n: usize, id: &str) -> (String, bool) {
    let messages = requests[n]["messages"].as_array().unwrap();
    let parts = messages.last().unwrap()["content"].as_array().unwrap();
    let part = parts
        .iter()
        .find(|p| p["tool_use_id"] == id)
        .unwrap_or_else(|| panic!("no result for {id}"));
    (
        part["content"].as_str().unwrap().to_owned(),
        part["is_error"] == true,
    )
}

fn json_result(requests: &[Value], n: usize, id: &str) -> Value {
    serde_json::from_str(&result_for(requests, n, id).0).expect("a JSON result")
}

async fn run_script(
    script: Arc<Script>,
    settings: Settings,
    context: &ReviewContext,
    tools: &Tools,
) -> AiOutcome {
    let client = AiClient::new(settings, script, None);
    generate(&client, context, tools).await.unwrap()
}

#[tokio::test]
async fn validate_changes_is_free_and_reports_diffs_or_problems() {
    let g = graph();
    let good = remove_sweep(&g);
    let mut stale = good.clone();
    stale[0]["expect"] = json!({"kind": "Nothing"});
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                call("v1", "validate_changes", json!({"steps": [good]})),
                call("v2", "validate_changes", json!({"steps": [stale]})),
                call("v3", "validate_changes", json!({"steps": []})),
                call(
                    "v4",
                    "validate_changes",
                    json!({"steps": [{"key": "a", "title": "Remove the sweep", "changes": good}]}),
                ),
                call(
                    "v5",
                    "validate_changes",
                    json!({"steps": [[{"op": "replace", "target": {"events": 1}, "path": ""}]]}),
                ),
            ]),
        ),
        reply("end_turn", json!([])),
    ]);
    let tools = Tools::new();
    let outcome = run_script(script.clone(), settings(), &context(&[]), &tools).await;
    let requests = script.requests();

    let ok = json_result(&requests, 1, "v1");
    assert_eq!(ok["valid"], true);
    assert!(!ok["steps"][0]["diff"].as_array().unwrap().is_empty());
    let bad = json_result(&requests, 1, "v2");
    assert_eq!(bad["valid"], false);
    assert_eq!(bad["failed_step"], 0);
    assert_eq!(bad["problems"][0]["kind"], "stale");
    // A dry run is informational, not an error; an unusable call is.
    assert!(!result_for(&requests, 1, "v2").1);
    assert!(result_for(&requests, 1, "v3").1);
    // A note's step object reads as the step it holds.
    assert_eq!(json_result(&requests, 1, "v4")["valid"], true);
    // A change that does not parse is named by its place.
    let (text, is_error) = result_for(&requests, 1, "v5");
    assert!(is_error);
    assert!(text.starts_with("step 1, change 1:"), "{text}");
    assert_eq!(outcome.usage.previews, 0, "validation spends no preview");
    assert_eq!(*tools.previews.lock().unwrap(), 0);
}

#[tokio::test]
async fn preview_paths_previews_each_path_on_the_budget_and_counts_for_submission() {
    let g = graph();
    let remove = remove_sweep(&g);
    let halve = halve_sweep(&g);
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call(
                "p1",
                "preview_paths",
                json!({"paths": [
                    {"key": "remove", "steps": [remove.clone()]},
                    {"key": "halve", "steps": [halve.clone()]}
                ]})
            )]),
        ),
        reply(
            "tool_use",
            json!([call("s1", "submit_suggestion", {
                let mut note = good_note(remove.clone());
                note["paths"] = json!([
                    {"key": "remove", "label": "Remove the sweep", "recommended": true,
                     "steps": [{"key": "a", "title": "Remove the sweep", "changes": remove}]},
                    {"key": "halve", "label": "Halve the sweep", "recommended": false,
                     "steps": [{"key": "a", "title": "Halve the sweep", "changes": halve}]}
                ]);
                note
            })]),
        ),
        reply("end_turn", json!([])),
    ]);
    let tools = Tools::new();
    let outcome = run_script(script.clone(), settings(), &context(&[]), &tools).await;
    let requests = script.requests();

    let result = json_result(&requests, 1, "p1");
    assert_eq!(result["paths"][0]["path"], "remove");
    assert_eq!(result["paths"][1]["path"], "halve");
    assert_eq!(result["paths"][1]["preview"]["paired"], true);
    assert_eq!(outcome.usage.previews, 2, "one preview per path");
    assert_eq!(*tools.previews.lock().unwrap(), 2);
    // Both paths were previewed by the call, so the note needs no preview_changes.
    assert_eq!(outcome.drafts.len(), 1, "{:?}", outcome.summary);
    assert!(outcome.drafts[0].paths.iter().all(|p| p.previewed));
}

#[tokio::test]
async fn preview_paths_is_refused_when_it_would_overspend_or_is_malformed() {
    let g = graph();
    let remove = remove_sweep(&g);
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                call(
                    "p1",
                    "preview_paths",
                    json!({"paths": [{"steps": [remove.clone()]}, {"steps": [remove.clone()]}]})
                ),
                call(
                    "p2",
                    "preview_paths",
                    json!({"paths": [{"steps": [remove.clone()]}]})
                ),
                call("p3", "preview_changes", json!({"steps": [remove.clone()]})),
                call("p4", "preview_changes", json!({"steps": [remove.clone()]})),
            ]),
        ),
        reply("end_turn", json!([])),
    ]);
    let tools = Tools::new();
    let settings = Settings {
        max_previews: 1,
        ..settings()
    };
    let outcome = run_script(script.clone(), settings, &context(&[]), &tools).await;
    let requests = script.requests();

    let (over, is_error) = result_for(&requests, 1, "p1");
    assert!(
        is_error && over.contains("need 2 previews and 1 are left"),
        "{over}"
    );
    let (few, is_error) = result_for(&requests, 1, "p2");
    assert!(is_error && few.contains("2 to 4 paths"), "{few}");
    // The refusals spent nothing: the one preview left served the next call.
    assert!(!result_for(&requests, 1, "p3").1);
    let (spent, is_error) = result_for(&requests, 1, "p4");
    assert!(is_error && spent.contains("budget"), "{spent}");
    assert_eq!(outcome.usage.previews, 1);
}

#[tokio::test]
async fn preflight_and_calculators_answer_through_the_loop_and_can_be_cited() {
    let g = graph();
    let changes = remove_sweep(&g);
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                call("f1", "preflight", json!({})),
                call(
                    "f2",
                    "reference_facts",
                    json!({"topic": "401k", "year": 2025})
                ),
                call(
                    "f3",
                    "finance_calc",
                    json!({"op": "pmt", "principal": 400000, "annual_rate": 0.06, "years": 30})
                ),
                call(
                    "f4",
                    "estimate_social_security",
                    json!({"birth_year": 1965, "claim_age": 67, "current_salary": 100000})
                ),
                call(
                    "f5",
                    "estimate_taxes",
                    json!({"income": 100000, "year": 2025, "filing_status": "single"})
                ),
                call(
                    "f6",
                    "reference_facts",
                    json!({"topic": "401k", "year": 2031})
                ),
                call("f7", "preview_changes", json!({"changes": changes})),
            ]),
        ),
        reply("end_turn", json!([])),
    ]);
    let tools = Tools::new();
    let outcome = run_script(script.clone(), settings(), &context(&[]), &tools).await;
    let requests = script.requests();

    assert_eq!(json_result(&requests, 1, "f1")["can_run"], true);
    assert_eq!(
        json_result(&requests, 1, "f2")["facts"]["401k"]["employee_deferral"],
        23_500.0
    );
    assert_eq!(
        json_result(&requests, 1, "f3")["payment_per_period"],
        2_398.2
    );
    let ss = json_result(&requests, 1, "f4");
    assert_eq!(
        (ss["aime"].clone(), ss["monthly_benefit"].clone()),
        (json!(8_333.0), json!(3_313.0))
    );
    let tax = json_result(&requests, 1, "f5");
    assert_eq!(tax["federal_income_tax"], 13_449.0);
    // The double's plan settings are supplied to the tax tool.
    assert!(tax["plan_model"]["total_tax"].is_number());
    let (refused, is_error) = result_for(&requests, 1, "f6");
    assert!(is_error && refused.contains("outside"), "{refused}");
    assert_eq!(outcome.usage.previews, 1);

    // Successful calls can be cited; failed ones and made-up ids cannot.
    let ctx = context(&[]);
    let client = AiClient::new(settings(), Script::new(Vec::new()), None);
    let tools = Tools::new();
    let mut session = Session {
        client: &client,
        context: &ctx,
        tools: &tools,
        observer: &NoObserver,
        drafts: Vec::new(),
        usage: Usage::default(),
        previewed: HashMap::new(),
        computed: HashMap::new(),
    };
    let (_, is_error) = session
        .run_tool(
            "c1",
            "reference_facts",
            &json!({"topic": "hsa", "year": 2026}),
        )
        .await;
    assert!(!is_error);
    let (_, is_error) = session
        .run_tool(
            "c2",
            "reference_facts",
            &json!({"topic": "hsa", "year": 1999}),
        )
        .await;
    assert!(is_error);
    let cite = |tool: &str, call_id: &str| {
        session.check_evidence(
            &serde_json::from_value(json!({"ref": "computed", "tool": tool, "call_id": call_id}))
                .unwrap(),
        )
    };
    assert!(cite("reference_facts", "c1").is_ok());
    assert!(
        cite("finance_calc", "c1")
            .unwrap_err()
            .contains("not finance_calc")
    );
    assert!(
        cite("reference_facts", "c2")
            .unwrap_err()
            .contains("no successful call")
    );
    assert!(cite("reference_facts", "nope").is_err());
}

#[tokio::test]
async fn a_note_citing_a_tool_result_is_accepted_and_a_forged_call_is_not() {
    let g = graph();
    let changes = remove_sweep(&g);
    let cited = |call_id: &str| {
        let mut note = good_note(changes.clone());
        note["evidence"] =
            json!([{"ref": "computed", "tool": "reference_facts", "call_id": call_id}]);
        note
    };
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                call(
                    "f1",
                    "reference_facts",
                    json!({"topic": "401k", "year": 2026})
                ),
                call("f2", "preview_changes", json!({"changes": changes.clone()})),
            ]),
        ),
        reply(
            "tool_use",
            json!([call("s1", "submit_suggestion", cited("forged"))]),
        ),
        reply(
            "tool_use",
            json!([call("s2", "submit_suggestion", cited("f1"))]),
        ),
        reply("end_turn", json!([])),
    ]);
    let outcome = run_script(script.clone(), settings(), &context(&[]), &Tools::new()).await;
    let requests = script.requests();
    let (rejected, is_error) = result_for(&requests, 2, "s1");
    assert!(
        is_error && rejected.contains("no successful call forged"),
        "{rejected}"
    );
    assert_eq!(outcome.drafts.len(), 1);
    assert_eq!(
        outcome.drafts[0].evidence,
        vec![Evidence::Computed {
            tool: "reference_facts".into(),
            call_id: "f1".into()
        }]
    );
}

#[tokio::test]
async fn goal_seek_costs_previews_is_capped_and_its_answer_is_citable() {
    let g = graph();
    let changes = remove_sweep(&g);
    let mut note = good_note(changes.clone());
    note["evidence"] = json!([{"ref": "computed", "tool": "goal_seek", "call_id": "g1"}]);
    let seek =
        |parameter: &str| json!({"parameter": parameter, "metric": "success_rate", "target": 0.9});
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                call("g1", "goal_seek", seek("Retirement age")),
                // A malformed call is not a search and costs nothing.
                call(
                    "bad",
                    "goal_seek",
                    json!({"parameter": "x", "metric": "success_rate", "target": 1.5})
                ),
                call("g2", "goal_seek", seek("Spending")),
                call("g3", "goal_seek", seek("Spending")),
                call("f", "preview_changes", json!({"changes": changes.clone()})),
            ]),
        ),
        reply("tool_use", json!([call("s1", "submit_suggestion", note)])),
        reply("end_turn", json!([])),
    ]);
    let mut settings = settings();
    settings.max_previews = 12;
    let tools = Tools::new();
    let outcome = run_script(script.clone(), settings, &context(&[]), &tools).await;
    let requests = script.requests();

    let found = json_result(&requests, 1, "g1");
    assert_eq!(found["result"]["value_text"], "age 43");
    let (invalid, is_error) = result_for(&requests, 1, "bad");
    assert!(is_error && invalid.contains("fraction"), "{invalid}");
    assert!(!result_for(&requests, 1, "g2").1);
    let (capped, is_error) = result_for(&requests, 1, "g3");
    assert!(is_error && capped.contains("2 times"), "{capped}");
    assert_eq!(
        *tools.goal_seeks.lock().unwrap(),
        [
            "Retirement age/success_rate/0.9",
            "Spending/success_rate/0.9"
        ],
        "the host is asked only for the two allowed searches"
    );
    // Two searches at four previews each, and the preview after them.
    assert_eq!(
        outcome.usage.previews,
        2 * tools::goal_seek::PREVIEW_COST + 1
    );
    assert_eq!(outcome.usage.goal_seeks, 2);
    assert_eq!(
        outcome.drafts[0].evidence,
        vec![Evidence::Computed {
            tool: "goal_seek".into(),
            call_id: "g1".into()
        }]
    );
}

#[tokio::test]
async fn sensitivity_costs_previews_once_and_a_refusal_costs_nothing() {
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                // Refused before simulating: free, and the one use is left.
                call("r", "sensitivity", json!({"parameters": ["nothing"]})),
                call("bad", "sensitivity", json!({"fraction": 3})),
                call(
                    "s1",
                    "sensitivity",
                    json!({"parameters": ["Spending", "Retirement age"]})
                ),
                call("s2", "sensitivity", json!({})),
                call(
                    "b",
                    "cash_flow_breakdown",
                    json!({"rank": "p10", "years": [2040, 2050]})
                ),
                call("b2", "cash_flow_breakdown", json!({"rank": "p99"})),
            ]),
        ),
        reply("end_turn", json!([])),
    ]);
    let mut settings = settings();
    settings.max_previews = 12;
    let tools = Tools::new();
    let outcome = run_script(script.clone(), settings, &context(&[]), &tools).await;
    let requests = script.requests();

    let (refused, is_error) = result_for(&requests, 1, "r");
    assert!(is_error && refused.contains("no parameter"), "{refused}");
    let (invalid, is_error) = result_for(&requests, 1, "bad");
    assert!(is_error && invalid.contains("fraction"), "{invalid}");
    let ranked = json_result(&requests, 1, "s1");
    assert_eq!(ranked["metric"], "funding_success_rate");
    assert_eq!(ranked["ranking"][0]["span_points"], 12.5);
    let (capped, is_error) = result_for(&requests, 1, "s2");
    assert!(is_error && capped.contains("1 time"), "{capped}");
    assert_eq!(
        *tools.sensitivities.lock().unwrap(),
        [vec!["Spending".to_owned(), "Retirement age".to_owned()]]
    );
    assert_eq!(
        (outcome.usage.previews, outcome.usage.sensitivities),
        (tools::sensitivity::PREVIEW_COST, 1)
    );

    assert_eq!(
        result_for(&requests, 1, "b").0,
        "p10 breakdown, years Some((2040, 2050))"
    );
    assert!(result_for(&requests, 1, "b2").1);
}

#[tokio::test]
async fn a_failed_sensitivity_is_charged_and_one_without_previews_is_refused() {
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call("f", "sensitivity", json!({"parameters": ["boom"]}))]),
        ),
        reply("end_turn", json!([])),
    ]);
    let tools = Tools::new();
    let outcome = run_script(script.clone(), settings(), &context(&[]), &tools).await;
    assert!(result_for(&script.requests(), 1, "f").1);
    assert_eq!(
        (outcome.usage.previews, outcome.usage.sensitivities),
        (tools::sensitivity::PREVIEW_COST, 1)
    );

    let script = Script::new(vec![
        reply("tool_use", json!([call("p", "sensitivity", json!({}))])),
        reply("end_turn", json!([])),
    ]);
    let mut settings = settings();
    settings.max_previews = tools::sensitivity::PREVIEW_COST - 1;
    let tools = Tools::new();
    let outcome = run_script(script.clone(), settings, &context(&[]), &tools).await;
    let (text, is_error) = result_for(&script.requests(), 1, "p");
    assert!(is_error && text.contains("costs 4 previews"), "{text}");
    assert!(tools.sensitivities.lock().unwrap().is_empty());
    assert_eq!(
        (outcome.usage.previews, outcome.usage.sensitivities),
        (0, 0)
    );
}

#[tokio::test]
async fn goal_seek_is_refused_without_the_previews_to_pay_for_it() {
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call(
                "g",
                "goal_seek",
                json!({"parameter": "Retirement age", "metric": "funding_success_rate", "target": 0.9})
            )]),
        ),
        reply("end_turn", json!([])),
    ]);
    let mut settings = settings();
    settings.max_previews = tools::goal_seek::PREVIEW_COST - 1;
    let tools = Tools::new();
    let outcome = run_script(script.clone(), settings, &context(&[]), &tools).await;
    let (text, is_error) = result_for(&script.requests(), 1, "g");
    assert!(is_error && text.contains("costs 4 previews"), "{text}");
    assert!(tools.goal_seeks.lock().unwrap().is_empty());
    assert_eq!((outcome.usage.previews, outcome.usage.goal_seeks), (0, 0));
}

#[tokio::test]
async fn a_failed_goal_seek_still_counts_and_is_not_citable() {
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([call(
                "g",
                "goal_seek",
                json!({"parameter": "nothing", "metric": "success_rate", "target": 0.9})
            )]),
        ),
        reply("end_turn", json!([])),
    ]);
    let outcome = run_script(script.clone(), settings(), &context(&[]), &Tools::new()).await;
    let (text, is_error) = result_for(&script.requests(), 1, "g");
    assert!(is_error && text.contains("no parameter"), "{text}");
    assert_eq!(outcome.usage.goal_seeks, 1);
    assert_eq!(outcome.usage.previews, tools::goal_seek::PREVIEW_COST);
}

#[tokio::test]
async fn failure_profile_and_inspect_path_ground_risk_notes_in_the_run() {
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                call("i1", "failure_profile", json!({})),
                call(
                    "i2",
                    "inspect_path",
                    json!({"rank": "median", "years": [2060, 2070]})
                ),
                call("i3", "inspect_path", json!({"rank": "worst"})),
                call("i4", "inspect_path", json!({"rank": "typical"})),
                call(
                    "i5",
                    "inspect_path",
                    json!({"rank": "median", "years": [2070, 2060]})
                ),
            ]),
        ),
        reply("end_turn", json!([])),
    ]);
    run_script(script.clone(), settings(), &context(&[]), &Tools::new()).await;
    let requests = script.requests();

    let profile = json_result(&requests, 1, "i1");
    assert_eq!(profile["failed"], 189);
    assert_eq!(profile["first_shortfall_years"][0]["year"], 2074);
    assert_eq!(
        profile["first_shortfall_years"][0]["share_of_failed"],
        0.9524
    );
    assert_eq!(profile["shortfall_accounts"][0]["account_id"], 6);
    assert!(
        profile["shortfall_accounts"][0]["account"]
            .as_str()
            .unwrap()
            .len()
            > 1
    );
    assert!(
        profile["note"]
            .as_str()
            .unwrap()
            .contains("percentile paths")
    );
    assert_eq!(
        result_for(&requests, 1, "i2").0,
        "median path, years Some((2060, 2070))"
    );
    // A host with no stored path says so.
    let (worst, is_error) = result_for(&requests, 1, "i3");
    assert!(is_error && worst.contains("no percentile paths"));
    assert!(result_for(&requests, 1, "i4").1);
    assert!(result_for(&requests, 1, "i5").1);
}

#[test]
fn a_stored_path_renders_with_its_percentile_and_thins_long_runs() {
    let g = graph();
    let r = results();
    let all = render_path(&g, &r, "median", None);
    assert!(all.contains("stored P50 path"), "{all}");
    assert!(all.contains("year,age,income"));
    // 70 years thin to at most 45 rows plus the last.
    let flow_rows = |text: &str| {
        text.lines()
            .filter(|l| {
                l.split(',')
                    .next()
                    .is_some_and(|y| y.len() == 4 && y.starts_with("20"))
            })
            .count()
    };
    let rows = flow_rows(&all);
    assert!(rows <= 46 && rows > 20, "{rows}");
    let narrow = render_path(&g, &r, "median", Some((2030, 2032)));
    assert_eq!(flow_rows(&narrow), 3);
    assert!(narrow.contains("2032-12-31,"));
    assert!(!narrow.contains("2040-12-31"));
}

#[test]
fn the_run_and_a_stored_path_show_lifetime_tax_and_early_withdrawal_penalties() {
    let g = graph();
    let mut r = results();
    // None on the hand-built path.
    assert!(
        context(&[])
            .text
            .contains("Early-withdrawal penalties: none.")
    );
    // $9k a year of tax; penalties in 2037-2039, at 41-43 (born 1996).
    for c in &mut r.cash_flows {
        c.taxes = 9_000.0;
        if (2037..=2039).contains(&c.year) {
            c.early_withdrawal_penalties = 3_000.0 * (c.year - 2036) as f64;
        }
    }
    let run = ReviewContext::build(&g, &r, &[]).text;
    assert!(
        run.contains(
            "Lifetime tax on the P50 path by final nominal net worth (nominal, penalties \
             included): $630,000 (9.00% of lifetime spending). Early-withdrawal penalties: \
             $18,000 (0.26% of lifetime spending) in 3 years: 2037 (age 41) $3,000, 2038 \
             (age 42) $6,000, 2039 (age 43) $9,000."
        ),
        "{run}"
    );
    let path = render_path(&g, &r, "median", Some((2030, 2032)));
    assert!(
        path.contains("Lifetime tax on this path (nominal, penalties included): $630,000"),
        "{path}"
    );
    assert!(path.contains("in 3 years: 2037 (age 41) $3,000"), "{path}");
}

#[test]
fn the_plan_shows_its_funding_policy() {
    let mut g = graph();
    assert!(
        ReviewContext::build(&g, &results(), &[])
            .text
            .contains("Funding policy: none;")
    );
    g.scenario.funding_strategy = Some("PenaltyAware".into());
    assert!(
        ReviewContext::build(&g, &results(), &[])
            .text
            .contains("Funding policy: PenaltyAware: a bank that runs short")
    );
}

#[test]
fn document_answer_and_description_evidence_is_checked_against_its_source() {
    let ctx = context(&[])
        .with_documents([
            (
                7,
                Some("USAA Savings\nBalance:   $6,000.00\u{c}Page two\nno deposits"),
            ),
            (8, None),
        ])
        .with_answers(["bonus"])
        .with_description("I earn $205,000 a year and plan to retire at 45.");
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
        computed: HashMap::new(),
    };
    let check = |e: Value| session.check_evidence(&serde_json::from_value(e).unwrap());
    // Whitespace is normalized, pages are 1-based.
    assert!(
        check(
            json!({"ref": "document", "document_id": 7, "page": 1, "excerpt": "Balance: $6,000.00"})
        )
        .is_ok()
    );
    assert!(
        check(json!({"ref": "document", "document_id": 7, "page": 2, "excerpt": "no deposits"}))
            .is_ok()
    );
    assert!(
        check(
            json!({"ref": "document", "document_id": 7, "page": 2, "excerpt": "Balance: $6,000.00"})
        )
        .unwrap_err()
        .contains("does not appear")
    );
    assert!(
        check(json!({"ref": "document", "document_id": 7, "page": 3, "excerpt": "x"}))
            .unwrap_err()
            .contains("outside")
    );
    assert!(
        check(json!({"ref": "document", "document_id": 99, "page": 1, "excerpt": "x"})).is_err()
    );
    assert!(check(json!({"ref": "document", "document_id": 7, "page": 1, "excerpt": ""})).is_err());
    // No text layer: nothing to check against, so the citation stands.
    assert!(
        check(json!({"ref": "document", "document_id": 8, "page": 1, "excerpt": "Balance $1"}))
            .is_ok()
    );
    assert!(check(json!({"ref": "answer", "question_key": "bonus"})).is_ok());
    assert!(check(json!({"ref": "answer", "question_key": "filing"})).is_err());
    assert!(check(json!({"ref": "description", "excerpt": "retire at   45"})).is_ok());
    assert!(check(json!({"ref": "description", "excerpt": "retire at 50"})).is_err());
    // A review has none of these.
    let bare = context(&[]);
    let session = Session {
        context: &bare,
        ..session
    };
    let check = |e: Value| session.check_evidence(&serde_json::from_value(e).unwrap());
    assert!(
        check(json!({"ref": "description", "excerpt": "x"}))
            .unwrap_err()
            .contains("no user description")
    );
    assert!(check(json!({"ref": "answer", "question_key": "bonus"})).is_err());
}

#[tokio::test]
async fn the_review_expands_its_templates_against_the_plan() {
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                call(
                    "a",
                    "expand_template",
                    json!({"kind": "reinvest_cash", "params": {"from_account_id": 6,
                           "annual_spending": 100000}})
                ),
                call(
                    "b",
                    "expand_template",
                    json!({"kind": "salary", "params": {"to_account_id": 6,
                           "annual_amount": 1000}})
                ),
                call(
                    "c",
                    "expand_template",
                    json!({"kind": "reinvest_cash", "params": {"from_account_id": 3,
                           "to_account_id": 1}})
                ),
            ]),
        ),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script.clone(), None);
    generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    let requests = script.requests();
    // USAA's cash above two years of $100k moves into Vanguard, the largest
    // taxable account, which buys its holdings.
    let expansion = json_result(&requests, 1, "a");
    let event = &expansion["changes"][0]["value"];
    assert_eq!(event["effects"][0]["kind"], "CashTransfer");
    assert_eq!(event["effects"][0]["to_account_id"], 1);
    assert_eq!(
        event["effects"][0]["amount"]["source"],
        "max(0, cash(source) - inflation(200000))"
    );
    // Only the review's templates, and the plan's rules for them.
    let (refused, is_error) = result_for(&requests, 1, "b");
    assert!(
        is_error && refused.contains("only reinvest_cash"),
        "{refused}"
    );
    let (refused, is_error) = result_for(&requests, 1, "c");
    assert!(is_error && refused.contains("in place"), "{refused}");
    // Its params are in the cached reference.
    assert!(prompt::reference().contains("ReinvestCashParams"));
}

#[tokio::test]
async fn a_loop_can_serve_a_subset_of_the_registry() {
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                call(
                    "a",
                    "reference_facts",
                    json!({"topic": "ira", "year": 2026})
                ),
                call("b", "preview_changes", json!({"changes": []})),
            ]),
        ),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script.clone(), None)
        .with_registry(tools::Registry::only(&["reference_facts", "finance_calc"]));
    generate(&client, &context(&[]), &Tools::new())
        .await
        .unwrap();
    let requests = script.requests();
    let names: Vec<&str> = requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["reference_facts", "finance_calc", "submit_suggestion"]
    );
    assert!(!result_for(&requests, 1, "a").1);
    let (unknown, is_error) = result_for(&requests, 1, "b");
    assert!(is_error && unknown.contains("unknown tool"));

    assert_eq!(
        tools::Registry::group(tools::Group::Calculators)
            .names()
            .len(),
        4
    );
    assert!(tools::Registry::all().contains("validate_changes"));
    assert!(
        !tools::Registry::all()
            .without(&["preflight"])
            .contains("preflight")
    );
}

#[tokio::test]
async fn every_tool_reports_under_its_own_metric_label() {
    #[derive(Default)]
    struct Seen(Mutex<Vec<(AiTool, AiToolOutcome)>>);
    impl Observer for Seen {
        fn tool(&self, tool: AiTool, outcome: AiToolOutcome, _seconds: f64) {
            self.0.lock().unwrap().push((tool, outcome));
        }
    }
    let script = Script::new(vec![
        reply(
            "tool_use",
            json!([
                call("1", "preflight", json!({})),
                call("2", "finance_calc", json!({"op": "wat"})),
                call("3", "estimate_taxes", json!({"income": 1000})),
                call("4", "nonsense", json!({})),
            ]),
        ),
        reply("end_turn", json!([])),
    ]);
    let client = AiClient::new(settings(), script, None);
    let seen = Seen::default();
    generate_observed(&client, &context(&[]), &Tools::new(), &seen)
        .await
        .unwrap();
    assert_eq!(
        *seen.0.lock().unwrap(),
        vec![
            (AiTool::Preflight, AiToolOutcome::Ok),
            (AiTool::FinanceCalc, AiToolOutcome::Invalid),
            (AiTool::Taxes, AiToolOutcome::Ok),
            (AiTool::Unknown, AiToolOutcome::Invalid),
        ]
    );
}

// ── the drafting agent ──────────────────────────────────────────────────────

mod drafting {
    use std::collections::HashMap;
    use std::ops::RangeInclusive;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

    use super::*;
    use crate::api::suggestions::DraftColumn;
    use crate::documents::store::DocumentPage;
    use crate::suggest::ai::draft::{
        self, AddedNote, AnswerType, DocumentRead, DocumentTool, DraftHost, DraftInput,
        DraftQuestion, DraftStop, LibraryProfile, NewNote, NoteSummary, QuestionOption, Resume,
        Transcript, validate_answer,
    };

    const STATEMENT: &str = "Acme Bank statement\nAccount ending 1234\nBalance: $12,345.00";

    /// A draft's server, in memory: one plan (the default snapshot, which never
    /// changes), a few documents, and a record of what the loop did.
    struct FakeDraft {
        tools: Tools,
        docs: Mutex<HashMap<i64, Vec<String>>>,
        /// Documents that are held originals: id -> (mime, bytes).
        held: Mutex<HashMap<i64, (String, Vec<u8>)>>,
        notes: Mutex<Vec<NewNote>>,
        extractions: Mutex<Vec<(i64, String)>>,
        progress: Mutex<Vec<String>>,
        blocked: Mutex<Vec<(String, Vec<String>)>>,
        simulations: Mutex<Vec<usize>>,
        ids: AtomicI64,
        alive: AtomicBool,
    }

    impl FakeDraft {
        fn new() -> Self {
            Self {
                tools: Tools::new(),
                docs: Mutex::new(HashMap::from([(1, vec![STATEMENT.to_owned()])])),
                held: Mutex::new(HashMap::new()),
                notes: Mutex::new(Vec::new()),
                extractions: Mutex::new(Vec::new()),
                progress: Mutex::new(Vec::new()),
                blocked: Mutex::new(Vec::new()),
                simulations: Mutex::new(Vec::new()),
                ids: AtomicI64::new(100),
                alive: AtomicBool::new(true),
            }
        }

        fn added(&self) -> Vec<String> {
            self.notes
                .lock()
                .unwrap()
                .iter()
                .map(|n| n.draft.title.clone())
                .collect()
        }
    }

    impl ToolHost for FakeDraft {
        fn preview<'a>(&'a self, _: Vec<Change>) -> BoxFuture<'a, Result<Value, String>> {
            Box::pin(async { Err("no previews in a draft".into()) })
        }
        fn preflight(&self) -> Result<Value, String> {
            self.tools.preflight()
        }
        fn goal_seek<'a>(
            &'a self,
            request: tools::goal_seek::GoalSeekRequest,
        ) -> BoxFuture<'a, Result<Value, String>> {
            self.tools.goal_seek(request)
        }
        fn resolve_steps(
            &self,
            steps: &[Vec<Change>],
        ) -> Result<Vec<Vec<DiffLine>>, (usize, Vec<ChangeProblem>)> {
            self.tools.resolve_steps(steps)
        }
    }

    impl DraftHost for FakeDraft {
        fn alive(&self) -> BoxFuture<'_, bool> {
            Box::pin(async move { self.alive.load(Ordering::SeqCst) })
        }
        fn progress(&self, line: String) -> BoxFuture<'_, ()> {
            Box::pin(async move { self.progress.lock().unwrap().push(line) })
        }
        fn read_document(
            &self,
            id: i64,
            pages: Option<RangeInclusive<u32>>,
        ) -> BoxFuture<'_, Result<DocumentRead, String>> {
            Box::pin(async move {
                if let Some((mime, bytes)) = self.held.lock().unwrap().get(&id).cloned() {
                    return Ok(DocumentRead::Held {
                        filename: "photo.png".into(),
                        mime,
                        bytes,
                    });
                }
                let docs = self.docs.lock().unwrap();
                let Some(text) = docs.get(&id) else {
                    return Err(format!("there is no document #{id} in this draft"));
                };
                let all: Vec<DocumentPage> = text
                    .iter()
                    .enumerate()
                    .map(|(i, t)| DocumentPage {
                        page: i as u32 + 1,
                        text: t.clone(),
                    })
                    .collect();
                Ok(DocumentRead::Text {
                    filename: "statement.pdf".into(),
                    kind: "bank_statement".into(),
                    pages_total: all.len(),
                    pages: all
                        .into_iter()
                        .filter(|p| pages.as_ref().is_none_or(|r| r.contains(&p.page)))
                        .collect(),
                })
            })
        }
        fn store_extraction(
            &self,
            id: i64,
            text: String,
        ) -> BoxFuture<'_, Result<Vec<String>, String>> {
            Box::pin(async move {
                self.held.lock().unwrap().remove(&id);
                self.extractions.lock().unwrap().push((id, text.clone()));
                self.docs.lock().unwrap().insert(id, vec![text.clone()]);
                Ok(vec![text])
            })
        }
        fn document_tool(
            &self,
            tool: DocumentTool,
            id: i64,
            _months: Option<u32>,
        ) -> BoxFuture<'_, Result<Value, String>> {
            Box::pin(async move { Ok(json!({"tool": format!("{tool:?}"), "document": id})) })
        }
        fn return_profiles(&self) -> Vec<LibraryProfile> {
            Vec::new()
        }
        fn simulate(&self, steps: Vec<Vec<Change>>) -> BoxFuture<'_, Result<Value, String>> {
            Box::pin(async move {
                self.simulations.lock().unwrap().push(steps.len());
                Ok(json!({"iterations": 400, "stats": {"success_rate": 0.81}}))
            })
        }
        fn notes(&self) -> BoxFuture<'_, Vec<NoteSummary>> {
            Box::pin(async move {
                self.notes
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|n| NoteSummary {
                        key: n.key.clone(),
                        kind: n.draft.kind,
                        title: n.draft.title.clone(),
                        changes: n.draft.all_changes(),
                        open: !n.auto_add,
                    })
                    .collect()
            })
        }
        fn block_notes(
            &self,
            question: String,
            notes: Vec<String>,
        ) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.blocked.lock().unwrap().push((question, notes));
                Ok(())
            })
        }
        fn add_note(&self, note: NewNote) -> BoxFuture<'_, Result<AddedNote, String>> {
            Box::pin(async move {
                let added = note.auto_add && note.blocked_by.is_empty();
                if let Some(key) = &note.replaces {
                    self.notes
                        .lock()
                        .unwrap()
                        .retain(|n| n.key.as_deref() != Some(key));
                }
                self.notes.lock().unwrap().push(note);
                Ok(AddedNote {
                    id: self.ids.fetch_add(1, Ordering::SeqCst),
                    added,
                    not_added: None,
                    created: if added {
                        vec![json!({"key": "floor", "kind": "parameter", "id": 9})]
                    } else {
                        Vec::new()
                    },
                    counts: json!({"accounts": 0, "parameters": self.notes.lock().unwrap().len()}),
                })
            })
        }
    }

    fn client(script: &Arc<Script>) -> AiClient {
        AiClient::new(settings(), script.clone(), Some(KEY.into()))
            .with_registry(draft::shared_tools())
            .with_zero_data_retention(true)
    }

    fn input(questions: Vec<DraftQuestion>) -> DraftInput {
        DraftInput {
            context: "<today>2026-09-29</today> the person's context".into(),
            description: "I earn $205,000 a year and plan to retire at 60.".into(),
            documents: vec![(1, Some(STATEMENT))]
                .into_iter()
                .map(|(id, t)| (id, t.map(str::to_owned)))
                .collect(),
            questions,
        }
    }

    fn parameter(key: &str, name: &str, value: f64) -> Value {
        json!({"op": "add", "target": {"new_parameter": key}, "path": "",
               "value": {"name": name, "value": {"kind": "Money", "value": value}}})
    }

    fn note(title: &str, changes: Vec<Value>, extra: Value) -> Value {
        let mut note = json!({
            "kind": "add", "section": "plan", "title": title,
            "summary": "The one-line lead.", "reasoning": "Read from the statement.",
            "evidence": [{"ref": "document", "document_id": 1, "page": 1,
                          "excerpt": "Balance: $12,345.00"}],
            "paths": [{"key": "a", "label": "Add it", "recommended": true,
                       "steps": [{"key": "a", "title": "Add it", "changes": changes}]}]
        });
        for (k, v) in extra.as_object().unwrap() {
            note[k] = v.clone();
        }
        note
    }

    fn bonus_question() -> Value {
        json!({
            "key": "bonus", "prompt": "Do you get a yearly bonus?", "answer_type": "choice",
            "options": [{"value": "none", "label": "No bonus"}, {"value": "yes", "label": "Yes, add one"}]
        })
    }

    fn all_requests(script: &Script) -> Vec<Value> {
        script.requests()
    }

    #[tokio::test]
    async fn reading_asking_suspending_answering_and_resuming_is_one_conversation() {
        let host = FakeDraft::new();
        let first = Script::new(vec![
            reply(
                "tool_use",
                json!([call("read1", "read_document", json!({"id": 1}))]),
            ),
            reply(
                "tool_use",
                json!([
                    // The question comes last in the message but is served
                    // first, so the note after it can wait on it.
                    call(
                        "submit1",
                        "submit_suggestion",
                        note(
                            "Checking holds $12,345 at Acme Bank",
                            vec![parameter("floor", "Acme balance", 12_345.0)],
                            json!({"auto_add": true, "key": "acme", "column": "portfolio",
                                   "section": "portfolio"}),
                        )
                    ),
                    call(
                        "submit2",
                        "submit_suggestion",
                        note(
                            "Add a yearly bonus of the usual size",
                            vec![parameter("bonus", "Yearly bonus", 20_000.0)],
                            json!({"blocked_by": ["bonus"], "key": "bonus-note",
                                   "evidence": [{"ref": "description",
                                                 "excerpt": "I earn $205,000 a year"}]})
                        )
                    ),
                    call("ask1", "ask_user", json!({"questions": [bonus_question()]})),
                ]),
            ),
        ]);
        let outcome = draft::run(
            &client(&first),
            &host,
            &NoObserver,
            input(Vec::new()),
            Transcript::default(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(outcome.stop, DraftStop::Suspended);
        assert_eq!(outcome.segment_turns, 2);
        assert_eq!(outcome.questions.len(), 1);
        assert!(outcome.questions[0].is_open());
        assert_eq!(
            host.added(),
            [
                "Checking holds $12,345 at Acme Bank",
                "Add a yearly bonus of the usual size"
            ]
        );
        {
            let notes = host.notes.lock().unwrap();
            assert!(notes[0].auto_add);
            assert_eq!(notes[0].column, DraftColumn::Portfolio);
            assert_eq!(notes[1].blocked_by, ["bonus"]);
            // Not auto-added, and to plan by default.
            assert_eq!(notes[1].column, DraftColumn::Plan);
        }
        // What the model saw of its two submissions.
        let pending = serde_json::to_value(&outcome.transcript.pending).unwrap();
        let accepted: Value = serde_json::from_str(
            pending
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["part"]["tool_use_id"] == "submit1")
                .unwrap()["part"]["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(accepted["accepted"], true);
        assert_eq!(accepted["state"], "added");
        assert_eq!(accepted["created"][0]["id"], 9);
        assert_eq!(request_count(&first), 2);

        // The stored conversation keeps the statement by reference.
        let stored = serde_json::to_string(&outcome.transcript).unwrap();
        assert!(!stored.contains("Account ending 1234"), "{stored}");
        assert!(outcome.transcript.refs.contains_key("read1"));
        assert_eq!(outcome.transcript.pending.len(), 3);
        assert_eq!(outcome.transcript.usage.turns, 2);
        assert!(!host.progress.lock().unwrap().is_empty());

        // The answer arrives; the conversation resumes from what was stored.
        let mut answered = outcome.questions.clone();
        answered[0].answer = Some(json!("none"));
        let stored: Transcript = serde_json::from_str(&stored).unwrap();
        let second = Script::new(vec![
            reply(
                "tool_use",
                json!([call(
                    "submit3",
                    "submit_suggestion",
                    note(
                        "The person gets no bonus",
                        vec![parameter("nobonus", "Bonus", 0.0)],
                        json!({"evidence": [{"ref": "answer", "question_key": "bonus"}],
                               "replaces": "bonus-note"}),
                    )
                )]),
            ),
            reply(
                "end_turn",
                json!([{"type": "text", "text": "Draft ready."}]),
            ),
        ]);
        let resumed = draft::run(
            &client(&second),
            &host,
            &NoObserver,
            input(answered),
            stored,
            Some(Resume {
                state: "The draft now holds: 0 accounts.".into(),
                unblocked: vec!["bonus-note".into()],
                message: None,
                follow_up: false,
            }),
        )
        .await
        .unwrap();
        assert_eq!(resumed.stop, DraftStop::Finished);
        assert_eq!(resumed.summary.as_deref(), Some("Draft ready."));
        assert_eq!(
            resumed.transcript.usage.turns, 4,
            "turns total the whole job"
        );
        // The replaced note is gone, the answer note is in.
        assert_eq!(
            host.added(),
            [
                "Checking holds $12,345 at Acme Bank",
                "The person gets no bonus"
            ]
        );

        let requests = second.requests();
        let messages = requests[0]["messages"].as_array().unwrap();
        // opening, the model's two turns, then the resume turn.
        assert_eq!(messages.len(), 5);
        let opening = messages[0]["content"].as_array().unwrap();
        assert!(opening[0]["text"].as_str().unwrap().contains("2026-09-29"));
        assert_eq!(opening[0]["cache_control"]["type"], "ephemeral");
        let resume_turn = messages[4]["content"].as_array().unwrap();
        let result = |id: &str| {
            resume_turn
                .iter()
                .find(|p| p["tool_use_id"] == id)
                .unwrap_or_else(|| panic!("no result for {id}"))["content"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        // The statement is read into the resumed request again, from the draft.
        let read = messages[2]["content"][0]["content"].as_str().unwrap();
        assert!(read.contains("Account ending 1234"), "{read}");
        // ask_user's result is the answers; the other results are as they were.
        let answers: Value = serde_json::from_str(&result("ask1")).unwrap();
        assert_eq!(answers["answers"][0]["key"], "bonus");
        assert_eq!(answers["answers"][0]["answer"], "none");
        assert_eq!(answers["unblocked_notes"][0], "bonus-note");
        assert!(result("submit1").contains("\"accepted\":true"));
        assert!(
            resume_turn.last().unwrap()["text"]
                .as_str()
                .unwrap()
                .contains("The draft now holds")
        );
        // Drafting requests: ZDR routing, its own tools, three system blocks.
        assert_eq!(requests[0]["provider"]["zdr"], true);
        let names: Vec<&str> = requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for expected in [
            "ask_user",
            "read_document",
            "expand_template",
            "find_return_profile",
            "simulate_draft",
            "validate_changes",
            "preflight",
            "reference_facts",
            "finance_calc",
            "estimate_social_security",
            "estimate_taxes",
            "goal_seek",
            "summarize_transactions",
            "match_account",
            "reconcile",
            "submit_suggestion",
        ] {
            assert!(names.contains(&expected), "{expected} missing in {names:?}");
        }
        assert!(!names.contains(&"preview_changes"));
        // The writing style, then the draft's own blocks.
        assert_eq!(requests[0]["system"].as_array().unwrap().len(), 4);
        assert_eq!(requests[0]["system"][0]["text"], prompt::WRITING_STYLE);
    }

    fn request_count(script: &Script) -> usize {
        all_requests(script).len()
    }

    #[tokio::test]
    async fn a_draft_note_is_gated_by_its_own_rules() {
        let host = FakeDraft::new();
        let good = note(
            "Checking holds $12,345 at Acme Bank",
            vec![parameter("floor", "Acme balance", 12_345.0)],
            json!({}),
        );
        let script = Script::new(vec![
            reply(
                "tool_use",
                json!([
                    // A review kind, no evidence, unknown question.
                    call("k", "submit_suggestion", {
                        let mut n = note("A fix", vec![parameter("a", "A", 1.0)], json!({}));
                        n["kind"] = json!("fix");
                        n["evidence"] = json!([]);
                        n
                    }),
                    // Add without evidence.
                    call("e", "submit_suggestion", {
                        let mut n = note(
                            "No evidence at all",
                            vec![parameter("b", "B", 1.0)],
                            json!({}),
                        );
                        n["evidence"] = json!([]);
                        n
                    }),
                    // A quote that is not on the page; a run-based evidence kind.
                    call("q", "submit_suggestion", {
                        let mut n = note(
                            "A misquoted balance",
                            vec![parameter("c", "C", 1.0)],
                            json!({}),
                        );
                        n["evidence"] = json!([
                            {"ref": "document", "document_id": 1, "page": 1, "excerpt": "Balance: $99.00"},
                            {"ref": "ledger", "year": 2030},
                            {"ref": "document", "document_id": 5, "page": 1, "excerpt": "x"},
                            {"ref": "computed", "tool": "finance_calc", "call_id": "nope"}
                        ]);
                        n
                    }),
                    // auto_add with a choice of two paths, blocked by nothing asked.
                    call("a", "submit_suggestion", {
                        let mut n = note(
                            "Two ways to add it",
                            vec![parameter("d", "D", 1.0)],
                            json!({"auto_add": true, "blocked_by": ["never-asked"]}),
                        );
                        let second = n["paths"][0].clone();
                        n["paths"] = json!([second.clone(), {"key": "b", "label": "Another way", "steps": second["steps"].clone()}]);
                        n["paths"][1]["steps"][0]["changes"] = json!([parameter("d2", "D2", 2.0)]);
                        n
                    }),
                    // Results section, a bad key.
                    call("s", "submit_suggestion", {
                        let mut n = note(
                            "Wrong section and key",
                            vec![parameter("f", "F", 1.0)],
                            json!({"key": "Not Valid"}),
                        );
                        n["section"] = json!("results");
                        n
                    }),
                    // Changes that do nothing / cannot resolve.
                    call(
                        "c",
                        "submit_suggestion",
                        note(
                            "Edits an event that does not exist",
                            vec![
                                json!({"op": "replace", "target": {"event": 99999}, "path": "/name", "expect": "x", "value": "y"})
                            ],
                            json!({}),
                        )
                    ),
                ]),
            ),
            reply(
                "tool_use",
                json!([
                    call("ok", "submit_suggestion", good.clone()),
                    // The same note again, then the same title reworded.
                    call("dup1", "submit_suggestion", {
                        let mut n = good.clone();
                        n["paths"][0]["steps"][0]["changes"] =
                            json!([parameter("zzz", "Other name", 5.0)]);
                        n
                    }),
                    call("dup2", "submit_suggestion", {
                        let mut n = good.clone();
                        n["title"] = json!("Acme Bank: checking holds $12,345 ");
                        n["title"] = json!("checking holds $12,345 at acme bank!");
                        n
                    }),
                ]),
            ),
            reply("end_turn", json!([])),
        ]);
        let outcome = draft::run(
            &client(&script),
            &host,
            &NoObserver,
            input(Vec::new()),
            Transcript::default(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.stop, DraftStop::Finished);
        let requests = script.requests();
        let problems = |id: &str, n: usize| -> String {
            let result = json_result(&requests, n, id);
            assert_eq!(result["accepted"], false, "{id}: {result}");
            result["problems"].to_string()
        };
        assert!(problems("k", 1).contains("add or a check note"));
        assert!(problems("e", 1).contains("an add note cites where"));
        let q = problems("q", 1);
        assert!(q.contains("does not appear on page"), "{q}");
        assert!(q.contains("a draft has no run"), "{q}");
        assert!(q.contains("no document #5"), "{q}");
        assert!(q.contains("no successful call nope"), "{q}");
        let a = problems("a", 1);
        assert!(a.contains("you have not asked a question"), "{a}");
        assert!(a.contains("exactly one path"), "{a}");
        assert!(a.contains("cannot be auto_add"), "{a}");
        let s = problems("s", 1);
        assert!(s.contains("no results section"), "{s}");
        assert!(s.contains("key must be 1 to 32"), "{s}");
        assert!(problems("c", 1).contains("unknown_target") || problems("c", 1).contains("event"));

        let ok = json_result(&requests, 2, "ok");
        assert_eq!(ok["accepted"], true, "{ok}");
        assert_eq!(ok["state"], "open", "a note not auto_add stays open");
        // Duplicates of the draft's own notes, by edits and by title.
        assert!(problems("dup1", 2).contains("repeats a note already in the draft"));
        assert!(problems("dup2", 2).contains("repeats a note already in the draft"));
        assert_eq!(host.notes.lock().unwrap().len(), 1);
        assert_eq!(outcome.transcript.usage.turns, 3);
    }

    #[tokio::test]
    async fn only_three_questions_are_asked_in_all_and_they_must_be_well_formed() {
        let host = FakeDraft::new();
        let script = Script::new(vec![
            reply(
                "tool_use",
                json!([
                    call(
                        "q1",
                        "ask_user",
                        json!({"questions": [
                            {"key": "One", "prompt": "x", "answer_type": "choice", "options": [{"value": "a", "label": "A"}]},
                            {"key": "two", "prompt": "Money?", "answer_type": "money", "options": [{"value": "a", "label": "A"}]},
                            {"key": "three", "prompt": "Blocks a ghost", "answer_type": "text", "blocks": ["ghost"]},
                        ]})
                    ),
                    call("q2", "ask_user", json!({"questions": []})),
                    call("q3", "ask_user", json!({"questions": [1, 2, 3, 4]})),
                ]),
            ),
            reply("end_turn", json!([])),
        ]);
        let outcome = draft::run(
            &client(&script),
            &host,
            &NoObserver,
            input(Vec::new()),
            Transcript::default(),
            None,
        )
        .await
        .unwrap();
        // Nothing valid was asked, so the model went on and finished.
        assert_eq!(outcome.stop, DraftStop::Finished);
        assert!(outcome.questions.is_empty());
        let requests = script.requests();
        let (q1, is_error) = result_for(&requests, 1, "q1");
        assert!(is_error);
        assert!(q1.contains("must be 1 to 32"), "{q1}");
        assert!(q1.contains("two to six options"), "{q1}");
        assert!(q1.contains("only a choice has options"), "{q1}");
        assert!(q1.contains("not an open note of yours"), "{q1}");
        assert!(result_for(&requests, 1, "q2").1);
        assert!(result_for(&requests, 1, "q3").1);

        // Already three asked: a fourth is refused.
        let asked: Vec<DraftQuestion> = ["a", "b", "c"]
            .into_iter()
            .map(|k| DraftQuestion {
                key: k.into(),
                prompt: "?".into(),
                answer_type: AnswerType::Text,
                options: Vec::new(),
                blocks: Vec::new(),
                answer: Some(json!("yes")),
            })
            .collect();
        let script = Script::new(vec![
            reply(
                "tool_use",
                json!([call(
                    "q4",
                    "ask_user",
                    json!({"questions": [
                        {"key": "d", "prompt": "One more?", "answer_type": "text"}
                    ]})
                )]),
            ),
            reply("end_turn", json!([])),
        ]);
        let outcome = draft::run(
            &client(&script),
            &host,
            &NoObserver,
            input(asked),
            Transcript::default(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.stop, DraftStop::Finished);
        let (text, is_error) = result_for(&script.requests(), 1, "q4");
        assert!(is_error && text.contains("at most 3 questions"), "{text}");
    }

    #[test]
    fn answers_are_checked_against_their_question() {
        let question = |answer_type, options: &[&str]| DraftQuestion {
            key: "q".into(),
            prompt: "?".into(),
            answer_type,
            options: options
                .iter()
                .map(|v| QuestionOption {
                    value: (*v).into(),
                    label: (*v).into(),
                })
                .collect(),
            blocks: Vec::new(),
            answer: None,
        };
        let choice = question(AnswerType::Choice, &["per-fund", "mix"]);
        assert_eq!(validate_answer(&choice, &json!("mix")), Ok(json!("mix")));
        assert!(validate_answer(&choice, &json!("other")).is_err());
        assert!(validate_answer(&choice, &json!(3)).is_err());
        let money = question(AnswerType::Money, &[]);
        assert_eq!(validate_answer(&money, &json!(1200)), Ok(json!(1200.0)));
        assert_eq!(
            validate_answer(&money, &json!("$1,200.50")),
            Ok(json!(1200.5))
        );
        assert!(validate_answer(&money, &json!(-5)).is_err());
        assert!(validate_answer(&money, &json!("lots")).is_err());
        let date = question(AnswerType::Date, &[]);
        assert_eq!(
            validate_answer(&date, &json!("2030-02-01")),
            Ok(json!("2030-02-01"))
        );
        assert!(validate_answer(&date, &json!("2030-02-31")).is_err());
        assert!(validate_answer(&date, &json!("soon")).is_err());
        let text = question(AnswerType::Text, &[]);
        assert_eq!(validate_answer(&text, &json!("  hi ")), Ok(json!("hi")));
        assert!(validate_answer(&text, &json!("")).is_err());
        assert!(validate_answer(&text, &json!("x".repeat(600))).is_err());
    }

    #[tokio::test]
    async fn a_scan_is_shown_to_the_model_and_kept_only_as_its_reading() {
        let host = FakeDraft::new();
        host.held
            .lock()
            .unwrap()
            .insert(2, ("image/png".into(), vec![137, 80, 78, 71, 1, 2, 3]));
        let script = Script::new(vec![
            reply(
                "tool_use",
                json!([call("look", "read_document", json!({"id": 2}))]),
            ),
            reply(
                "tool_use",
                json!([call(
                    "keep",
                    "read_document",
                    json!({"id": 2, "extraction": "Vanguard 401k balance $88,000.00 as of 2026-09-01"})
                )]),
            ),
            reply(
                "tool_use",
                json!([call("note", "submit_suggestion", {
                    let mut n = note(
                        "The 401(k) holds $88,000",
                        vec![parameter("k401", "401k balance", 88_000.0)],
                        json!({}),
                    );
                    n["evidence"] = json!([{"ref": "document", "document_id": 2, "page": 1,
                                                "excerpt": "401k balance $88,000.00"}]);
                    n
                })]),
            ),
            reply("end_turn", json!([])),
        ]);
        let mut with_scan = input(Vec::new());
        with_scan.documents.push((2, None));
        let outcome = draft::run(
            &client(&script),
            &host,
            &NoObserver,
            with_scan,
            Transcript::default(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.stop, DraftStop::Finished);
        let requests = script.requests();
        // The second request carries the image in the result of the first call.
        let shown = &requests[1]["messages"][2]["content"][0]["content"];
        let parts = shown.as_array().unwrap();
        assert_eq!(parts[1]["type"], "image");
        assert_eq!(parts[1]["source"]["media_type"], "image/png");
        assert!(parts[1]["source"]["data"].as_str().unwrap().len() > 4);
        assert!(parts[0]["text"].as_str().unwrap().contains("extraction"));
        // The reading was stored, and a quote of it then checks out.
        assert_eq!(host.extractions.lock().unwrap().len(), 1);
        assert_eq!(json_result(&requests, 3, "note")["accepted"], true);
        // Neither the image nor the reading is in the stored conversation.
        let stored = serde_json::to_string(&outcome.transcript).unwrap();
        assert!(!stored.contains("\"image\""), "{stored}");
        assert!(
            !stored.contains("Vanguard 401k balance $88,000.00 as of"),
            "{stored}"
        );
    }

    #[tokio::test]
    async fn templates_profiles_and_simulations_are_served_and_budgeted() {
        let host = FakeDraft::new();
        let script = Script::new(vec![
            reply(
                "tool_use",
                json!([
                    call(
                        "t",
                        "expand_template",
                        json!({
                            "kind": "recurring_expense", "key_prefix": "rent-",
                            "params": {"name": "Rent", "amount": 2400.0, "interval": "Monthly",
                                       "from_account_id": 1, "start": {"kind": "Age", "years": 30}}
                        })
                    ),
                    call(
                        "bad",
                        "expand_template",
                        json!({"kind": "salary", "params": {"nope": 1}})
                    ),
                    call("f", "find_return_profile", json!({"ticker": "VTI"})),
                    call("s1", "simulate_draft", json!({})),
                    call(
                        "s2",
                        "simulate_draft",
                        json!({"steps": [[parameter("p", "P", 1.0)]]})
                    ),
                ]),
            ),
            reply("end_turn", json!([])),
        ]);
        let mut settings = settings();
        settings.max_previews = 1;
        let client =
            AiClient::new(settings, script.clone(), None).with_registry(draft::shared_tools());
        let outcome = draft::run(
            &client,
            &host,
            &NoObserver,
            input(Vec::new()),
            Transcript::default(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.stop, DraftStop::Finished);
        let requests = script.requests();
        let expansion = result_for(&requests, 1, "t");
        // Whatever the template lowers to, it is changes and their keys or a
        // named problem with the parameters; never a crash.
        assert!(!expansion.0.is_empty());
        assert!(result_for(&requests, 1, "bad").1);
        let profile = json_result(&requests, 1, "f");
        assert_eq!(profile["asset_class"], "UsEquity");
        assert_eq!(profile["source"], "history_preset");
        assert_eq!(json_result(&requests, 1, "s1")["iterations"], 400);
        let (text, is_error) = result_for(&requests, 1, "s2");
        assert!(is_error && text.contains("budget"), "{text}");
        assert_eq!(host.simulations.lock().unwrap().len(), 1);
        assert_eq!(outcome.transcript.usage.previews, 1);
    }

    #[tokio::test]
    async fn a_draft_may_goal_seek_twice_in_the_whole_job() {
        let host = FakeDraft::new();
        let seek = |id: &str| {
            call(
                id,
                "goal_seek",
                json!({"parameter": "Retirement age", "metric": "success_rate", "target": 0.9}),
            )
        };
        // The first segment spends one and suspends; the resumed one has one left.
        let first = Script::new(vec![
            reply("tool_use", json!([seek("a")])),
            reply(
                "tool_use",
                json!([call(
                    "q",
                    "ask_user",
                    json!({"questions": [{"key": "bonus", "prompt": "Any bonus?",
                        "answer_type": "choice",
                        "options": [{"value": "no", "label": "No bonus"},
                                    {"value": "yes", "label": "Yes"}]}]})
                )]),
            ),
        ]);
        let outcome = draft::run(
            &client(&first),
            &host,
            &NoObserver,
            input(Vec::new()),
            Transcript::default(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.stop, DraftStop::Suspended);
        assert_eq!(outcome.transcript.usage.goal_seeks, 1);
        assert_eq!(
            json_result(&first.requests(), 1, "a")["result"]["value_text"],
            "age 43"
        );

        let mut questions = outcome.questions.clone();
        questions[0].answer = Some(validate_answer(&questions[0], &json!("no")).unwrap());
        let second = Script::new(vec![
            reply("tool_use", json!([seek("b"), seek("c")])),
            reply("end_turn", json!([])),
        ]);
        let resumed = draft::run(
            &client(&second),
            &host,
            &NoObserver,
            input(questions),
            outcome.transcript,
            Some(Resume {
                state: "The draft now holds nothing.".into(),
                unblocked: Vec::new(),
                message: None,
                follow_up: false,
            }),
        )
        .await
        .unwrap();
        let requests = second.requests();
        let last = requests.len() - 1;
        assert!(!result_for(&requests, last, "b").1);
        let (capped, is_error) = result_for(&requests, last, "c");
        assert!(is_error && capped.contains("2 times"), "{capped}");
        assert_eq!(resumed.transcript.usage.goal_seeks, 2);
        assert_eq!(host.tools.goal_seeks.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn the_turn_budget_totals_the_whole_job_and_a_deleted_draft_stops_it() {
        let host = FakeDraft::new();
        let mut transcript = Transcript::default();
        transcript.usage.turns = settings().max_turns;
        let script = Script::new(vec![]);
        let outcome = draft::run(
            &client(&script),
            &host,
            &NoObserver,
            input(Vec::new()),
            transcript,
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.stop, DraftStop::TurnLimit);
        assert_eq!(outcome.segment_turns, 0);
        assert!(script.requests().is_empty());

        host.alive.store(false, Ordering::SeqCst);
        let outcome = draft::run(
            &client(&script),
            &host,
            &NoObserver,
            input(Vec::new()),
            Transcript::default(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.stop, DraftStop::Cancelled);
        assert!(script.requests().is_empty());

        // A first request that fails is an error; nothing was drafted.
        host.alive.store(true, Ordering::SeqCst);
        let failing = Script::new(vec![Err(TransportError::Status {
            status: 400,
            error_type: None,
            message: format!("bad request with {KEY}"),
        })]);
        let error = draft::run(
            &client(&failing),
            &host,
            &NoObserver,
            input(Vec::new()),
            Transcript::default(),
            None,
        )
        .await
        .unwrap_err();
        assert!(!error.to_string().contains(KEY), "{error}");
    }

    #[test]
    fn the_drafting_prompt_and_tools_are_static_and_complete() {
        let tools = draft::prompt::tools(&draft::shared_tools());
        let tools = tools.as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names.last(), Some(&"submit_suggestion"));
        assert_eq!(
            names.iter().filter(|n| **n == "ask_user").count(),
            1,
            "{names:?}"
        );
        let submit = tools.last().unwrap();
        let kinds = &submit["input_schema"]["properties"]["kind"]["enum"];
        assert_eq!(kinds, &json!(["add", "check"]));
        for field in ["auto_add", "blocked_by", "column", "key", "replaces"] {
            assert!(
                submit["input_schema"]["properties"].get(field).is_some(),
                "{field}"
            );
        }
        let evidence =
            &submit["input_schema"]["properties"]["evidence"]["items"]["properties"]["ref"]["enum"];
        assert_eq!(
            evidence,
            &json!(["document", "description", "answer", "computed"])
        );
        // The declarations of what a draft writes beyond the review's bodies.
        let extra = draft::prompt::extra_reference();
        for needed in [
            "TemplateRequest",
            "ParameterBody",
            "CreateProfile",
            "CreateTaxConfig",
            "UpdateScenario",
        ] {
            assert!(extra.contains(needed), "{needed}");
        }
        assert_eq!(extra, draft::prompt::extra_reference());
        assert!(draft::prompt::SYSTEM_PROMPT.contains("at most three questions"));
    }
}

#[test]
fn the_reference_says_an_assets_name_is_its_ticker() {
    let reference = super::prompt::reference();
    assert!(
        reference.contains("The ticker symbol alone"),
        "CreateAsset's field docs reach the model"
    );
    assert!(super::draft::prompt::SYSTEM_PROMPT.contains("its ticker symbol alone (VBTLX)"));
}
