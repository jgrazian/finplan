//! Resolve expression references through core IDs before renaming or deleting.
//!
//! The reading half (which expressions name an entity, what a rename rewrites
//! them to) is `finplan_plan::expression_refs`; only the SQL write is here.

use sqlx::{Sqlite, Transaction};

use crate::compile::rows::ScenarioGraph;
use crate::error::ApiResult;
pub(crate) use finplan_plan::expression_refs::{Entity, rerendered, used_by};

/// Render all expressions against new metadata from their compiled ID references.
/// The existing graph supplies the old names; callers update metadata and SQL in
/// the same transaction so neither half of a rename can persist alone.
pub(crate) async fn rerender(
    tx: &mut Transaction<'_, Sqlite>,
    graph: &ScenarioGraph,
    entity: Entity,
    new_name: &str,
) -> ApiResult<()> {
    for (amount_id, source) in rerendered(graph, entity, new_name)? {
        sqlx::query("UPDATE transfer_amounts SET expression_source=?1 WHERE id=?2")
            .bind(source)
            .bind(amount_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
