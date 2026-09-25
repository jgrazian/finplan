//! Resolve expression references through core IDs before renaming or deleting.
use crate::{
    compile::{self, rows::ScenarioGraph},
    error::{ApiError, ApiResult},
};
use finplan_core::expression::compile_amount;
use sqlx::{Sqlite, Transaction};

pub(crate) enum Entity {
    Account(i64),
    Asset(i64),
    Parameter(i64),
}

pub(crate) fn used_by(graph: &ScenarioGraph, entity: Entity) -> ApiResult<bool> {
    let (ids, metadata, parameters) = compile::expression_context(graph)?;
    for row in graph.amounts.values() {
        let Some(source) = &row.expression_source else {
            continue;
        };
        let amount = compile_amount(source, &metadata, &parameters)
            .map_err(|e| ApiError::unprocessable(e.to_string()))?;
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

/// Render all expressions against new metadata from their compiled ID references.
/// The existing graph supplies the old names; callers update metadata and SQL in
/// the same transaction so neither half of a rename can persist alone.
pub(crate) async fn rerender(
    tx: &mut Transaction<'_, Sqlite>,
    graph: &ScenarioGraph,
    entity: Entity,
    new_name: &str,
) -> ApiResult<()> {
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
            .map_err(|e| ApiError::unprocessable(e.to_string()))?;
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
            .map_err(|e| ApiError::unprocessable(e.to_string()))?;
        let rendered = match amount.amount_mode {
            Some(finplan_core::model::AmountMode::Gross) => format!("gross({rendered})"),
            Some(finplan_core::model::AmountMode::Net) => format!("net({rendered})"),
            None => rendered,
        };
        sqlx::query("UPDATE transfer_amounts SET expression_source=?1 WHERE id=?2")
            .bind(rendered)
            .bind(row.id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
