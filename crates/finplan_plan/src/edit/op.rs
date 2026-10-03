//! One entry point for every plan edit: [`EditOp`] names a write route and
//! carries its body, [`apply`] runs it on a graph.
//!
//! The JSON is what a client would send the route with the route's path
//! folded in: `{"op": "update_asset", "id": 7, "body": {"name": "VTI"}}`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::PlanResult;
use crate::graph::ScenarioGraph;
use crate::specs::accounts::{CreateAccount, CreatePosition, UpdateAccount, UpdatePosition};
use crate::specs::assets::{CreateAsset, UpdateAsset};
use crate::specs::events::EventBody;
use crate::specs::parameters::ParameterBody;
use crate::specs::scenarios::UpdateScenario;

/// One write to a plan, as one of the plan-scoped routes the web calls. Each
/// variant is the in-memory twin of the route in its doc comment (under
/// `/scenarios/{id}`).
///
/// Return profiles, inflation profiles and tax configs are the user's library,
/// not the plan's: those writes are `LibraryOp` (`crate::library`).
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
#[ts(export)]
pub enum EditOp {
    /// `PATCH /scenarios/{id}`
    UpdateScenario { body: UpdateScenario },

    /// `POST …/assets`
    CreateAsset { body: CreateAsset },
    /// `PATCH …/assets/{id}`
    UpdateAsset { id: i64, body: UpdateAsset },
    /// `DELETE …/assets/{id}`
    DeleteAsset { id: i64 },
    /// `POST …/assets/reorder`
    ReorderAssets { ids: Vec<i64> },

    /// `POST …/accounts`
    CreateAccount { body: CreateAccount },
    /// `PATCH …/accounts/{id}`
    UpdateAccount { id: i64, body: UpdateAccount },
    /// `DELETE …/accounts/{id}`
    DeleteAccount { id: i64 },
    /// `POST …/accounts/reorder`
    ReorderAccounts { ids: Vec<i64> },

    /// `POST …/accounts/{account_id}/positions`
    CreatePosition {
        account_id: i64,
        body: CreatePosition,
    },
    /// `PATCH …/accounts/{account_id}/positions/{id}`
    UpdatePosition {
        account_id: i64,
        id: i64,
        body: UpdatePosition,
    },
    /// `DELETE …/accounts/{account_id}/positions/{id}`
    DeletePosition { account_id: i64, id: i64 },
    /// `POST …/accounts/{account_id}/positions/reorder`
    ReorderPositions { account_id: i64, ids: Vec<i64> },

    /// `POST …/events`
    CreateEvent { body: EventBody },
    /// `PUT …/events/{id}`
    ReplaceEvent { id: i64, body: EventBody },
    /// `DELETE …/events/{id}`
    DeleteEvent { id: i64 },
    /// `POST …/events/reorder`
    ReorderEvents { ids: Vec<i64> },

    /// `POST …/parameters`
    CreateParameter { body: ParameterBody },
    /// `PATCH …/parameters/{id}`
    UpdateParameter { id: i64, body: ParameterBody },
    /// `DELETE …/parameters/{id}`
    DeleteParameter { id: i64 },
}

/// What an applied edit reports back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EditOutcome {
    /// The id of the row a `Create*` op made; null for every other op.
    pub id: Option<i64>,
}

impl EditOutcome {
    fn created(id: i64) -> Self {
        EditOutcome { id: Some(id) }
    }

    fn done() -> Self {
        EditOutcome { id: None }
    }
}

/// Run `op` on `graph`. Atomic: on an error the graph is as it was.
///
/// The edits do not model `updated_at`; a caller that keeps one sets it after
/// each applied op (a reorder that moves nothing included, or not, as it
/// likes: the routes' reorders do not touch it).
pub fn apply(graph: &mut ScenarioGraph, op: &EditOp) -> PlanResult<EditOutcome> {
    use super::*;
    Ok(match op {
        EditOp::UpdateScenario { body } => {
            update_scenario(graph, body)?;
            EditOutcome::done()
        }

        EditOp::CreateAsset { body } => EditOutcome::created(create_asset(graph, body)?),
        EditOp::UpdateAsset { id, body } => {
            update_asset(graph, *id, body)?;
            EditOutcome::done()
        }
        EditOp::DeleteAsset { id } => {
            delete_asset(graph, *id)?;
            EditOutcome::done()
        }
        EditOp::ReorderAssets { ids } => {
            reorder_assets(graph, ids);
            EditOutcome::done()
        }

        EditOp::CreateAccount { body } => EditOutcome::created(create_account(graph, body)?),
        EditOp::UpdateAccount { id, body } => {
            update_account(graph, *id, body)?;
            EditOutcome::done()
        }
        EditOp::DeleteAccount { id } => {
            delete_account(graph, *id)?;
            EditOutcome::done()
        }
        EditOp::ReorderAccounts { ids } => {
            reorder_accounts(graph, ids);
            EditOutcome::done()
        }

        EditOp::CreatePosition { account_id, body } => {
            EditOutcome::created(create_position(graph, *account_id, body)?)
        }
        EditOp::UpdatePosition {
            account_id,
            id,
            body,
        } => {
            update_position(graph, *account_id, *id, body)?;
            EditOutcome::done()
        }
        EditOp::DeletePosition { account_id, id } => {
            delete_position(graph, *account_id, *id)?;
            EditOutcome::done()
        }
        EditOp::ReorderPositions { account_id, ids } => {
            reorder_positions(graph, *account_id, ids);
            EditOutcome::done()
        }

        EditOp::CreateEvent { body } => EditOutcome::created(create_event(graph, body)?),
        EditOp::ReplaceEvent { id, body } => {
            replace_event(graph, *id, body)?;
            EditOutcome::done()
        }
        EditOp::DeleteEvent { id } => {
            delete_event(graph, *id)?;
            EditOutcome::done()
        }
        EditOp::ReorderEvents { ids } => {
            reorder_events(graph, ids);
            EditOutcome::done()
        }

        EditOp::CreateParameter { body } => EditOutcome::created(create_parameter(graph, body)?),
        EditOp::UpdateParameter { id, body } => {
            update_parameter(graph, *id, body)?;
            EditOutcome::done()
        }
        EditOp::DeleteParameter { id } => {
            delete_parameter(graph, *id)?;
            EditOutcome::done()
        }
    })
}
