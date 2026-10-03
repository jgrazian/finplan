//! Named, typed scenario inputs: the value spec, the request body, and the
//! checks on deleting one that an event still uses.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{PlanError, PlanResult};
use crate::expression_refs;
use crate::graph::{ParameterRow, ScenarioGraph};

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
    fn validate(&self) -> PlanResult<()> {
        match self {
            Self::Money { value } | Self::Rate { value } if !value.is_finite() => {
                Err(PlanError::invalid("parameter must be finite"))
            }
            Self::Date { value } => {
                value
                    .parse::<jiff::civil::Date>()
                    .map_err(|_| PlanError::invalid("invalid parameter date"))?;
                Ok(())
            }
            Self::Age { months, .. } if *months > 11 => {
                Err(PlanError::invalid("invalid calendar age"))
            }
            _ => Ok(()),
        }
    }
    pub fn fields(
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
    type Error = PlanError;
    fn try_from(row: &ParameterRow) -> PlanResult<Self> {
        Ok(match row.kind.as_str() {
            "Money" => Self::Money {
                value: row
                    .number_value
                    .ok_or_else(|| PlanError::internal("missing parameter value"))?,
            },
            "Rate" => Self::Rate {
                value: row
                    .number_value
                    .ok_or_else(|| PlanError::internal("missing parameter value"))?,
            },
            "Date" => Self::Date {
                value: row
                    .date_value
                    .clone()
                    .ok_or_else(|| PlanError::internal("missing parameter date"))?,
            },
            "Age" => Self::Age {
                years: row
                    .age_years
                    .ok_or_else(|| PlanError::internal("missing parameter age"))?
                    as u8,
                months: row
                    .age_months
                    .ok_or_else(|| PlanError::internal("missing parameter age"))?
                    as u8,
            },
            _ => return Err(PlanError::internal("unknown parameter kind")),
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

/// What a second parameter of the same name is refused with.
pub const NAME_TAKEN: &str = "a parameter with that name already exists";

pub fn validate(body: &ParameterBody) -> PlanResult<&str> {
    let name = body.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(PlanError::invalid(
            "parameter name must be 1–120 characters",
        ));
    }
    body.value.validate()?;
    Ok(name)
}

/// The refusal for giving a parameter a different type while anything still
/// reads it. `old_kind` is the type it has now.
pub fn check_retype(
    graph: &ScenarioGraph,
    parameter_id: i64,
    old_kind: &str,
    body: &ParameterBody,
) -> PlanResult<()> {
    let (kind, ..) = body.value.fields();
    if old_kind != kind
        && (!usages(graph, parameter_id)?.is_empty()
            || expression_refs::used_by(graph, expression_refs::Entity::Parameter(parameter_id))?)
    {
        return Err(PlanError::Conflict(
            "remove references before changing parameter type".into(),
        ));
    }
    Ok(())
}

/// The refusal a parameter's deletion meets, if any: it is used by an event.
pub fn delete_refusal(graph: &ScenarioGraph, parameter_id: i64) -> PlanResult<()> {
    if graph.parameters.iter().all(|p| p.id != parameter_id) {
        return Err(PlanError::NotFound("parameter"));
    }
    if !usages(graph, parameter_id)?.is_empty()
        || expression_refs::used_by(graph, expression_refs::Entity::Parameter(parameter_id))?
    {
        return Err(PlanError::Conflict("parameter is used by an event".into()));
    }
    Ok(())
}

pub fn usages(graph: &ScenarioGraph, parameter_id: i64) -> PlanResult<Vec<ParameterUsage>> {
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
    if [row.amount_id, row.down_payment_amount_id]
        .into_iter()
        .flatten()
        .filter_map(|id| graph.amounts.get(&id))
        .any(|amount| {
            amount
                .expression_source
                .as_deref()
                .and_then(|s| {
                    finplan_core::expression::compile_amount(s, metadata, parameters).ok()
                })
                .is_some_and(|amount| {
                    amount
                        .amount
                        .expression()
                        .references()
                        .parameters
                        .contains(&parameter)
                })
        })
    {
        return true;
    }
    graph.effect_children.iter().any(|((parent, _), child)| {
        *parent == id && effect_uses(graph, *child, parameter, metadata, parameters)
    })
}
