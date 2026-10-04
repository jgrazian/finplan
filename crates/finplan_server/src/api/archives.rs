//! Versioned, transactionally exported plan inputs and independent restoration.
use std::collections::HashMap;

use crate::observability::{AuthAction, AuthOutcome, EventFields, Operation, Resource};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, Transaction};
use ts_rs::TS;

use crate::{
    auth::session::CurrentUser,
    error::{ApiError, ApiResult},
    state::AppState,
};
use finplan_plan::adopt::{IMPORTED_INFLATION, adopt, import_name};
use finplan_plan::archive::{self, ArchivePreview, PlanArchive, pack, unpack};
use finplan_plan::graph::{DistributionRow, ScenarioGraph};
use finplan_plan::library::{
    Library, LibraryInflationProfile, LibraryReturnProfile, LibraryTaxConfig,
};
use finplan_plan::specs::taxes::Bracket;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/{id}/archive", get(export_one))
        .route("/archives", get(export_all))
        .route("/runs/{id}/archive", get(export_run))
        .route("/archives/preview", post(preview))
        .route("/archives/import", post(import))
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ImportArchive {
    pub archive: PlanArchive,
    /// Prefix applied to every imported plan, so existing plans are never overwritten.
    pub name_prefix: String,
    /// Reusing this key returns the original result instead of creating duplicates.
    pub request_id: String,
    /// The web sets this when it brings a guest plan into an account after
    /// sign-in (spec 17), so adoption can be counted. It changes nothing else.
    #[serde(default)]
    pub from_guest: bool,
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
    let graph = crate::db::graph::load(&state.db, id, &user.id).await?;
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
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE user_id=? AND status='active' ORDER BY id",
    )
    .bind(&user.id)
    .fetch_all(&mut *tx)
    .await?;
    let mut graphs = Vec::new();
    for id in ids {
        graphs.push(crate::db::graph::load_connection(&mut tx, id, &user.id).await?);
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

async fn preview(
    _user: CurrentUser,
    Json(archive): Json<PlanArchive>,
) -> ApiResult<Json<ArchivePreview>> {
    Ok(Json(archive::preview(&archive)?))
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
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scenarios WHERE user_id=? AND status='active'",
        )
        .bind(&user.id)
        .fetch_one(&mut *tx)
        .await?;
        if count as usize + graphs.len() > limit {
            return Err(ApiError::Forbidden("Free includes one saved plan. Export or read existing plans at any time; importing additional plans requires Pro.".into()));
        }
    }
    // Read inside the transaction, and kept current as plans are restored, so
    // two plans of one archive that share a profile add it once.
    let mut library = crate::db::library::load_connection(&mut tx, &user.id).await?;
    let mut scenario_ids = Vec::new();
    for graph in &mut graphs {
        let name = format!("{}{}", body.name_prefix, graph.scenario.name);
        scenario_ids.push(restore_graph(&mut tx, &mut library, graph, &user.id, &name).await?);
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
    if body.from_guest {
        state.telemetry.auth(
            AuthAction::GuestAdopted,
            AuthOutcome::Succeeded,
            &EventFields {
                user_id: Some(&user.id),
                count: Some(result.scenario_ids.len() as u64),
                ..Default::default()
            },
        );
    }

    Ok(Json(result))
}

/// Point `graph` at `user`'s library and save it as a new scenario. Each
/// assumption the library already holds, by name and by value, is used as it
/// is (`finplan_plan::adopt`), so moving a plan back and forth does not grow
/// the library; the rest are added as rows of their own (and to `library`),
/// named `"<name> [<tag>]"` only where the name is taken. A same-named row
/// with different numbers is never substituted.
pub(crate) async fn restore_graph(
    tx: &mut Transaction<'_, Sqlite>,
    library: &mut Library,
    graph: &mut ScenarioGraph,
    user: &str,
    name: &str,
) -> ApiResult<i64> {
    let suffix = uuid::Uuid::new_v4().to_string()[..8].to_owned();
    let adoption = adopt(graph, library);
    let needed = adoption.new_distributions(graph);
    let mut distribution_ids = HashMap::new();
    let mut remaining: Vec<_> = graph
        .distributions
        .values()
        .filter(|row| needed.contains(&row.id))
        .cloned()
        .collect();
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
            let bull_id = row.bull_id.map(|id| distribution_ids[&id]);
            let bear_id = row.bear_id.map(|id| distribution_ids[&id]);
            let id: i64 = sqlx::query_scalar("INSERT INTO distributions(user_id,kind,rate,mean,std_dev,scale,df,bull_id,bear_id,bull_to_bear_prob,bear_to_bull_prob,history_preset,block_size) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) RETURNING id")
                .bind(user).bind(&row.kind).bind(row.rate).bind(row.mean).bind(row.std_dev).bind(row.scale).bind(row.df)
                .bind(bull_id).bind(bear_id)
                .bind(row.bull_to_bear_prob).bind(row.bear_to_bull_prob).bind(&row.history_preset).bind(row.block_size)
                .fetch_one(&mut **tx).await?;
            distribution_ids.insert(row.id, id);
            library.distributions.push(DistributionRow {
                id,
                bull_id,
                bear_id,
                ..row
            });
        }
        if deferred.len() == before {
            return Err(ApiError::bad_request("Invalid distribution references."));
        }
        remaining = deferred;
    }
    let mut profile_ids = adoption.return_profiles.clone();
    let mut profiles: Vec<_> = graph
        .return_profiles
        .values()
        .filter(|p| !profile_ids.contains_key(&p.id))
        .collect();
    profiles.sort_by_key(|p| p.id);
    for profile in profiles {
        let distribution = *distribution_ids
            .get(&profile.distribution_id)
            .ok_or_else(|| ApiError::bad_request("Missing profile distribution."))?;
        let name = import_name(
            &profile.name,
            &format!("{suffix}:{}", profile.id),
            library.return_profiles.iter().map(|p| p.name.as_str()),
        );
        let id:i64 = sqlx::query_scalar("INSERT INTO return_profiles(user_id,name,description,distribution_id,asset_class) VALUES(?,?,?,?,?) RETURNING id")
            .bind(user).bind(&name).bind(&profile.description).bind(distribution).bind(&profile.asset_class)
            .fetch_one(&mut **tx).await?;
        profile_ids.insert(profile.id, id);
        library.return_profiles.push(LibraryReturnProfile {
            id,
            name,
            description: profile.description.clone(),
            asset_class: profile.asset_class.clone(),
            distribution_id: distribution,
            sort_order: 0,
        });
    }
    graph.scenario.inflation_profile_id = match graph.inflation_distribution_id {
        Some(_) if adoption.inflation_profile.is_some() => adoption.inflation_profile,
        Some(distribution) => {
            let distribution = *distribution_ids
                .get(&distribution)
                .ok_or_else(|| ApiError::bad_request("Missing inflation distribution."))?;
            let name = import_name(
                graph
                    .inflation_profile_name
                    .as_deref()
                    .unwrap_or(IMPORTED_INFLATION),
                &suffix,
                library.inflation_profiles.iter().map(|p| p.name.as_str()),
            );
            let description = "Restored input assumptions";
            let id: i64 = sqlx::query_scalar("INSERT INTO inflation_profiles(user_id,name,description,distribution_id) VALUES(?,?,?,?) RETURNING id")
                .bind(user).bind(&name).bind(description).bind(distribution).fetch_one(&mut **tx).await?;
            library.inflation_profiles.push(LibraryInflationProfile {
                id,
                name,
                description: Some(description.to_string()),
                distribution_id: distribution,
                sort_order: 0,
            });
            Some(id)
        }
        None => None,
    };
    graph.scenario.tax_config_id = match &graph.tax_config {
        Some(_) if adoption.tax_config.is_some() => adoption.tax_config,
        Some(tax) => {
            let name = import_name(
                &tax.name,
                &suffix,
                library.tax_configs.iter().map(|t| t.name.as_str()),
            );
            let id:i64 = sqlx::query_scalar("INSERT INTO tax_configs(user_id,name,state_rate,capital_gains_rate,early_withdrawal_penalty_rate,standard_deduction,age_65_extra_deduction) VALUES(?,?,?,?,?,?,?) RETURNING id")
                .bind(user).bind(&name).bind(tax.state_rate).bind(tax.capital_gains_rate).bind(tax.early_withdrawal_penalty_rate).bind(tax.standard_deduction).bind(tax.age_65_extra_deduction).fetch_one(&mut **tx).await?;
            for bracket in &graph.tax_brackets {
                sqlx::query("INSERT INTO tax_brackets(tax_config_id,threshold,rate) VALUES(?,?,?)")
                    .bind(id)
                    .bind(bracket.threshold)
                    .bind(bracket.rate)
                    .execute(&mut **tx)
                    .await?;
            }
            library.tax_configs.push(LibraryTaxConfig {
                id,
                name,
                description: None,
                state_rate: tax.state_rate,
                capital_gains_rate: tax.capital_gains_rate,
                early_withdrawal_penalty_rate: tax.early_withdrawal_penalty_rate,
                standard_deduction: tax.standard_deduction,
                age_65_extra_deduction: tax.age_65_extra_deduction,
                federal_brackets: graph
                    .tax_brackets
                    .iter()
                    .map(|b| Bracket {
                        threshold: b.threshold,
                        rate: b.rate,
                    })
                    .collect(),
            });
            Some(id)
        }
        None => None,
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
