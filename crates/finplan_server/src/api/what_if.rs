//! What-if: an ordered stack of override layers over a saved plan.
//!
//! The stack itself is a client document stored whole per scenario (see
//! `what_if_stacks`), the same bargain the sweep layout makes. Running it is an
//! analysis job (`POST /scenarios/{id}/analyses` with `kind: "what-if"`), and
//! applying it writes the layers into the plan for real: parameter values are
//! set, and shocks and one-offs become fires-once age events.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use finplan_core::config::SimulationConfig;
use finplan_core::model::{
    AmountMode, Event as CoreEvent, EventEffect, EventId, EventTrigger, IncomeType, ParameterValue,
    TransferAmount,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::analysis::jobs::{InlineFailure, run_inline};
use crate::analysis::params::{ParamKind, PlanParameter, parameters};
use crate::analysis::results::{AnalysisOutcome, WhatIfOutcome};
use crate::api::analysis::CreateAnalysis;
use crate::auth::session::CurrentUser;
use crate::compile::{self, CompiledScenario, rows::ScenarioGraph};
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{JobKind as MetricKind, Origin};
use crate::runner::telemetry::Submission;
use crate::state::AppState;
use finplan_core::analysis::SweepProgress;

use super::events::EventBody;
use super::scenarios::Scenario;
use super::specs::{AmountSpec, EffectSpec, TriggerSpec};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/scenarios/{scenario_id}/what-if",
            get(load_stack).put(save_stack),
        )
        .route("/scenarios/{scenario_id}/what-if/apply", post(apply))
        .route("/scenarios/{scenario_id}/what-if/quick", post(quick))
}

/// Most simulations a quick what-if may spend. It holds a request open while
/// it runs, so it stays small; anything bigger is a job.
const MAX_QUICK_ITERATIONS: usize = 500;
/// What a quick what-if spends when the request names nothing.
const DEFAULT_QUICK_ITERATIONS: usize = 200;

/// A small what-if answered in the response itself.
#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct QuickWhatIf {
    /// The enabled layers only, in order. At most eight.
    pub layers: Vec<WhatIfLayer>,
    /// Simulations for the whole stack, split across its steps; at most 500.
    #[serde(default)]
    pub iterations: Option<usize>,
}

/// Cancels the engine when the request goes away.
///
/// A client that has moved on aborts its fetch, axum drops this handler's
/// future, and the drop flags the worker to stop rather than finishing an
/// answer nobody will read.
struct CancelOnDrop(SweepProgress);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// `POST /scenarios/{id}/what-if/quick`: the quick pass of a what-if, run
/// inline and returned directly — one round trip instead of start, poll and
/// fetch. Same checks and lowering as the job route; the refinement still
/// goes through `POST /scenarios/{id}/analyses`.
async fn quick(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<QuickWhatIf>,
) -> ApiResult<Json<WhatIfOutcome>> {
    let mut decision = Submission::new(&state.telemetry, MetricKind::WhatIf);
    let result = run_quick(&state, &user, scenario_id, body, &mut decision).await;
    decision.result(&result);
    result
}

async fn run_quick(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    body: QuickWhatIf,
    decision: &mut Submission,
) -> ApiResult<Json<WhatIfOutcome>> {
    let iterations = body
        .iterations
        .unwrap_or(DEFAULT_QUICK_ITERATIONS)
        .min(MAX_QUICK_ITERATIONS);
    let prepared = super::analysis::prepare(
        state,
        user,
        scenario_id,
        CreateAnalysis::WhatIf {
            layers: body.layers,
            iterations: Some(iterations),
        },
    )
    .await?;
    let permit =
        crate::billing::admit_compute_observed(&user.id, &state.telemetry, Origin::Request)?;
    decision.accepted();

    let progress = SweepProgress::new(prepared.spec.budget());
    let guard = CancelOnDrop(progress.clone());
    let outcome = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        run_inline(&prepared.base, &prepared.spec, &progress)
    })
    .await;
    drop(guard);
    match outcome {
        Ok(Ok(AnalysisOutcome::WhatIf(outcome))) => Ok(Json(outcome)),
        Ok(Err(InlineFailure::Cancelled)) => Err(ApiError::Conflict("what-if canceled".into())),
        Ok(Ok(_)) | Ok(Err(InlineFailure::Failed)) | Err(_) => {
            Err(ApiError::Internal("what-if failed".into()))
        }
    }
}

/// Most layers one analysis or apply may carry.
pub const MAX_LAYERS: usize = 8;
/// Most rows a stored stack may hold, enabled or not.
const MAX_ENTRIES: usize = 16;
/// Longest client-generated row key.
const MAX_ENTRY_ID: usize = 64;

/// One override on top of the plan.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[ts(export)]
pub enum WhatIfLayer {
    /// value in the analysis units of AnalysisParameter: age = years, amount = dollars,
    /// rate = fraction, date = UTC epoch days.
    Parameter {
        parameter_id: i64,
        value: f64,
    },
    MarketShock {
        age: u8,
        drop: f64,
    },
    OneOff {
        age: u8,
        amount: f64,
        account_id: Option<i64>,
    },
}

/// One row of the stored stack. `id` is a client-generated stable key.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WhatIfEntry {
    pub id: String,
    pub enabled: bool,
    pub layer: WhatIfLayer,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WhatIfStack {
    pub entries: Vec<WhatIfEntry>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ApplyWhatIf {
    /// Enabled layers, in order.
    pub layers: Vec<WhatIfLayer>,
    /// None = write into this scenario. Some(name) = duplicate first, then write into the copy
    /// (parameter/account ids mapped onto the copy's rows).
    #[serde(default)]
    pub new_scenario_name: Option<String>,
}

// ── the stored stack ────────────────────────────────────────────────────────

async fn load_stack(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<WhatIfStack>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let row: Option<String> = sqlx::query_scalar(
        "SELECT stack FROM what_if_stacks WHERE scenario_id = ?1 AND user_id = ?2",
    )
    .bind(scenario_id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;
    // A stack written by an older client that no longer parses is treated as
    // no stack: the screen starts empty, which is where it would start anyway.
    Ok(Json(
        row.and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
    ))
}

async fn save_stack(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<WhatIfStack>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    if body.entries.len() > MAX_ENTRIES {
        return Err(ApiError::bad_request(format!(
            "a what-if stack holds at most {MAX_ENTRIES} layers"
        )));
    }
    for entry in &body.entries {
        if entry.id.is_empty() || entry.id.len() > MAX_ENTRY_ID {
            return Err(ApiError::bad_request(format!(
                "each layer needs an id of 1–{MAX_ENTRY_ID} characters"
            )));
        }
        check_shape(&entry.layer)?;
    }
    let json =
        serde_json::to_string(&body).map_err(|_| ApiError::internal("unserializable stack"))?;
    sqlx::query(
        "INSERT INTO what_if_stacks (scenario_id, user_id, stack, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(scenario_id) DO UPDATE
            SET user_id = excluded.user_id,
                stack = excluded.stack,
                updated_at = excluded.updated_at",
    )
    .bind(scenario_id)
    .bind(&user.id)
    .bind(json)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Checks that need nothing but the layer itself.
fn check_shape(layer: &WhatIfLayer) -> ApiResult<()> {
    match layer {
        WhatIfLayer::Parameter { value, .. } if !value.is_finite() => {
            Err(ApiError::bad_request("a parameter override must be finite"))
        }
        WhatIfLayer::MarketShock { drop, .. } if !(*drop > 0.0 && *drop < 1.0) => Err(
            ApiError::bad_request("a market shock's drop is a fraction between 0 and 1"),
        ),
        WhatIfLayer::OneOff { amount, .. } if !amount.is_finite() || *amount == 0.0 => {
            Err(ApiError::bad_request("a one-off needs a non-zero amount"))
        }
        _ => Ok(()),
    }
}

// ── lowering layers onto a plan ─────────────────────────────────────────────

/// A layer checked against the plan it is applied to, with every id resolved.
enum Resolved<'a> {
    Parameter {
        param: &'a PlanParameter,
        value: ParameterValue,
    },
    MarketShock {
        age: u8,
        drop: f64,
    },
    OneOff {
        age: u8,
        amount: f64,
        /// Database id of the account the money moves through.
        account_id: i64,
    },
}

/// Check `layers` against the scenario and resolve their ids.
fn resolve<'a>(
    graph: &ScenarioGraph,
    available: &'a [PlanParameter],
    layers: &[WhatIfLayer],
) -> ApiResult<Vec<Resolved<'a>>> {
    if layers.len() > MAX_LAYERS {
        return Err(ApiError::bad_request(format!(
            "a what-if applies at most {MAX_LAYERS} layers"
        )));
    }
    let has_birth_date = graph.scenario.birth_date.is_some();
    let default_account = || {
        let mut banks: Vec<_> = graph
            .accounts
            .iter()
            .filter(|a| a.flavor == "Bank")
            .collect();
        banks.sort_by_key(|a| (a.sort_order, a.id));
        banks.first().map(|a| a.id)
    };

    layers
        .iter()
        .map(|layer| {
            check_shape(layer)?;
            let needs_birth_date = || {
                if has_birth_date {
                    Ok(())
                } else {
                    Err(ApiError::bad_request(
                        "age-based what-if layers need the scenario's birth date; set it on the plan first",
                    ))
                }
            };
            Ok(match layer {
                WhatIfLayer::Parameter {
                    parameter_id,
                    value,
                } => {
                    let param = available
                        .iter()
                        .find(|p| p.parameter_id == *parameter_id)
                        .ok_or_else(|| {
                            ApiError::bad_request(format!(
                                "parameter {parameter_id} is not in this plan"
                            ))
                        })?;
                    Resolved::Parameter {
                        param,
                        value: param.typed_value(*value)?,
                    }
                }
                WhatIfLayer::MarketShock { age, drop } => {
                    needs_birth_date()?;
                    Resolved::MarketShock {
                        age: *age,
                        drop: *drop,
                    }
                }
                WhatIfLayer::OneOff {
                    age,
                    amount,
                    account_id,
                } => {
                    needs_birth_date()?;
                    let account_id = match account_id {
                        Some(id) => {
                            let account =
                                graph.accounts.iter().find(|a| a.id == *id).ok_or_else(|| {
                                    ApiError::bad_request(format!(
                                        "account {id} is not in this plan"
                                    ))
                                })?;
                            if account.flavor != "Bank" && account.flavor != "Investment" {
                                return Err(ApiError::bad_request(
                                    "a one-off moves cash, so it needs a cash or investment account",
                                ));
                            }
                            *id
                        }
                        None => default_account().ok_or_else(|| {
                            ApiError::bad_request(
                                "this plan has no cash account for a one-off; name an account",
                            )
                        })?,
                    };
                    Resolved::OneOff {
                        age: *age,
                        amount: *amount,
                        account_id,
                    }
                }
            })
        })
        .collect()
}

/// What a what-if analysis runs: the plan and each cumulative step.
pub(crate) struct Lowered {
    pub steps: Vec<SimulationConfig>,
    pub plan_retirement_age: Option<f64>,
    pub what_if_retirement_age: Option<f64>,
}

/// Lower `layers` onto the compiled plan, one cumulative config per step.
pub(crate) fn lower(
    graph: &ScenarioGraph,
    compiled: &CompiledScenario,
    layers: &[WhatIfLayer],
) -> ApiResult<Lowered> {
    let available = parameters(compiled);
    let resolved = resolve(graph, &available, layers)?;

    let retirement = available
        .iter()
        .find(|p| p.kind == ParamKind::Age && p.name.to_lowercase().contains("retire"));
    let plan_retirement_age = retirement.map(|p| p.current);
    let mut what_if_retirement_age = plan_retirement_age;

    let mut next_event = compiled
        .config
        .events
        .iter()
        .map(|e| e.event_id.0)
        .max()
        .map_or(0, |id| id + 1);
    let mut synthetic_event = |effect: EventEffect, age: u8| -> ApiResult<CoreEvent> {
        let event_id = EventId(next_event);
        next_event = next_event
            .checked_add(1)
            .ok_or_else(|| ApiError::unprocessable("this plan has too many events"))?;
        Ok(CoreEvent {
            event_id,
            trigger: EventTrigger::Age {
                years: age,
                months: None,
            },
            effects: vec![effect],
            once: true,
        })
    };

    let mut current = compiled.config.clone();
    let mut steps = Vec::with_capacity(resolved.len() + 1);
    steps.push(current.clone());
    for layer in &resolved {
        match layer {
            Resolved::Parameter { param, value } => {
                current.parameters.insert(param.dense_id, *value);
                if retirement.is_some_and(|r| r.parameter_id == param.parameter_id)
                    && let ParameterValue::Age(age) = value
                {
                    what_if_retirement_age =
                        Some(f64::from(age.years) + f64::from(age.months) / 12.0);
                }
            }
            Resolved::MarketShock { age, drop } => {
                let event = synthetic_event(EventEffect::MarketShock { drop: *drop }, *age)?;
                current.events.push(event);
            }
            Resolved::OneOff {
                age,
                amount,
                account_id,
            } => {
                let account = compiled.id_map.account(*account_id)?;
                let effect = if *amount < 0.0 {
                    EventEffect::Expense {
                        from: account,
                        amount: TransferAmount::fixed(-amount),
                    }
                } else {
                    EventEffect::Income {
                        to: account,
                        amount: TransferAmount::fixed(*amount),
                        amount_mode: AmountMode::Gross,
                        income_type: IncomeType::TaxFree,
                    }
                };
                current.events.push(synthetic_event(effect, *age)?);
            }
        }
        steps.push(current.clone());
    }

    Ok(Lowered {
        steps,
        plan_retirement_age,
        what_if_retirement_age,
    })
}

// ── apply ───────────────────────────────────────────────────────────────────

/// Write the layers into the plan: this scenario, or a fresh copy of it.
async fn apply(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ApplyWhatIf>,
) -> ApiResult<Json<Scenario>> {
    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    let compiled = compile::compile(&graph)?;
    let available = parameters(&compiled);
    let resolved = resolve(&graph, &available, &body.layers)?;
    let new_name = body
        .new_scenario_name
        .as_deref()
        .map(str::trim)
        .map(str::to_string);

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let (target, maps) = match &new_name {
        Some(name) => {
            crate::billing::check_plan_slot(&mut tx, &user.id, &state.config, 1).await?;
            let cloned = crate::domain::clone_into_mapped(&mut tx, &graph, name).await?;
            (cloned.id, Some(cloned))
        }
        None => (scenario_id, None),
    };
    let account = |id: i64| -> ApiResult<i64> {
        match &maps {
            Some(maps) => maps
                .accounts
                .get(&id)
                .copied()
                .ok_or_else(|| ApiError::internal("account missing from the copy")),
            None => Ok(id),
        }
    };
    let parameter = |id: i64| -> ApiResult<i64> {
        match &maps {
            Some(maps) => maps
                .parameters
                .get(&id)
                .copied()
                .ok_or_else(|| ApiError::internal("parameter missing from the copy")),
            None => Ok(id),
        }
    };

    let mut names: Vec<String> = graph.events.iter().map(|e| e.name.clone()).collect();
    for layer in &resolved {
        match layer {
            // Applied in order, so a later layer on the same parameter wins.
            Resolved::Parameter { param, value } => {
                let (kind, number, date, years, months) = value_columns(value);
                sqlx::query(
                    "UPDATE named_parameters
                        SET kind=?1, number_value=?2, date_value=?3, age_years=?4, age_months=?5
                      WHERE id=?6 AND scenario_id=?7",
                )
                .bind(kind)
                .bind(number)
                .bind(date)
                .bind(years)
                .bind(months)
                .bind(parameter(param.parameter_id)?)
                .bind(target)
                .execute(&mut *tx)
                .await?;
            }
            Resolved::MarketShock { age, drop } => {
                let name = unique_name(
                    &mut names,
                    format!("Market shock −{:.0}% at {age}", drop * 100.0),
                );
                insert_event(
                    &mut tx,
                    target,
                    &name,
                    *age,
                    EffectSpec::MarketShock { drop: *drop },
                )
                .await?;
            }
            Resolved::OneOff {
                age,
                amount,
                account_id,
            } => {
                let account_id = account(*account_id)?;
                let (label, effect) = if *amount < 0.0 {
                    (
                        "One-off cost",
                        EffectSpec::Expense {
                            from_account_id: account_id,
                            amount: AmountSpec::Fixed { value: -amount },
                        },
                    )
                } else {
                    (
                        "Windfall",
                        EffectSpec::Income {
                            to_account_id: account_id,
                            amount: AmountSpec::Fixed { value: *amount },
                            amount_mode: super::specs::AmountMode::Gross,
                            income_type: super::specs::IncomeType::TaxFree,
                        },
                    )
                };
                let name = unique_name(
                    &mut names,
                    format!("{label} {} at {age}", money(amount.abs())),
                );
                insert_event(&mut tx, target, &name, *age, effect).await?;
            }
        }
    }

    if new_name.is_none() {
        // The plan now says what the stack said; keeping the stack would
        // apply it twice.
        sqlx::query("DELETE FROM what_if_stacks WHERE scenario_id = ?1")
            .bind(target)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    super::touch_scenario(&state.db, target).await?;

    let row: Scenario = sqlx::query_as(&format!(
        "SELECT {} FROM scenarios WHERE id = ?1",
        super::scenarios::SCENARIO_COLUMNS
    ))
    .bind(target)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(row))
}

/// Append a fires-once age event to the end of the scenario's list.
async fn insert_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    name: &str,
    age: u8,
    effect: EffectSpec,
) -> ApiResult<()> {
    let body = EventBody {
        name: name.to_string(),
        description: Some("Applied from a what-if".to_string()),
        fires_once: true,
        enabled: true,
        sort_order: None,
        trigger: TriggerSpec::Age {
            years: age,
            months: None,
        },
        effects: vec![effect],
    };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO events (scenario_id, name, description, fires_once, enabled, sort_order)
         VALUES (?1,?2,?3,1,1,
                 (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM events WHERE scenario_id = ?1))
         RETURNING id",
    )
    .bind(scenario_id)
    .bind(&body.name)
    .bind(&body.description)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, "an event with that name already exists"))?;
    super::events::write_tree(tx, scenario_id, id, &body).await
}

/// `base`, or `base (2)`, `base (3)`… — event names are unique per scenario.
fn unique_name(taken: &mut Vec<String>, base: String) -> String {
    let mut name = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t == &name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    taken.push(name.clone());
    name
}

/// `$40k`, `$1.2M`, `$500`.
fn money(value: f64) -> String {
    if value >= 1_000_000.0 {
        let m = value / 1_000_000.0;
        if (m - m.round()).abs() < 0.05 {
            format!("${m:.0}M")
        } else {
            format!("${m:.1}M")
        }
    } else if value >= 1_000.0 {
        format!("${:.0}k", value / 1_000.0)
    } else {
        format!("${value:.0}")
    }
}

/// A typed parameter value as `named_parameters` columns.
fn value_columns(
    value: &ParameterValue,
) -> (
    &'static str,
    Option<f64>,
    Option<String>,
    Option<i64>,
    Option<i64>,
) {
    match value {
        ParameterValue::Money(v) => ("Money", Some(*v), None, None, None),
        ParameterValue::Rate(v) => ("Rate", Some(*v), None, None, None),
        ParameterValue::Date(d) => ("Date", None, Some(d.to_string()), None, None),
        ParameterValue::Age(age) => (
            "Age",
            None,
            None,
            Some(i64::from(age.years)),
            Some(i64::from(age.months)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_round_trip_with_kebab_case_tags() {
        let stack: WhatIfStack = serde_json::from_value(serde_json::json!({
            "entries": [
                {"id": "a", "enabled": true,
                 "layer": {"kind": "parameter", "parameter_id": 3, "value": 62}},
                {"id": "b", "enabled": false,
                 "layer": {"kind": "market-shock", "age": 67, "drop": 0.3}},
                {"id": "c", "enabled": true,
                 "layer": {"kind": "one-off", "age": 58, "amount": -40000, "account_id": null}}
            ]
        }))
        .unwrap();
        assert_eq!(stack.entries.len(), 3);
        let back = serde_json::to_value(&stack).unwrap();
        assert_eq!(back["entries"][1]["layer"]["kind"], "market-shock");
        assert_eq!(back["entries"][2]["layer"]["kind"], "one-off");
    }

    #[test]
    fn names_are_short_and_unique() {
        assert_eq!(money(40_000.0), "$40k");
        assert_eq!(money(1_250_000.0), "$1.2M");
        assert_eq!(money(2_000_000.0), "$2M");
        let mut taken = vec!["Windfall $40k at 58".to_string()];
        assert_eq!(
            unique_name(&mut taken, "Windfall $40k at 58".into()),
            "Windfall $40k at 58 (2)"
        );
    }
}
