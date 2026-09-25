//! Named, typed scenario inputs.
use crate::{
    auth::session::CurrentUser,
    compile::rows::{ParameterRow, ScenarioGraph},
    error::{ApiError, ApiResult, on_unique_violation},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum ParameterValueSpec {
    Money { value: f64 },
    Rate { value: f64 },
    Date { value: String },
    Age { years: u8, months: u8 },
}
impl ParameterValueSpec {
    fn validate(&self) -> ApiResult<()> {
        match self {
            Self::Money { value } | Self::Rate { value } if !value.is_finite() => {
                Err(ApiError::bad_request("parameter must be finite"))
            }
            Self::Date { value } => {
                value
                    .parse::<jiff::civil::Date>()
                    .map_err(|_| ApiError::bad_request("invalid parameter date"))?;
                Ok(())
            }
            Self::Age { months, .. } if *months > 11 => {
                Err(ApiError::bad_request("invalid calendar age"))
            }
            _ => Ok(()),
        }
    }
    fn fields(
        &self,
    ) -> (
        &'static str,
        Option<f64>,
        Option<&str>,
        Option<i64>,
        Option<i64>,
    ) {
        match self {
            Self::Money { value } => ("Money", Some(*value), None, None, None),
            Self::Rate { value } => ("Rate", Some(*value), None, None, None),
            Self::Date { value } => ("Date", None, Some(value), None, None),
            Self::Age { years, months } => (
                "Age",
                None,
                None,
                Some(i64::from(*years)),
                Some(i64::from(*months)),
            ),
        }
    }
}
impl TryFrom<&ParameterRow> for ParameterValueSpec {
    type Error = ApiError;
    fn try_from(row: &ParameterRow) -> ApiResult<Self> {
        Ok(match row.kind.as_str() {
            "Money" => Self::Money {
                value: row
                    .number_value
                    .ok_or_else(|| ApiError::internal("missing parameter value"))?,
            },
            "Rate" => Self::Rate {
                value: row
                    .number_value
                    .ok_or_else(|| ApiError::internal("missing parameter value"))?,
            },
            "Date" => Self::Date {
                value: row
                    .date_value
                    .clone()
                    .ok_or_else(|| ApiError::internal("missing parameter date"))?,
            },
            "Age" => Self::Age {
                years: row
                    .age_years
                    .ok_or_else(|| ApiError::internal("missing parameter age"))?
                    as u8,
                months: row
                    .age_months
                    .ok_or_else(|| ApiError::internal("missing parameter age"))?
                    as u8,
            },
            _ => return Err(ApiError::internal("unknown parameter kind")),
        })
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ParameterUsage {
    pub event_id: i64,
    pub event_name: String,
    pub location: String,
}
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct NamedParameter {
    pub id: i64,
    pub scenario_id: i64,
    pub name: String,
    pub value: ParameterValueSpec,
    pub uses: Vec<ParameterUsage>,
}
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ParameterBody {
    pub name: String,
    pub value: ParameterValueSpec,
}
fn validate(body: &ParameterBody) -> ApiResult<&str> {
    let name = body.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::bad_request(
            "parameter name must be 1–120 characters",
        ));
    }
    body.value.validate()?;
    Ok(name)
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Vec<NamedParameter>>> {
    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
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
    let name = validate(&body)?.to_owned();
    let (kind, number, date, years, months) = body.value.fields();
    let id: i64 = sqlx::query_scalar("INSERT INTO named_parameters (scenario_id,name,kind,number_value,date_value,age_years,age_months) VALUES (?1,?2,?3,?4,?5,?6,?7) RETURNING id")
        .bind(scenario_id).bind(&name).bind(kind).bind(number).bind(date).bind(years).bind(months).fetch_one(&state.db).await
        .map_err(|e| on_unique_violation(e,"a parameter with that name already exists"))?;
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(NamedParameter {
            id,
            scenario_id,
            name,
            value: body.value,
            uses: vec![],
        }),
    ))
}
async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, parameter_id)): Path<(i64, i64)>,
    Json(body): Json<ParameterBody>,
) -> ApiResult<Json<NamedParameter>> {
    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    let old = graph
        .parameters
        .iter()
        .find(|p| p.id == parameter_id)
        .ok_or(ApiError::NotFound("parameter"))?;
    let name = validate(&body)?.to_owned();
    let uses = usages(&graph, parameter_id)?;
    let (kind, number, date, years, months) = body.value.fields();
    if old.kind != kind
        && (!uses.is_empty()
            || super::expression_refs::used_by(
                &graph,
                super::expression_refs::Entity::Parameter(parameter_id),
            )?)
    {
        return Err(ApiError::Conflict(
            "remove references before changing parameter type".into(),
        ));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE named_parameters SET name=?1,kind=?2,number_value=?3,date_value=?4,age_years=?5,age_months=?6 WHERE id=?7 AND scenario_id=?8")
        .bind(&name).bind(kind).bind(number).bind(date).bind(years).bind(months).bind(parameter_id).bind(scenario_id).execute(&mut *tx).await
        .map_err(|e| on_unique_violation(e,"a parameter with that name already exists"))?;
    if name != old.name {
        super::expression_refs::rerender(
            &mut tx,
            &graph,
            super::expression_refs::Entity::Parameter(parameter_id),
            &name,
        )
        .await?;
    }
    tx.commit().await?;
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(Json(NamedParameter {
        id: parameter_id,
        scenario_id,
        name,
        value: body.value,
        uses,
    }))
}
async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, parameter_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    if graph.parameters.iter().all(|p| p.id != parameter_id) {
        return Err(ApiError::NotFound("parameter"));
    }
    if !usages(&graph, parameter_id)?.is_empty()
        || super::expression_refs::used_by(
            &graph,
            super::expression_refs::Entity::Parameter(parameter_id),
        )?
    {
        return Err(ApiError::Conflict("parameter is used by an event".into()));
    }
    sqlx::query("DELETE FROM named_parameters WHERE id=?1 AND scenario_id=?2")
        .bind(parameter_id)
        .bind(scenario_id)
        .execute(&state.db)
        .await?;
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn usages(graph: &ScenarioGraph, parameter_id: i64) -> ApiResult<Vec<ParameterUsage>> {
    if graph.parameters.iter().all(|p| p.id != parameter_id) {
        return Ok(vec![]);
    };
    let (ids, metadata, parameters) = crate::compile::expression_context(graph)?;
    let dense = ids.parameter(parameter_id)?;
    Ok(graph
        .events
        .iter()
        .flat_map(|event| {
            let mut found = Vec::new();
            if graph
                .triggers
                .values()
                .any(|t| t.parameter_id == Some(parameter_id) && belongs(graph, t.id, event.id))
            {
                found.push(ParameterUsage {
                    event_id: event.id,
                    event_name: event.name.clone(),
                    location: "schedule".into(),
                });
            }
            if graph
                .event_effects
                .get(&event.id)
                .into_iter()
                .flatten()
                .any(|id| effect_uses(graph, *id, dense, &metadata, &parameters))
            {
                found.push(ParameterUsage {
                    event_id: event.id,
                    event_name: event.name.clone(),
                    location: "effect amount".into(),
                });
            }
            found
        })
        .collect())
}
fn belongs(graph: &ScenarioGraph, mut id: i64, event: i64) -> bool {
    for _ in 0..64 {
        let Some(row) = graph.triggers.get(&id) else {
            return false;
        };
        if row.event_id == Some(event) {
            return true;
        }
        if let Some(parent) = row.parent_id {
            id = parent;
            continue;
        }
        if let Some(parent) = graph
            .triggers
            .values()
            .find(|p| p.start_trigger_id == Some(id) || p.end_trigger_id == Some(id))
        {
            id = parent.id;
            continue;
        }
        return false;
    }
    false
}
fn effect_uses(
    graph: &ScenarioGraph,
    id: i64,
    parameter: finplan_core::model::ParameterId,
    metadata: &finplan_core::config::SimulationMetadata,
    parameters: &std::collections::HashMap<
        finplan_core::model::ParameterId,
        finplan_core::model::ParameterValue,
    >,
) -> bool {
    let Some(row) = graph.effects.get(&id) else {
        return false;
    };
    if row
        .amount_id
        .and_then(|id| graph.amounts.get(&id))
        .and_then(|a| a.expression_source.as_deref())
        .and_then(|s| finplan_core::expression::compile_amount(s, metadata, parameters).ok())
        .is_some_and(|amount| {
            amount
                .amount
                .expression()
                .references()
                .parameters
                .contains(&parameter)
        })
    {
        return true;
    }
    graph.effect_children.iter().any(|((parent, _), child)| {
        *parent == id && effect_uses(graph, *child, parameter, metadata, parameters)
    })
}
