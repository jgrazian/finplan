//! Named, typed scenario inputs.
use crate::{
    auth::session::CurrentUser,
    compile::rows::ScenarioGraph,
    error::{ApiError, ApiResult, on_unique_violation},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};

pub use finplan_plan::specs::parameters::{
    NamedParameter, ParameterBody, ParameterUsage, ParameterValueSpec, delete_refusal, usages,
    validate,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/scenarios/{scenario_id}/parameters",
            get(list).post(create),
        )
        .route(
            "/scenarios/{scenario_id}/parameters/{parameter_id}",
            axum::routing::patch(update).delete(destroy),
        )
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Vec<NamedParameter>>> {
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    graph
        .parameters
        .iter()
        .map(|p| {
            Ok(NamedParameter {
                id: p.id,
                scenario_id,
                name: p.name.clone(),
                value: p.try_into()?,
                uses: usages(&graph, p.id)?,
            })
        })
        .collect::<ApiResult<Vec<_>>>()
        .map(Json)
}
async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ParameterBody>,
) -> ApiResult<(StatusCode, Json<NamedParameter>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let mut conn = state.db.acquire().await?;
    let id = create_in(&mut conn, scenario_id, &body).await?;
    drop(conn);
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(NamedParameter {
            id,
            scenario_id,
            name: body.name.trim().to_owned(),
            value: body.value,
            uses: vec![],
        }),
    ))
}

/// Insert a parameter, as `POST /scenarios/{id}/parameters` does; returns its
/// id. The suggestion path writes through this too, inside its transaction.
pub(crate) async fn create_in(
    conn: &mut sqlx::SqliteConnection,
    scenario_id: i64,
    body: &ParameterBody,
) -> ApiResult<i64> {
    let name = validate(body)?.to_owned();
    let (kind, number, date, years, months) = body.value.fields();
    sqlx::query_scalar("INSERT INTO named_parameters (scenario_id,name,kind,number_value,date_value,age_years,age_months) VALUES (?1,?2,?3,?4,?5,?6,?7) RETURNING id")
        .bind(scenario_id).bind(&name).bind(kind).bind(number).bind(date).bind(years).bind(months).fetch_one(&mut *conn).await
        .map_err(|e| on_unique_violation(e,"a parameter with that name already exists"))
}
async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, parameter_id)): Path<(i64, i64)>,
    Json(body): Json<ParameterBody>,
) -> ApiResult<Json<NamedParameter>> {
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    let uses = usages(&graph, parameter_id)?;
    let mut tx = state.db.begin().await?;
    update_in(&mut tx, &graph, scenario_id, parameter_id, &body).await?;
    tx.commit().await?;
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(Json(NamedParameter {
        id: parameter_id,
        scenario_id,
        name: body.name.trim().to_owned(),
        value: body.value,
        uses,
    }))
}

/// Rewrite a parameter, as `PATCH /scenarios/{id}/parameters/{id}` does.
/// `live` is the plan as stored: it says what uses the parameter (a type
/// change is refused while anything does) and what a rename rewrites.
pub(crate) async fn update_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    live: &ScenarioGraph,
    scenario_id: i64,
    parameter_id: i64,
    body: &ParameterBody,
) -> ApiResult<()> {
    let old = live
        .parameters
        .iter()
        .find(|p| p.id == parameter_id)
        .ok_or(ApiError::NotFound("parameter"))?;
    let name = validate(body)?.to_owned();
    let (kind, number, date, years, months) = body.value.fields();
    if old.kind != kind
        && (!usages(live, parameter_id)?.is_empty()
            || super::expression_refs::used_by(
                live,
                super::expression_refs::Entity::Parameter(parameter_id),
            )?)
    {
        return Err(ApiError::Conflict(
            "remove references before changing parameter type".into(),
        ));
    }
    sqlx::query("UPDATE named_parameters SET name=?1,kind=?2,number_value=?3,date_value=?4,age_years=?5,age_months=?6 WHERE id=?7 AND scenario_id=?8")
        .bind(&name).bind(kind).bind(number).bind(date).bind(years).bind(months).bind(parameter_id).bind(scenario_id).execute(&mut **tx).await
        .map_err(|e| on_unique_violation(e,"a parameter with that name already exists"))?;
    if name != old.name {
        super::expression_refs::rerender(
            tx,
            live,
            super::expression_refs::Entity::Parameter(parameter_id),
            &name,
        )
        .await?;
    }
    Ok(())
}
async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, parameter_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    let mut conn = state.db.acquire().await?;
    destroy_in(&mut conn, &graph, scenario_id, parameter_id).await?;
    drop(conn);
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a parameter nothing uses, as `DELETE /scenarios/{id}/parameters/{id}`.
pub(crate) async fn destroy_in(
    conn: &mut sqlx::SqliteConnection,
    live: &ScenarioGraph,
    scenario_id: i64,
    parameter_id: i64,
) -> ApiResult<()> {
    delete_refusal(live, parameter_id)?;
    sqlx::query("DELETE FROM named_parameters WHERE id=?1 AND scenario_id=?2")
        .bind(parameter_id)
        .bind(scenario_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}
