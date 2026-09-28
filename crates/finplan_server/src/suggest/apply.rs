//! Writing a resolved batch: into an in-memory [`ScenarioGraph`] (preview, and
//! the dry run before a real write) or through the routes' SQL halves (apply).
//!
//! Both walk the batch in [`Resolved`]'s write order — new assets, new
//! accounts, edits to assets and accounts, new events, edits to events,
//! deletes — and lower each target again as they reach it, with `{"$new":
//! key}` references replaced by the ids of rows already written. That is why
//! a batch resolved against one plan can be written to another holding the
//! same rows under different ids (a snapshot's rows versus the database's).
//!
//! A suggestion's path is a sequence of batches ("steps"): each is resolved
//! against the plan the earlier ones produced, and may refer to what they
//! created. [`Created`] carries those keys from one batch to the next, and
//! from one request to the next when steps are applied one at a time.

use serde_json::Value;

use super::{
    Change, ChangeProblem, ChangeTarget, Created, CreatedRef, Names, RefKind, Resolved,
    ResolvedChange, effective_target, lower, resolve_with, substitute,
};
use crate::api::{accounts, assets, events, parameters, profiles, scenarios, taxes};
use crate::compile::rows::ScenarioGraph;
use crate::domain::edit;
use crate::error::{ApiError, ApiResult};

/// An edit's refusal as a problem with the batch; a database or internal
/// failure stays the server's.
pub fn plan_problem(
    err: ApiError,
    change: usize,
    target: ChangeTarget,
) -> ApiResult<ChangeProblem> {
    match err {
        ApiError::Database(_) | ApiError::Internal(_) => Err(err),
        other => Ok(ChangeProblem::InvalidBody {
            change,
            target,
            message: other.to_string(),
        }),
    }
}

/// The writes for one target, lowered with `created`'s real ids.
fn writes_for(
    graph: &ScenarioGraph,
    resolved: &Resolved,
    index: usize,
    created: &Created,
) -> Result<Vec<ResolvedChange>, ChangeProblem> {
    let delta = &resolved.deltas[index];
    let by_key = |key: &str| created.get(key).map(|c| Value::from(c.id));
    let after = delta.after.as_ref().map(|a| substitute(a, &by_key));
    let target = effective_target(&delta.target, created);
    lower(graph, &target, delta.before.as_ref(), after.as_ref(), false).map_err(|message| {
        ChangeProblem::InvalidBody {
            change: delta.last,
            target: delta.target.clone(),
            message,
        }
    })
}

/// Write `resolved` into `graph`, as the routes would write it into the plan.
/// On success, `created` gains the keys the batch created. All or nothing:
/// on a refusal neither `graph` nor `created` changes.
pub fn apply_to_graph(
    graph: &mut ScenarioGraph,
    resolved: &Resolved,
    created: &mut Created,
) -> ApiResult<Result<(), ChangeProblem>> {
    let mut staged = graph.clone();
    let mut keys = created.clone();
    for &index in &resolved.order {
        let delta = &resolved.deltas[index];
        let writes = match writes_for(&staged, resolved, index, &keys) {
            Ok(writes) => writes,
            Err(problem) => return Ok(Err(problem)),
        };
        for write in &writes {
            match write_graph(&mut staged, write) {
                Ok(Some((key, kind, id))) => {
                    keys.insert(key, CreatedRef { kind, id });
                }
                Ok(None) => {}
                Err(err) => {
                    return plan_problem(err, delta.last, delta.target.clone()).map(Err);
                }
            }
        }
    }
    *graph = staged;
    *created = keys;
    Ok(Ok(()))
}

/// One write into the graph; a create returns its key, kind and new id.
fn write_graph(
    g: &mut ScenarioGraph,
    write: &ResolvedChange,
) -> ApiResult<Option<(String, RefKind, i64)>> {
    Ok(match write {
        ResolvedChange::CreateEvent { key, body } => {
            Some((key.clone(), RefKind::Event, edit::create_event(g, body)?))
        }
        ResolvedChange::CreateAsset { key, body } => {
            Some((key.clone(), RefKind::Asset, edit::create_asset(g, body)?))
        }
        ResolvedChange::CreateAccount {
            key,
            body,
            positions,
        } => {
            let id = edit::create_account(g, body)?;
            for position in positions {
                edit::create_position(g, id, position)?;
            }
            Some((key.clone(), RefKind::Account, id))
        }
        ResolvedChange::ReplaceEvent { id, body } => {
            edit::replace_event(g, *id, body)?;
            None
        }
        ResolvedChange::DeleteEvent { id } => {
            edit::delete_event(g, *id)?;
            None
        }
        ResolvedChange::UpdateAsset { id, body } => {
            edit::update_asset(g, *id, body)?;
            None
        }
        ResolvedChange::UpdateAccount { id, body } => {
            edit::update_account(g, *id, body)?;
            None
        }
        ResolvedChange::CreatePosition { account_id, body } => {
            edit::create_position(g, *account_id, body)?;
            None
        }
        ResolvedChange::UpdatePosition {
            account_id,
            position_id,
            body,
        } => {
            edit::update_position(g, *account_id, *position_id, body)?;
            None
        }
        ResolvedChange::DeletePosition {
            account_id,
            position_id,
        } => {
            edit::delete_position(g, *account_id, *position_id)?;
            None
        }
        ResolvedChange::DeleteAsset { id } => {
            edit::delete_asset(g, *id)?;
            None
        }
        ResolvedChange::DeleteAccount { id } => {
            edit::delete_account(g, *id)?;
            None
        }
        ResolvedChange::CreateParameter { key, body } => Some((
            key.clone(),
            RefKind::Parameter,
            edit::create_parameter(g, body)?,
        )),
        ResolvedChange::ReplaceParameter { id, body } => {
            edit::update_parameter(g, *id, body)?;
            None
        }
        ResolvedChange::DeleteParameter { id } => {
            edit::delete_parameter(g, *id)?;
            None
        }
        ResolvedChange::UpdateScenario { body } => {
            edit::update_scenario(g, body)?;
            None
        }
        ResolvedChange::CreateReturnProfile { key, body } => Some((
            key.clone(),
            RefKind::ReturnProfile,
            edit::create_return_profile(g, body)?,
        )),
        ResolvedChange::CreateTaxConfig { key, body } => Some((
            key.clone(),
            RefKind::TaxConfig,
            edit::create_tax_config(g, body)?,
        )),
    })
}

/// Write `resolved` through the routes' SQL halves, inside the caller's
/// transaction. `live` is the plan the batch was resolved against, as stored
/// (it names renamed rows' old names for expression rewrites). `created` gains
/// the keys the batch created, with their real ids.
///
/// Run [`apply_to_graph`] on `live` first: it makes every check these writes
/// make, plus the expression checks they leave to the routes' handlers, so a
/// refusal here is a server fault rather than a problem with the batch.
pub async fn apply_to_sql(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    live: &ScenarioGraph,
    scenario_id: i64,
    resolved: &Resolved,
    created: &mut Created,
) -> ApiResult<()> {
    for &index in &resolved.order {
        let writes = writes_for(live, resolved, index, created)
            .map_err(|problem| ApiError::internal(format!("unchecked batch: {problem:?}")))?;
        for write in &writes {
            if let Some((key, kind, id)) = write_sql(tx, live, scenario_id, write).await? {
                created.insert(key, CreatedRef { kind, id });
            }
        }
    }
    Ok(())
}

async fn write_sql(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    live: &ScenarioGraph,
    scenario_id: i64,
    write: &ResolvedChange,
) -> ApiResult<Option<(String, RefKind, i64)>> {
    Ok(match write {
        ResolvedChange::CreateEvent { key, body } => Some((
            key.clone(),
            RefKind::Event,
            events::create_in(tx, scenario_id, body).await?,
        )),
        ResolvedChange::CreateAsset { key, body } => Some((
            key.clone(),
            RefKind::Asset,
            assets::create_in(tx, scenario_id, body).await?,
        )),
        ResolvedChange::CreateAccount {
            key,
            body,
            positions,
        } => {
            let id = accounts::create_in(tx, scenario_id, body).await?;
            for position in positions {
                accounts::add_position_in(tx, scenario_id, id, position).await?;
            }
            Some((key.clone(), RefKind::Account, id))
        }
        ResolvedChange::ReplaceEvent { id, body } => {
            events::replace_in(tx, scenario_id, *id, body).await?;
            None
        }
        ResolvedChange::DeleteEvent { id } => {
            events::destroy_in(tx, scenario_id, *id).await?;
            None
        }
        ResolvedChange::UpdateAsset { id, body } => {
            assets::update_in(tx, Some(live), scenario_id, *id, body).await?;
            None
        }
        ResolvedChange::UpdateAccount { id, body } => {
            accounts::update_in(tx, Some(live), scenario_id, *id, body).await?;
            None
        }
        ResolvedChange::CreatePosition { account_id, body } => {
            accounts::add_position_in(tx, scenario_id, *account_id, body).await?;
            None
        }
        ResolvedChange::UpdatePosition {
            account_id,
            position_id,
            body,
        } => {
            accounts::update_position_in(tx, scenario_id, *account_id, *position_id, body).await?;
            None
        }
        ResolvedChange::DeletePosition {
            account_id,
            position_id,
        } => {
            accounts::delete_position_in(tx, scenario_id, *account_id, *position_id).await?;
            None
        }
        ResolvedChange::DeleteAsset { id } => {
            assets::destroy_in(tx, live, scenario_id, *id).await?;
            None
        }
        ResolvedChange::DeleteAccount { id } => {
            accounts::destroy_in(tx, live, scenario_id, *id).await?;
            None
        }
        ResolvedChange::CreateParameter { key, body } => Some((
            key.clone(),
            RefKind::Parameter,
            parameters::create_in(tx, scenario_id, body).await?,
        )),
        ResolvedChange::ReplaceParameter { id, body } => {
            parameters::update_in(tx, live, scenario_id, *id, body).await?;
            None
        }
        ResolvedChange::DeleteParameter { id } => {
            parameters::destroy_in(tx, live, scenario_id, *id).await?;
            None
        }
        ResolvedChange::UpdateScenario { body } => {
            scenarios::update_in(tx, &live.scenario.user_id, scenario_id, body).await?;
            None
        }
        ResolvedChange::CreateReturnProfile { key, body } => Some((
            key.clone(),
            RefKind::ReturnProfile,
            profiles::create_return_in(tx, &live.scenario.user_id, body).await?,
        )),
        ResolvedChange::CreateTaxConfig { key, body } => Some((
            key.clone(),
            RefKind::TaxConfig,
            taxes::create_in(tx, &live.scenario.user_id, body).await?,
        )),
    })
}

// ── sequences of batches ─────────────────────────────────────────────────────

/// Why a sequence of batches cannot be applied: `step` indexes the sequence,
/// and each problem's `change` indexes that step's changes.
#[derive(Debug, Clone, PartialEq)]
pub struct StepProblems {
    pub step: usize,
    pub problems: Vec<ChangeProblem>,
}

/// One batch of a sequence, resolved against the plan before it.
#[derive(Debug)]
pub struct Step {
    pub resolved: Resolved,
    /// Display names in the plan the step was resolved against — including
    /// what earlier steps created — for [`Step::diff`].
    names: Names,
}

impl Step {
    /// The step's diff. `extra` adds names the plan lacks (return profiles a
    /// snapshot pruned).
    pub fn diff(&self, extra: impl IntoIterator<Item = (i64, String)>) -> Vec<super::DiffLine> {
        self.resolved.diff(&self.names.clone().with_profiles(extra))
    }
}

/// A sequence of batches applied, in memory, in order.
#[derive(Debug)]
pub struct Stepped {
    pub steps: Vec<Step>,
    /// The plan after every step.
    pub graph: ScenarioGraph,
    /// `seeded` plus everything the steps created, as ids in `graph`.
    pub created: Created,
}

/// Resolve each batch against the plan the ones before it left, applying each
/// in memory before resolving the next. `seeded` holds what earlier steps,
/// applied before this call, created in `graph`.
///
/// Load every return profile the steps may point at into `graph` first (see
/// [`profiles_named`]); a snapshot keeps only those its run used.
pub fn resolve_steps(
    graph: &ScenarioGraph,
    steps: &[Vec<Change>],
    seeded: &Created,
) -> ApiResult<Result<Stepped, StepProblems>> {
    let mut plan = graph.clone();
    let mut created = seeded.clone();
    let mut out = Vec::with_capacity(steps.len());
    for (step, changes) in steps.iter().enumerate() {
        let resolved = match resolve_with(&plan, changes, &created) {
            Ok(resolved) => resolved,
            Err(problems) => return Ok(Err(StepProblems { step, problems })),
        };
        let names = Names::from_graph(&plan);
        if let Err(problem) = apply_to_graph(&mut plan, &resolved, &mut created)? {
            return Ok(Err(StepProblems {
                step,
                problems: vec![problem],
            }));
        }
        out.push(Step { resolved, names });
    }
    Ok(Ok(Stepped {
        steps: out,
        graph: plan,
        created,
    }))
}

/// Write a sequence of batches into the stored plan, inside the caller's
/// transaction, step by step: each is resolved against the plan as the steps
/// before it left it in the database (so its `expect`s and ids are the stored
/// ones), checked in memory, then written. Returns `seeded` plus what the steps
/// created, as real ids.
pub async fn apply_steps_sql(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    user_id: &str,
    steps: &[Vec<Change>],
    seeded: &Created,
) -> ApiResult<Result<Created, StepProblems>> {
    let mut created = seeded.clone();
    for (step, changes) in steps.iter().enumerate() {
        let live = ScenarioGraph::load_connection(tx, scenario_id, user_id).await?;
        let resolved = match resolve_with(&live, changes, &created) {
            Ok(resolved) => resolved,
            Err(problems) => return Ok(Err(StepProblems { step, problems })),
        };
        let mut dry = live.clone();
        if let Err(problem) = apply_to_graph(&mut dry, &resolved, &mut created.clone())? {
            return Ok(Err(StepProblems {
                step,
                problems: vec![problem],
            }));
        }
        apply_to_sql(tx, &live, scenario_id, &resolved, &mut created).await?;
    }
    Ok(Ok(created))
}

/// Every return profile id the changes' values name, for loading them into a
/// snapshot before [`resolve_steps`].
pub fn profiles_named<'a>(changes: impl IntoIterator<Item = &'a Change>) -> Vec<i64> {
    fn walk(value: &Value, out: &mut Vec<i64>) {
        match value {
            Value::Object(object) => {
                for (key, child) in object {
                    if matches!(key.as_str(), "return_profile_id" | "cash_return_profile_id")
                        && let Some(id) = child.as_i64()
                    {
                        out.push(id);
                    }
                    walk(child, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for change in changes {
        if let Some(value) = &change.value {
            // A path ending at a profile field carries the id bare.
            if (change.path.ends_with("/return_profile_id")
                || change.path.ends_with("/cash_return_profile_id"))
                && let Some(id) = value.as_i64()
            {
                out.push(id);
            }
            walk(value, &mut out);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Every tax config id and inflation profile id the changes' values name, as
/// `(tax configs, inflation profiles)`, for loading them into a snapshot
/// before [`resolve_steps`] (see [`profiles_named`]).
pub fn assumptions_named<'a>(
    changes: impl IntoIterator<Item = &'a Change>,
) -> (Vec<i64>, Vec<i64>) {
    fn walk(value: &Value, tax: &mut Vec<i64>, inflation: &mut Vec<i64>) {
        match value {
            Value::Object(object) => {
                for (key, child) in object {
                    match (key.as_str(), child.as_i64()) {
                        ("tax_config_id", Some(id)) => tax.push(id),
                        ("inflation_profile_id", Some(id)) => inflation.push(id),
                        _ => {}
                    }
                    walk(child, tax, inflation);
                }
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, tax, inflation)),
            _ => {}
        }
    }
    let (mut tax, mut inflation) = (Vec::new(), Vec::new());
    for change in changes {
        if let Some(value) = &change.value {
            // A path ending at the field carries the id bare.
            if let Some(id) = value.as_i64() {
                if change.path.ends_with("/tax_config_id") {
                    tax.push(id);
                } else if change.path.ends_with("/inflation_profile_id") {
                    inflation.push(id);
                }
            }
            walk(value, &mut tax, &mut inflation);
        }
    }
    for ids in [&mut tax, &mut inflation] {
        ids.sort_unstable();
        ids.dedup();
    }
    (tax, inflation)
}
