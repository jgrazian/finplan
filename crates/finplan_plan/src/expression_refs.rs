//! Resolve expression references through core IDs before renaming or deleting.
use crate::{
    compile,
    error::{PlanError, PlanResult},
    graph::ScenarioGraph,
};
use finplan_core::expression::compile_amount;

pub enum Entity {
    Account(i64),
    Asset(i64),
    Parameter(i64),
}

pub fn used_by(graph: &ScenarioGraph, entity: Entity) -> PlanResult<bool> {
    let (ids, metadata, parameters) = compile::expression_context(graph)?;
    for row in graph.amounts.values() {
        let Some(source) = &row.expression_source else {
            continue;
        };
        let amount = compile_amount(source, &metadata, &parameters)
            .map_err(|e| PlanError::unprocessable(e.to_string()))?;
        let refs = amount.amount.expression().references();
        let found = match entity {
            Entity::Account(id) => refs.accounts.contains(&ids.account(id)?),
            Entity::Asset(id) => refs.assets.contains(&ids.asset(id)?),
            Entity::Parameter(id) => refs.parameters.contains(&ids.parameter(id)?),
        };
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The expression sources a rename rewrites, as `(amount id, new source)`.
///
/// `graph` is the plan before the rename. Only expressions that reference the
/// renamed entity are returned; the rest keep their text.
pub fn rerendered(
    graph: &ScenarioGraph,
    entity: Entity,
    new_name: &str,
) -> PlanResult<Vec<(i64, String)>> {
    let mut out = Vec::new();
    let (ids, old_metadata, parameters) = compile::expression_context(graph)?;
    let mut metadata = old_metadata.clone();
    match entity {
        Entity::Account(id) => {
            metadata.register_account(ids.account(id)?, Some(new_name.to_string()), None)
        }
        Entity::Asset(id) => {
            metadata.register_asset(ids.asset(id)?, Some(new_name.to_string()), None)
        }
        Entity::Parameter(id) => {
            metadata.register_parameter(ids.parameter(id)?, Some(new_name.to_string()), None)
        }
    }
    for row in graph.amounts.values() {
        let Some(source) = &row.expression_source else {
            continue;
        };
        let amount = compile_amount(source, &old_metadata, &parameters)
            .map_err(|e| PlanError::unprocessable(e.to_string()))?;
        let refs = amount.amount.expression().references();
        let used = match entity {
            Entity::Account(id) => refs.accounts.contains(&ids.account(id)?),
            Entity::Asset(id) => refs.assets.contains(&ids.asset(id)?),
            Entity::Parameter(id) => refs.parameters.contains(&ids.parameter(id)?),
        };
        if !used {
            continue;
        }
        let rendered = amount
            .amount
            .to_source(&metadata)
            .map_err(|e| PlanError::unprocessable(e.to_string()))?;
        let rendered = match amount.amount_mode {
            Some(finplan_core::model::AmountMode::Gross) => format!("gross({rendered})"),
            Some(finplan_core::model::AmountMode::Net) => format!("net({rendered})"),
            None => rendered,
        };
        out.push((row.id, rendered));
    }
    Ok(out)
}
