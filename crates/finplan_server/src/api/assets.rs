//! Assets are scenario-scoped: a price and a return profile assignment.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use super::ReorderRequest;
use crate::auth::activity::{ActivityFields, Submitted};
use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;

use finplan_plan::specs::assets::{Asset, CreateAsset, UpdateAsset};
use finplan_plan::specs::assets::{NAME_TAKEN, check_initial_price};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/{scenario_id}/assets", get(list).post(create))
        .route("/scenarios/{scenario_id}/assets/reorder", post(reorder))
        .route(
            "/scenarios/{scenario_id}/assets/{id}",
            get(fetch).patch(update).delete(destroy),
        )
}

const COLUMNS: &str =
    "id, name, description, initial_price, return_profile_id, tracking_error, sort_order";

/// Confirm a return profile exists in the caller's library before pointing an
/// asset at it. The FK only proves the row exists, not that it is theirs.
async fn owned_profile(state: &AppState, profile_id: i64, user_id: &str) -> ApiResult<()> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT id FROM return_profiles WHERE id = ?1 AND user_id = ?2")
            .bind(profile_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    found
        .map(|_| ())
        .ok_or(ApiError::NotFound("return profile"))
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Vec<Asset>>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let rows: Vec<Asset> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM assets WHERE scenario_id = ?1 ORDER BY sort_order, id"
    ))
    .bind(scenario_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<Json<Asset>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let row: Option<Asset> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM assets WHERE id = ?1 AND scenario_id = ?2"
    ))
    .bind(id)
    .bind(scenario_id)
    .fetch_optional(&state.db)
    .await?;
    row.map(Json).ok_or(ApiError::NotFound("asset"))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(Submitted { body, fields }): Json<Submitted<CreateAsset>>,
) -> ApiResult<(StatusCode, Json<Asset>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    if let Some(profile_id) = body.return_profile_id {
        owned_profile(&state, profile_id, &user.id).await?;
    }

    let mut tx = state.db.begin().await?;
    let id = create_in(&mut tx, scenario_id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Asset,
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

    let row: Asset = sqlx::query_as(&format!("SELECT {COLUMNS} FROM assets WHERE id = ?1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok((StatusCode::CREATED, Json(row)))
}

/// Add an asset to the scenario; the SQL half of `POST .../assets`. The
/// caller has already checked any return profile is the user's.
pub(crate) async fn create_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    body: &CreateAsset,
) -> ApiResult<i64> {
    check_initial_price(body.initial_price)?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO assets
            (scenario_id, name, description, initial_price, return_profile_id,
             tracking_error, sort_order)
         VALUES (?1,?2,?3,?4,?5,?6,
                 COALESCE(?7, (SELECT COALESCE(MAX(sort_order), -1) + 1
                                 FROM assets WHERE scenario_id = ?1)))
         RETURNING id",
    )
    .bind(scenario_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(body.initial_price)
    .bind(body.return_profile_id)
    .bind(body.tracking_error)
    .bind(body.sort_order)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))?;
    Ok(id)
}

/// Put the scenario's assets in the order the body names.
async fn reorder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let current: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM assets WHERE scenario_id = ?1 ORDER BY sort_order, id")
            .bind(scenario_id)
            .fetch_all(&state.db)
            .await?;

    let affected = super::apply_order(&state.db, "assets", &current, &body.ids).await?;

    if affected > 0 {
        state.telemetry.mutation(
            Resource::Asset,
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

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
    Json(Submitted { body, fields }): Json<Submitted<UpdateAsset>>,
) -> ApiResult<Json<Asset>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let rename_graph = if body.name.is_some() {
        Some(crate::db::graph::load(&state.db, scenario_id, &user.id).await?)
    } else {
        None
    };
    if let Some(Some(profile_id)) = body.return_profile_id {
        owned_profile(&state, profile_id, &user.id).await?;
    }

    let mut tx = state.db.begin().await?;
    update_in(&mut tx, rename_graph.as_ref(), scenario_id, id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Asset,
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

    let row: Asset = sqlx::query_as(&format!("SELECT {COLUMNS} FROM assets WHERE id = ?1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(row))
}

/// Apply `body` to asset `id`; the SQL half of `PATCH .../assets/{id}`, after
/// the profile has been checked as the caller's. `rename_graph` is the plan
/// before the change, needed only when `body` renames the asset.
pub(crate) async fn update_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    rename_graph: Option<&finplan_plan::graph::ScenarioGraph>,
    scenario_id: i64,
    id: i64,
    body: &UpdateAsset,
) -> ApiResult<()> {
    // `Some(None)` is a deliberate unmap, `None` is silence about the mapping.
    let remap = body.return_profile_id.is_some();
    let profile_id = body.return_profile_id.flatten();
    let retrack = body.tracking_error.is_some();
    let affected = sqlx::query(
        "UPDATE assets SET
            name              = COALESCE(?3, name),
            description       = COALESCE(?4, description),
            initial_price     = COALESCE(?5, initial_price),
            return_profile_id = CASE WHEN ?9 THEN ?6 ELSE return_profile_id END,
            tracking_error    = CASE WHEN ?10 THEN ?7 ELSE tracking_error END,
            sort_order        = COALESCE(?8, sort_order),
            updated_at        = datetime('now')
          WHERE id = ?1 AND scenario_id = ?2",
    )
    .bind(id)
    .bind(scenario_id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(&body.description)
    .bind(body.initial_price)
    .bind(profile_id)
    .bind(body.tracking_error.flatten())
    .bind(body.sort_order)
    .bind(remap)
    .bind(retrack)
    .execute(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("asset"));
    }
    if let (Some(graph), Some(name)) = (rename_graph, &body.name) {
        super::expression_refs::rerender(
            tx,
            graph,
            finplan_plan::expression_refs::Entity::Asset(id),
            name.trim(),
        )
        .await?;
    }
    Ok(())
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    let mut conn = state.db.acquire().await?;
    destroy_in(&mut conn, &graph, scenario_id, id).await?;
    drop(conn);

    state.telemetry.mutation(
        Resource::Asset,
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

/// Delete an asset, as `DELETE /scenarios/{id}/assets/{asset}` does. `live` is
/// the plan as stored, for the expression check. The suggestion path writes
/// through this too, inside its transaction.
pub(crate) async fn destroy_in(
    conn: &mut sqlx::SqliteConnection,
    live: &finplan_plan::graph::ScenarioGraph,
    scenario_id: i64,
    id: i64,
) -> ApiResult<()> {
    finplan_plan::expression_refs::refuse_if_used(
        live,
        finplan_plan::expression_refs::Entity::Asset(id),
    )?;

    // `account_property.asset_id` is ON DELETE RESTRICT, so deleting an asset a
    // property account is built on fails at the database. Report that clearly.
    let held_by: Option<String> = sqlx::query_scalar(
        "SELECT a.name FROM account_property p JOIN accounts a ON a.id = p.account_id
          WHERE p.asset_id = ?1 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(account) = held_by {
        return Err(ApiError::Conflict(format!(
            "asset is the underlying value of property account '{account}'; delete that account first"
        )));
    }

    let affected = sqlx::query("DELETE FROM assets WHERE id = ?1 AND scenario_id = ?2")
        .bind(id)
        .bind(scenario_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("asset"));
    }
    Ok(())
}

impl ActivityFields for CreateAsset {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "initial_price",
        "return_profile_id",
        "tracking_error",
        "sort_order",
    ];
}

impl ActivityFields for UpdateAsset {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "initial_price",
        "return_profile_id",
        "tracking_error",
        "sort_order",
    ];
}
