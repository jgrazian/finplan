//! Writing a resolved batch through the routes' SQL halves. The in-memory half
//! (preview, and the dry run before a real write) is
//! [`finplan_plan::suggest::apply`]; both walk the batch in the same order.

use finplan_plan::graph::ScenarioGraph;
use finplan_plan::suggest::apply::writes_for;

use crate::api::{accounts, assets, events, parameters, profiles, scenarios, taxes};
use crate::error::{ApiError, ApiResult};
use finplan_plan::suggest::{
    Change, Created, CreatedRef, RefKind, Resolved, ResolvedChange, StepProblems, apply_to_graph,
    resolve_with,
};

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
    for &index in resolved.order() {
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
        let live = crate::db::graph::load_connection(tx, scenario_id, user_id).await?;
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
