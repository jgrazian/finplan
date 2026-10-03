//! Event CRUD. Triggers and effects travel as nested JSON and are exploded into
//! the self-referential tables by `finplan_plan::specs`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use super::ReorderRequest;
use crate::auth::activity::{ActivityFields, Submitted};
use crate::auth::session::CurrentUser;
use crate::db::Db;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;

use finplan_plan::specs::events::{Event, EventBody, lower_tree, read_event};
use finplan_plan::specs::events::{NAME_TAKEN, referenced_by};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/{scenario_id}/events", get(list).post(create))
        .route("/scenarios/{scenario_id}/events/reorder", post(reorder))
        .route(
            "/scenarios/{scenario_id}/events/{id}",
            get(fetch).put(replace).delete(destroy),
        )
}

// ── handlers ────────────────────────────────────────────────────────────────

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Vec<Event>>> {
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    let mut out = Vec::with_capacity(graph.events.len());
    for event in &graph.events {
        out.push(read_event(&graph, event.id)?);
    }
    Ok(Json(out))
}

/// Renumber the scenario's events to read as `ids`, as the route does once it
/// has checked the scenario is the caller's. Returns the rows renumbered.
pub(crate) async fn reorder_in(db: &Db, scenario_id: i64, ids: &[i64]) -> ApiResult<u64> {
    let current: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM events WHERE scenario_id = ?1 ORDER BY sort_order, id")
            .bind(scenario_id)
            .fetch_all(db)
            .await?;
    super::apply_order(db, "events", &current, ids).await
}

/// Put the scenario's events in the order the body names.
async fn reorder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let affected = reorder_in(&state.db, scenario_id, &body.ids).await?;

    if affected > 0 {
        state.telemetry.mutation(
            Resource::Event,
            Operation::Reordered,
            &EventFields {
                user_id: Some(&user.id),
                scenario_id: Some(scenario_id),
                fields: &["ids"],
                count: Some(affected),
                ..Default::default()
            },
        );
    }
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<Json<Event>> {
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    Ok(Json(read_event(&graph, id)?))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(Submitted { body, fields }): Json<Submitted<EventBody>>,
) -> ApiResult<(StatusCode, Json<Event>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let current = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    finplan_plan::expressions::validate_tree(&current, &body.effects)?;

    let mut tx = state.db.begin().await?;
    let id = create_in(&mut tx, scenario_id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Event,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;

    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    Ok((StatusCode::CREATED, Json(read_event(&graph, id)?)))
}

/// Replace an event wholesale.
///
/// PUT rather than PATCH: a trigger or effect list is a tree, and merging a
/// partial tree into an existing one has no sensible semantics. The old tree is
/// deleted and rewritten in one transaction.
async fn replace(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
    Json(Submitted { body, fields }): Json<Submitted<EventBody>>,
) -> ApiResult<Json<Event>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let current = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    finplan_plan::expressions::validate_tree(&current, &body.effects)?;

    let exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM events WHERE id = ?1 AND scenario_id = ?2")
            .bind(id)
            .bind(scenario_id)
            .fetch_optional(&state.db)
            .await?;
    exists.ok_or(ApiError::NotFound("event"))?;

    let mut tx = state.db.begin().await?;
    replace_in(&mut tx, scenario_id, id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Event,
        Operation::Updated,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;

    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    Ok(Json(read_event(&graph, id)?))
}

/// Delete tree nodes that nothing points at any more.
///
/// Rewriting or deleting an event drops its `effects` and root `triggers` rows,
/// but two kinds of node survive that cascade:
///
///   * `transfer_amounts`, which are referenced *by* effects rather than owned
///     by them, and
///   * the `Repeating` start/end sub-conditions, which hang off the parent's
///     own columns and so carry neither an `event_id` nor a `parent_id`.
///
/// Both are reachable only from the tree that was just removed, so collect them
/// here. Freeing a parent can orphan its children, so each sweep repeats until
/// it stops making progress.
async fn collect_orphans(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
) -> ApiResult<()> {
    loop {
        let removed = sqlx::query(
            "DELETE FROM transfer_amounts
              WHERE scenario_id = ?1
                AND NOT EXISTS (SELECT 1 FROM effects e
                                 WHERE e.amount_id = transfer_amounts.id
                                    OR e.down_payment_amount_id = transfer_amounts.id)
                AND NOT EXISTS (SELECT 1 FROM transfer_amounts p
                                 WHERE p.left_id = transfer_amounts.id
                                    OR p.right_id = transfer_amounts.id)",
        )
        .bind(scenario_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();

        if removed == 0 {
            break;
        }
    }

    loop {
        let removed = sqlx::query(
            "DELETE FROM triggers
              WHERE scenario_id = ?1
                AND event_id IS NULL
                AND parent_id IS NULL
                AND NOT EXISTS (SELECT 1 FROM triggers p
                                 WHERE p.start_trigger_id = triggers.id
                                    OR p.end_trigger_id = triggers.id)",
        )
        .bind(scenario_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();

        if removed == 0 {
            break;
        }
    }

    Ok(())
}

/// Write `body`'s trigger and effect trees for an event that has none.
pub(crate) async fn write_tree(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    event_id: i64,
    body: &EventBody,
) -> ApiResult<()> {
    let mut batch = crate::db::batch::batch_for_scenario(tx, scenario_id).await?;
    lower_tree(&mut batch, event_id, body)?;
    crate::db::batch::insert(tx, scenario_id, &batch).await?;
    Ok(())
}

/// Insert a new event and its trees; the SQL half of `POST .../events`.
pub(crate) async fn create_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    body: &EventBody,
) -> ApiResult<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO events (scenario_id, name, description, fires_once, enabled, sort_order)
         VALUES (?1,?2,?3,?4,?5,
                 COALESCE(?6, (SELECT COALESCE(MAX(sort_order), -1) + 1
                                 FROM events WHERE scenario_id = ?1)))
         RETURNING id",
    )
    .bind(scenario_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(i64::from(body.fires_once))
    .bind(i64::from(body.enabled))
    .bind(body.sort_order)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))?;

    write_tree(tx, scenario_id, id, body).await?;
    Ok(id)
}

/// Rewrite an event and its trees in place; the SQL half of `PUT .../events/{id}`.
pub(crate) async fn replace_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    id: i64,
    body: &EventBody,
) -> ApiResult<()> {
    let affected = sqlx::query(
        "UPDATE events SET name = ?3, description = ?4, fires_once = ?5, enabled = ?6,
                           sort_order = COALESCE(?7, sort_order),
                           updated_at = datetime('now')
          WHERE id = ?1 AND scenario_id = ?2",
    )
    .bind(id)
    .bind(scenario_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(i64::from(body.fires_once))
    .bind(i64::from(body.enabled))
    .bind(body.sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("event"));
    }
    // Child triggers cascade from the root; effects cascade from the event and
    // take their `Random` branches and withdrawal-source rows with them.
    sqlx::query("DELETE FROM triggers WHERE event_id = ?1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM effects WHERE event_id = ?1")
        .bind(id)
        .execute(&mut **tx)
        .await?;

    write_tree(tx, scenario_id, id, body).await?;
    collect_orphans(tx, scenario_id).await?;
    Ok(())
}

/// Delete an event unless another event points at it; the SQL half of
/// `DELETE .../events/{id}`.
pub(crate) async fn destroy_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    id: i64,
) -> ApiResult<()> {
    // Another event may point here via RelativeToEvent or a *Event effect;
    // those rows cascade-delete, which would silently drop a trigger condition.
    // Refuse instead and let the caller decide.
    let referrers: Vec<String> = sqlx::query_scalar(
        "SELECT e.name FROM triggers t JOIN events e ON e.id = t.event_id
          WHERE t.ref_event_id = ?1 AND t.event_id <> ?1
         UNION
         SELECT e.name FROM effects f JOIN events e ON e.id = f.event_id
          WHERE f.target_event_id = ?1 AND f.event_id <> ?1",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;

    if !referrers.is_empty() {
        return Err(referenced_by(&referrers).into());
    }

    let affected = sqlx::query("DELETE FROM events WHERE id = ?1 AND scenario_id = ?2")
        .bind(id)
        .bind(scenario_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("event"));
    }
    collect_orphans(tx, scenario_id).await
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let mut tx = state.db.begin().await?;
    destroy_in(&mut tx, scenario_id, id).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Event,
        Operation::Deleted,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

impl ActivityFields for EventBody {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "fires_once",
        "enabled",
        "sort_order",
        "trigger",
        "effects",
    ];
}
