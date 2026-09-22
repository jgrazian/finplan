//! Versioned, transactionally exported plan inputs and independent restoration.
use std::collections::{HashMap, HashSet};

use crate::observability::{EventFields, Operation, Resource};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, Transaction};
use ts_rs::TS;

use crate::{
    auth::session::CurrentUser,
    compile::{self, rows::ScenarioGraph},
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/{id}/archive", get(export_one))
        .route("/archives", get(export_all))
        .route("/runs/{id}/archive", get(export_run))
        .route("/archives/preview", post(preview))
        .route("/archives/import", post(import))
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanArchive {
    pub format: String,
    pub version: u32,
    /// Plan graph only: never sessions, credentials, or billing identifiers.
    #[ts(type = "unknown[]")]
    pub plans: Vec<Value>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ImportArchive {
    pub archive: PlanArchive,
    /// Prefix applied to every imported plan, so existing plans are never overwritten.
    pub name_prefix: String,
    /// Reusing this key returns the original result instead of creating duplicates.
    pub request_id: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ArchivePreview {
    pub names: Vec<String>,
    pub accounts: usize,
    pub events: usize,
    pub assumptions: usize,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ArchiveImported {
    pub scenario_ids: Vec<i64>,
}

async fn export_one(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<PlanArchive>> {
    let graph = ScenarioGraph::load(&state.db, id, &user.id).await?;
    let archive = pack(vec![graph])?;
    state.telemetry.mutation(
        Resource::Archive,
        Operation::Exported,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(id),
            resource_id: Some(id),
            count: Some(1),
            ..Default::default()
        },
    );
    Ok(Json(archive))
}

async fn export_run(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<PlanArchive>> {
    let snapshot: Option<Option<String>> =
        sqlx::query_scalar("SELECT snapshot_json FROM runs WHERE id=? AND user_id=?")
            .bind(id)
            .bind(&user.id)
            .fetch_optional(&state.db)
            .await?;
    let snapshot = snapshot
        .ok_or(ApiError::NotFound("run"))?
        .ok_or_else(|| ApiError::Conflict("This legacy run has no restorable inputs.".into()))?;
    let graph: ScenarioGraph =
        serde_json::from_str(&snapshot).map_err(|e| ApiError::internal(e.to_string()))?;
    let scenario_id = graph.scenario.id;
    let archive = pack(vec![graph])?;
    state.telemetry.mutation(
        Resource::Archive,
        Operation::Exported,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            count: Some(1),
            ..Default::default()
        },
    );
    Ok(Json(archive))
}

async fn export_all(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<PlanArchive>> {
    let mut tx = state.db.begin().await?;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM scenarios WHERE user_id=? ORDER BY id")
        .bind(&user.id)
        .fetch_all(&mut *tx)
        .await?;
    let mut graphs = Vec::new();
    for id in ids {
        graphs.push(ScenarioGraph::load_connection(&mut tx, id, &user.id).await?);
    }
    tx.commit().await?;
    let count = graphs.len() as u64;
    let archive = pack(graphs)?;
    state.telemetry.mutation(
        Resource::Archive,
        Operation::Exported,
        &EventFields {
            user_id: Some(&user.id),
            count: Some(count),
            ..Default::default()
        },
    );
    Ok(Json(archive))
}

pub(crate) fn pack(graphs: Vec<ScenarioGraph>) -> ApiResult<PlanArchive> {
    let plans = graphs
        .into_iter()
        .map(|mut graph| {
            graph.scenario.user_id.clear();
            crate::runner::inputs::snapshot(&graph)
                .map_err(|e| ApiError::internal(e.to_string()))
                .and_then(|(json, _)| {
                    serde_json::from_str(&json).map_err(|e| ApiError::internal(e.to_string()))
                })
        })
        .collect::<ApiResult<_>>()?;
    Ok(PlanArchive {
        format: "finplan.inputs".into(),
        version: 2,
        plans,
    })
}

fn unpack(archive: &PlanArchive) -> ApiResult<Vec<ScenarioGraph>> {
    if archive.format != "finplan.inputs" || archive.version != 2 {
        return Err(ApiError::bad_request(
            "Unsupported archive. Use a version 2 FinPlan input export; legacy browser and CLI archives have different formats.",
        ));
    }
    if archive.plans.is_empty() || archive.plans.len() > 100 {
        return Err(ApiError::bad_request("An import must contain 1–100 plans."));
    }
    archive
        .plans
        .iter()
        .map(|value| {
            let graph: ScenarioGraph = serde_json::from_value(value.clone())
                .map_err(|_| ApiError::bad_request("Invalid plan graph in archive."))?;
            validate_graph(&graph)?;
            Ok(graph)
        })
        .collect()
}

fn validate_graph(graph: &ScenarioGraph) -> ApiResult<()> {
    let size = graph.accounts.len()
        + graph.assets.len()
        + graph.events.len()
        + graph.distributions.len()
        + graph.amounts.len()
        + graph.effects.len()
        + graph.triggers.len()
        + graph.return_profiles.len()
        + graph.tax_brackets.len()
        + graph.withdrawal_sources.len()
        + graph.withdrawal_items.values().map(Vec::len).sum::<usize>()
        + graph.trigger_children.values().map(Vec::len).sum::<usize>()
        + graph.event_effects.values().map(Vec::len).sum::<usize>()
        + graph.positions.values().map(Vec::len).sum::<usize>();
    if size > 10_000 || !(1..=120).contains(&graph.scenario.duration_years) {
        return Err(ApiError::bad_request(
            "Archive exceeds the plan size or horizon limit.",
        ));
    }
    let invalid = || {
        ApiError::bad_request("Archive contains inconsistent record identifiers or tree ownership.")
    };
    let unique = |ids: Vec<i64>| -> bool {
        ids.iter().all(|id| *id > 0) && ids.iter().collect::<HashSet<_>>().len() == ids.len()
    };
    if !unique(graph.accounts.iter().map(|r| r.id).collect())
        || !unique(graph.assets.iter().map(|r| r.id).collect())
        || !unique(graph.events.iter().map(|r| r.id).collect())
    {
        return Err(invalid());
    }
    macro_rules! check_rows {
        ($rows:expr) => {
            if $rows.iter().any(|(id, row)| *id <= 0 || *id != row.id) {
                return Err(invalid());
            }
        };
    }
    check_rows!(graph.distributions);
    check_rows!(graph.return_profiles);
    check_rows!(graph.amounts);
    check_rows!(graph.effects);
    check_rows!(graph.triggers);
    // Compound trigger children point back at an already inserted parent. Parent
    // pointers are ownership, not additional semantic child edges.
    for (parent, children) in &graph.trigger_children {
        if !graph.triggers.contains_key(parent) || !unique(children.clone()) {
            return Err(invalid());
        }
        for child in children {
            if graph
                .triggers
                .get(child)
                .is_none_or(|r| r.parent_id != Some(*parent) || r.event_id.is_some())
            {
                return Err(invalid());
            }
        }
    }
    for row in graph.triggers.values() {
        if let Some(parent) = row.parent_id
            && !graph
                .trigger_children
                .get(&parent)
                .is_some_and(|c| c.contains(&row.id))
        {
            return Err(invalid());
        }
        for id in [row.start_trigger_id, row.end_trigger_id]
            .into_iter()
            .flatten()
        {
            if graph
                .triggers
                .get(&id)
                .is_none_or(|r| r.parent_id.is_some() || r.event_id.is_some())
            {
                return Err(invalid());
            }
        }
    }
    for (event, root) in &graph.event_trigger {
        if !graph.events.iter().any(|e| e.id == *event)
            || graph
                .triggers
                .get(root)
                .is_none_or(|r| r.event_id != Some(*event) || r.parent_id.is_some())
        {
            return Err(invalid());
        }
    }
    for (event, effects) in &graph.event_effects {
        if !graph.events.iter().any(|e| e.id == *event) || !unique(effects.clone()) {
            return Err(invalid());
        }
        for effect in effects {
            if graph
                .effects
                .get(effect)
                .is_none_or(|r| r.event_id != Some(*event) || r.parent_id.is_some())
            {
                return Err(invalid());
            }
        }
    }
    for ((parent, slot), child) in &graph.effect_children {
        if !graph.effects.contains_key(parent)
            || !["on_true", "on_false"].contains(&slot.as_str())
            || graph.effects.get(child).is_none_or(|r| {
                r.parent_id != Some(*parent)
                    || r.parent_slot.as_ref() != Some(slot)
                    || r.event_id.is_some()
            })
        {
            return Err(invalid());
        }
    }
    for row in graph.effects.values() {
        if let Some(parent) = row.parent_id
            && row.parent_slot.as_ref().is_none_or(|slot| {
                graph.effect_children.get(&(parent, slot.clone())) != Some(&row.id)
            })
        {
            return Err(invalid());
        }
    }
    // Guard even unused rows before the recursive copier sees them.
    check_dag(graph.distributions.iter().map(|(id, row)| {
        (
            *id,
            [row.bull_id, row.bear_id].into_iter().flatten().collect(),
        )
    }))?;
    check_dag(graph.amounts.iter().map(|(id, row)| {
        (
            *id,
            [row.left_id, row.right_id].into_iter().flatten().collect(),
        )
    }))?;
    check_dag(graph.triggers.iter().map(|(id, row)| {
        let mut children = graph.trigger_children.get(id).cloned().unwrap_or_default();
        children.extend(
            [row.start_trigger_id, row.end_trigger_id]
                .into_iter()
                .flatten(),
        );
        (*id, children)
    }))?;
    check_dag(graph.effects.keys().map(|id| {
        (
            *id,
            graph
                .effect_children
                .iter()
                .filter_map(|((parent, _), child)| (parent == id).then_some(*child))
                .collect(),
        )
    }))?;
    compile::compile(graph)
        .map_err(|e| ApiError::bad_request(format!("Archive plan cannot be compiled: {e}")))?;
    Ok(())
}

fn check_dag(rows: impl Iterator<Item = (i64, Vec<i64>)>) -> ApiResult<()> {
    let edges: HashMap<_, _> = rows.collect();
    fn visit(
        id: i64,
        edges: &HashMap<i64, Vec<i64>>,
        path: &mut HashSet<i64>,
        depth: usize,
        heights: &mut HashMap<i64, usize>,
    ) -> ApiResult<usize> {
        if let Some(height) = heights.get(&id) {
            if depth + height > 32 {
                return Err(ApiError::bad_request("Archive nesting exceeds limit."));
            }
            return Ok(*height);
        }
        if depth > 32 || !path.insert(id) {
            return Err(ApiError::bad_request(
                "Archive contains a cycle or excessive nesting.",
            ));
        }
        let children = edges
            .get(&id)
            .ok_or_else(|| ApiError::bad_request("Archive contains a missing reference."))?;
        let mut height = 0;
        for child in children {
            height = height.max(1 + visit(*child, edges, path, depth + 1, heights)?);
        }
        path.remove(&id);
        heights.insert(id, height);
        Ok(height)
    }
    let mut heights = HashMap::new();
    for id in edges.keys() {
        visit(*id, &edges, &mut HashSet::new(), 0, &mut heights)?;
    }
    Ok(())
}

async fn preview(
    _user: CurrentUser,
    Json(archive): Json<PlanArchive>,
) -> ApiResult<Json<ArchivePreview>> {
    let graphs = unpack(&archive)?;
    Ok(Json(ArchivePreview {
        names: graphs.iter().map(|g| g.scenario.name.clone()).collect(),
        accounts: graphs.iter().map(|g| g.accounts.len()).sum(),
        events: graphs.iter().map(|g| g.events.len()).sum(),
        assumptions: graphs
            .iter()
            .map(|g| {
                g.return_profiles.len()
                    + usize::from(g.tax_config.is_some())
                    + usize::from(g.inflation_distribution_id.is_some())
            })
            .sum(),
    }))
}

async fn import(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<ImportArchive>,
) -> ApiResult<Json<ArchiveImported>> {
    if body.request_id.is_empty() || body.request_id.len() > 100 || body.name_prefix.len() > 80 {
        return Err(ApiError::bad_request(
            "A request key and a name prefix of at most 80 characters are required.",
        ));
    }
    let mut graphs = unpack(&body.archive)?;
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(
                &serde_json::json!({"archive":body.archive,"prefix":body.name_prefix})
            )
            .map_err(|e| ApiError::internal(e.to_string()))?
        )
    );
    let access = crate::billing::entitlements(&state.db, &user.id, &state.config).await?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    if let Some((stored, result)) = sqlx::query_as::<_, (String, String)>(
        "SELECT input_hash,result_json FROM archive_imports WHERE user_id=? AND request_id=?",
    )
    .bind(&user.id)
    .bind(&body.request_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        if stored != fingerprint {
            return Err(ApiError::Conflict(
                "This request key was used for a different import.".into(),
            ));
        }
        let result: ArchiveImported =
            serde_json::from_str(&result).map_err(|e| ApiError::internal(e.to_string()))?;
        state.telemetry.mutation(
            Resource::Archive,
            Operation::Imported,
            &EventFields {
                user_id: Some(&user.id),
                count: Some(result.scenario_ids.len() as u64),
                replay: true,
                ..Default::default()
            },
        );
        return Ok(Json(result));
    }
    if let Some(limit) = access.saved_plan_limit {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scenarios WHERE user_id=?")
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
        if count as usize + graphs.len() > limit {
            return Err(ApiError::Forbidden("Free includes one saved plan. Export or read existing plans at any time; importing additional plans requires Pro.".into()));
        }
    }
    let mut scenario_ids = Vec::new();
    for graph in &mut graphs {
        let name = format!("{}{}", body.name_prefix, graph.scenario.name);
        scenario_ids.push(restore_graph(&mut tx, graph, &user.id, &name).await?);
    }
    let result = ArchiveImported { scenario_ids };
    sqlx::query(
        "INSERT INTO archive_imports(user_id,request_id,input_hash,result_json) VALUES(?,?,?,?)",
    )
    .bind(&user.id)
    .bind(&body.request_id)
    .bind(fingerprint)
    .bind(serde_json::to_string(&result).map_err(|e| ApiError::internal(e.to_string()))?)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Archive,
        Operation::Imported,
        &EventFields {
            user_id: Some(&user.id),
            count: Some(result.scenario_ids.len() as u64),
            ..Default::default()
        },
    );

    Ok(Json(result))
}

/// New assumptions always get independent rows, never name-based substitution.
pub(crate) async fn restore_graph(
    tx: &mut Transaction<'_, Sqlite>,
    graph: &mut ScenarioGraph,
    user: &str,
    name: &str,
) -> ApiResult<i64> {
    let suffix = uuid::Uuid::new_v4().to_string()[..8].to_owned();
    let mut distribution_ids = HashMap::new();
    let mut remaining: Vec<_> = graph.distributions.values().cloned().collect();
    remaining.sort_by_key(|row| row.id);
    while !remaining.is_empty() {
        let before = remaining.len();
        let mut deferred = Vec::new();
        for row in remaining {
            if [row.bull_id, row.bear_id]
                .into_iter()
                .flatten()
                .any(|id| !distribution_ids.contains_key(&id))
            {
                deferred.push(row);
                continue;
            }
            let id: i64 = sqlx::query_scalar("INSERT INTO distributions(user_id,kind,rate,mean,std_dev,scale,df,bull_id,bear_id,bull_to_bear_prob,bear_to_bull_prob,history_preset,block_size) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) RETURNING id")
                .bind(user).bind(&row.kind).bind(row.rate).bind(row.mean).bind(row.std_dev).bind(row.scale).bind(row.df)
                .bind(row.bull_id.map(|id|distribution_ids[&id])).bind(row.bear_id.map(|id|distribution_ids[&id]))
                .bind(row.bull_to_bear_prob).bind(row.bear_to_bull_prob).bind(&row.history_preset).bind(row.block_size)
                .fetch_one(&mut **tx).await?;
            distribution_ids.insert(row.id, id);
        }
        if deferred.len() == before {
            return Err(ApiError::bad_request("Invalid distribution references."));
        }
        remaining = deferred;
    }
    let mut profile_ids = HashMap::new();
    let mut profiles: Vec<_> = graph.return_profiles.values().collect();
    profiles.sort_by_key(|p| p.id);
    for profile in profiles {
        let distribution = distribution_ids
            .get(&profile.distribution_id)
            .ok_or_else(|| ApiError::bad_request("Missing profile distribution."))?;
        let id:i64 = sqlx::query_scalar("INSERT INTO return_profiles(user_id,name,description,distribution_id,asset_class) VALUES(?,?,?,?,?) RETURNING id")
            .bind(user).bind(format!("{} [{suffix}:{}]",profile.name,profile.id)).bind(&profile.description).bind(distribution).bind(&profile.asset_class)
            .fetch_one(&mut **tx).await?;
        profile_ids.insert(profile.id, id);
    }
    graph.scenario.inflation_profile_id = if let Some(distribution) =
        graph.inflation_distribution_id
    {
        let mapped = distribution_ids
            .get(&distribution)
            .ok_or_else(|| ApiError::bad_request("Missing inflation distribution."))?;
        Some(sqlx::query_scalar("INSERT INTO inflation_profiles(user_id,name,description,distribution_id) VALUES(?,?,?,?) RETURNING id")
            .bind(user).bind(format!("{} [{suffix}]", graph.inflation_profile_name.as_deref().unwrap_or("Imported inflation"))).bind("Restored input assumptions").bind(mapped).fetch_one(&mut **tx).await?)
    } else {
        None
    };
    graph.scenario.tax_config_id = if let Some(tax) = &graph.tax_config {
        let id:i64 = sqlx::query_scalar("INSERT INTO tax_configs(user_id,name,state_rate,capital_gains_rate,early_withdrawal_penalty_rate) VALUES(?,?,?,?,?) RETURNING id")
            .bind(user).bind(format!("{} [{suffix}]",tax.name)).bind(tax.state_rate).bind(tax.capital_gains_rate).bind(tax.early_withdrawal_penalty_rate).fetch_one(&mut **tx).await?;
        for bracket in &graph.tax_brackets {
            sqlx::query("INSERT INTO tax_brackets(tax_config_id,threshold,rate) VALUES(?,?,?)")
                .bind(id)
                .bind(bracket.threshold)
                .bind(bracket.rate)
                .execute(&mut **tx)
                .await?;
        }
        Some(id)
    } else {
        None
    };
    let mapped = |id: i64| {
        profile_ids
            .get(&id)
            .copied()
            .ok_or_else(|| ApiError::bad_request("Missing return profile."))
    };
    for row in &mut graph.assets {
        row.return_profile_id = row.return_profile_id.map(mapped).transpose()?;
    }
    for row in graph.bank.values_mut() {
        row.return_profile_id = mapped(row.return_profile_id)?;
    }
    for row in graph.investment.values_mut() {
        row.cash_return_profile_id = mapped(row.cash_return_profile_id)?;
    }
    graph.scenario.user_id = user.to_owned();
    crate::domain::clone_into(tx, graph, name).await
}
