//! Return and inflation profiles: the user's reusable market-assumption library.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::{Sqlite, Transaction};

use super::ReorderRequest;
use crate::auth::activity::{ActivityFields, Submitted};
use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;
use finplan_plan::graph::DistributionRow;
use finplan_plan::read;
use std::collections::HashMap;

use finplan_plan::specs::profiles::NAME_TAKEN;
use finplan_plan::specs::profiles::{
    AssetClass, CreateProfile, DistributionSpec, HistoryPreset, Profile, UpdateProfile,
    check_inflation_kind,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/return-profiles", get(list_return).post(create_return))
        .route("/return-profiles/reorder", post(reorder_return))
        .route(
            "/return-profiles/{id}",
            get(fetch_return).patch(update_return).delete(delete_return),
        )
        .route(
            "/inflation-profiles",
            get(list_inflation).post(create_inflation),
        )
        .route("/inflation-profiles/reorder", post(reorder_inflation))
        .route(
            "/inflation-profiles/{id}",
            axum::routing::delete(delete_inflation),
        )
        .route("/history-presets", get(list_presets))
}
/// Write `spec` (and the regimes nested in it) to `distributions`, nested
/// rows first so the parent can point at them. Returns the new row's id.
///
/// What the row holds, and what it refuses, is [`DistributionSpec::columns`];
/// this is only the SQL.
fn insert_distribution<'a>(
    spec: &'a DistributionSpec,
    tx: &'a mut Transaction<'_, Sqlite>,
    user_id: &'a str,
    depth: usize,
) -> std::pin::Pin<Box<dyn Future<Output = ApiResult<i64>> + Send + 'a>> {
    Box::pin(async move {
        let columns = spec.columns(depth)?;

        let (bull_id, bear_id) = match columns.regimes {
            Some((bull, bear)) => (
                Some(insert_distribution(bull, tx, user_id, depth + 1).await?),
                Some(insert_distribution(bear, tx, user_id, depth + 1).await?),
            ),
            None => (None, None),
        };

        let id: i64 = sqlx::query_scalar(
            "INSERT INTO distributions
                (user_id, kind, rate, mean, std_dev, scale, df, bull_id, bear_id,
                 bull_to_bear_prob, bear_to_bull_prob, history_preset, block_size)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) RETURNING id",
        )
        .bind(user_id)
        .bind(columns.kind)
        .bind(columns.rate)
        .bind(columns.mean)
        .bind(columns.std_dev)
        .bind(columns.scale)
        .bind(columns.df)
        .bind(bull_id)
        .bind(bear_id)
        .bind(columns.bull_to_bear_prob)
        .bind(columns.bear_to_bull_prob)
        .bind(columns.history_preset)
        .bind(columns.block_size)
        .fetch_one(&mut **tx)
        .await?;

        Ok(id)
    })
}

/// One `return_profiles` row, before its distribution is assembled. Named
/// rather than a tuple because five columns is past where positions are
/// readable — and two handlers select exactly these.
#[derive(Debug, sqlx::FromRow)]
struct ProfileRow {
    id: i64,
    name: String,
    description: Option<String>,
    asset_class: Option<String>,
    distribution_id: i64,
}

/// Every distribution of the user's library, keyed by id, for reassembling
/// the nested [`DistributionSpec`] of a profile (`finplan_plan::read`).
async fn load_distributions(
    state: &AppState,
    user_id: &str,
) -> ApiResult<HashMap<i64, DistributionRow>> {
    let rows: Vec<DistributionRow> = sqlx::query_as(
        "SELECT id, kind, rate, mean, std_dev, scale, df, bull_id, bear_id,
                bull_to_bear_prob, bear_to_bull_prob, history_preset, block_size
           FROM distributions WHERE user_id = ?1",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|row| (row.id, row)).collect())
}

/// Every asset or account currently referencing a return profile.
async fn references(state: &AppState, profile_id: i64) -> ApiResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM assets WHERE return_profile_id = ?1
         UNION ALL
         SELECT a.name FROM account_bank b JOIN accounts a ON a.id = b.account_id
          WHERE b.return_profile_id = ?1
         UNION ALL
         SELECT a.name FROM account_investment i JOIN accounts a ON a.id = i.account_id
          WHERE i.cash_return_profile_id = ?1",
    )
    .bind(profile_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|(n,)| n).collect())
}

async fn list_return(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Vec<Profile>>> {
    let rows: Vec<ProfileRow> = sqlx::query_as(
        "SELECT id, name, description, asset_class, distribution_id
           FROM return_profiles WHERE user_id = ?1 ORDER BY sort_order, name",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;

    let distributions = load_distributions(&state, &user.id).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(Profile {
            id: row.id,
            name: row.name,
            description: row.description,
            asset_class: row.asset_class.as_deref().and_then(AssetClass::parse),
            distribution: read::distribution(&distributions, row.distribution_id)?,
            used_by: references(&state, row.id).await?,
        });
    }
    Ok(Json(out))
}

/// Put the user's return-profile library in the order the body names.
///
/// The library is the user's, not a scenario's: dragging a profile here moves
/// it for every plan that points at one, which is what a library is.
async fn reorder_return(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<StatusCode> {
    let current: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM return_profiles WHERE user_id = ?1 ORDER BY sort_order, name",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;

    let affected = super::apply_order(&state.db, "return_profiles", &current, &body.ids).await?;

    if affected > 0 {
        state.telemetry.mutation(
            Resource::ReturnProfile,
            Operation::Reordered,
            &EventFields {
                user_id: Some(&user.id),
                fields: &["ids"],
                count: Some(affected),
                ..Default::default()
            },
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_return(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Profile>> {
    let row: Option<ProfileRow> = sqlx::query_as(
        "SELECT id, name, description, asset_class, distribution_id
           FROM return_profiles WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;

    let row = row.ok_or(ApiError::NotFound("return profile"))?;

    let distributions = load_distributions(&state, &user.id).await?;
    Ok(Json(Profile {
        id: row.id,
        name: row.name,
        description: row.description,
        asset_class: row.asset_class.as_deref().and_then(AssetClass::parse),
        distribution: read::distribution(&distributions, row.distribution_id)?,
        used_by: references(&state, row.id).await?,
    }))
}

async fn create_return(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(Submitted { body, fields }): Json<Submitted<CreateProfile>>,
) -> ApiResult<(StatusCode, Json<Profile>)> {
    let mut tx = state.db.begin().await?;
    let id = create_return_in(&mut tx, &user.id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::ReturnProfile,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );

    Ok((
        StatusCode::CREATED,
        Json(Profile {
            id,
            name: body.name.trim().to_string(),
            description: body.description,
            asset_class: body.asset_class,
            distribution: body.distribution,
            used_by: Vec::new(),
        }),
    ))
}

/// Insert a return profile and its distribution, as `POST /return-profiles`
/// does; returns its id. The suggestion path writes through this too, inside
/// its transaction.
pub(crate) async fn create_return_in(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    body: &CreateProfile,
) -> ApiResult<i64> {
    body.distribution.validate(0)?;
    let distribution_id = insert_distribution(&body.distribution, tx, user_id, 0).await?;

    sqlx::query_scalar(
        "INSERT INTO return_profiles
            (user_id, name, description, asset_class, distribution_id, sort_order)
         VALUES (?1,?2,?3,?4,?5,
                 (SELECT COALESCE(MAX(sort_order), -1) + 1
                    FROM return_profiles WHERE user_id = ?1))
         RETURNING id",
    )
    .bind(user_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(body.asset_class.map(AssetClass::as_str))
    .bind(distribution_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))
}

async fn update_return(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(Submitted { body, fields }): Json<Submitted<UpdateProfile>>,
) -> ApiResult<Json<Profile>> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT distribution_id FROM return_profiles WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;
    let old_distribution = existing.ok_or(ApiError::NotFound("return profile"))?;

    let mut tx = state.db.begin().await?;

    // A new distribution is inserted and swapped in rather than updated in
    // place, so the old row stays intact for anything mid-flight reading it.
    let distribution_id = match &body.distribution {
        Some(spec) => insert_distribution(spec, &mut tx, &user.id, 0).await?,
        None => old_distribution,
    };

    // `Some(None)` is a deliberate unclassify, `None` is silence about the class.
    let reclassify = body.asset_class.is_some();
    let asset_class = body.asset_class.flatten().map(AssetClass::as_str);

    let affected = sqlx::query(
        "UPDATE return_profiles SET
            name            = COALESCE(?3, name),
            description     = COALESCE(?4, description),
            asset_class     = CASE WHEN ?7 THEN ?6 ELSE asset_class END,
            distribution_id = ?5,
            updated_at      = datetime('now')
          WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(&user.id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(&body.description)
    .bind(distribution_id)
    .bind(asset_class)
    .bind(reclassify)
    .execute(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("return profile"));
    }
    if distribution_id != old_distribution {
        // Safe now that nothing points at it; a shared row would be blocked by
        // the foreign key, which is the desired outcome.
        let _ = sqlx::query("DELETE FROM distributions WHERE id = ?1")
            .bind(old_distribution)
            .execute(&mut *tx)
            .await;
    }

    // Invalidate every plan using the shared assumption in the same transaction.
    sqlx::query(
        "UPDATE scenarios SET updated_at = datetime('now')
         WHERE user_id = ?2 AND id IN (
           SELECT scenario_id FROM assets WHERE return_profile_id = ?1
           UNION SELECT a.scenario_id FROM accounts a JOIN account_bank b ON b.account_id = a.id
             WHERE b.return_profile_id = ?1
           UNION SELECT a.scenario_id FROM accounts a JOIN account_investment i ON i.account_id = a.id
             WHERE i.cash_return_profile_id = ?1)",
    )
    .bind(id)
    .bind(&user.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    state.telemetry.mutation(
        Resource::ReturnProfile,
        Operation::Updated,
        &EventFields {
            user_id: Some(&user.id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );

    fetch_return(State(state), user, Path(id)).await
}

async fn delete_return(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let used = references(&state, id).await?;
    if !used.is_empty() {
        return Err(ApiError::Conflict(format!(
            "return profile is still used by: {}",
            used.join(", ")
        )));
    }

    let affected = sqlx::query("DELETE FROM return_profiles WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(&user.id)
        .execute(&state.db)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("return profile"));
    }

    state.telemetry.mutation(
        Resource::ReturnProfile,
        Operation::Deleted,
        &EventFields {
            user_id: Some(&user.id),
            resource_id: Some(id),
            ..Default::default()
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

// ── inflation profiles ──────────────────────────────────────────────────────

/// `InflationProfile` has no StudentT or RegimeSwitching variant, so reject
async fn list_inflation(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Vec<Profile>>> {
    let rows: Vec<(i64, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, name, description, distribution_id
           FROM inflation_profiles WHERE user_id = ?1 ORDER BY sort_order, name",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;

    let distributions = load_distributions(&state, &user.id).await?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, name, description, distribution_id) in rows {
        out.push(Profile {
            id,
            name,
            description,
            // Inflation is not a holding, so it has no class to be one of; the
            // two share a wire shape, not a meaning for every field of it.
            asset_class: None,
            distribution: read::distribution(&distributions, distribution_id)?,
            used_by: Vec::new(),
        });
    }
    Ok(Json(out))
}

/// Put the user's inflation-profile library in the order the body names.
async fn reorder_inflation(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<StatusCode> {
    let current: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM inflation_profiles WHERE user_id = ?1 ORDER BY sort_order, name",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;

    let affected = super::apply_order(&state.db, "inflation_profiles", &current, &body.ids).await?;

    if affected > 0 {
        state.telemetry.mutation(
            Resource::InflationProfile,
            Operation::Reordered,
            &EventFields {
                user_id: Some(&user.id),
                fields: &["ids"],
                count: Some(affected),
                ..Default::default()
            },
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn create_inflation(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(Submitted { body, fields }): Json<Submitted<CreateProfile>>,
) -> ApiResult<(StatusCode, Json<Profile>)> {
    check_inflation_kind(&body.distribution)?;

    let mut tx = state.db.begin().await?;
    let distribution_id = insert_distribution(&body.distribution, &mut tx, &user.id, 0).await?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO inflation_profiles (user_id, name, description, distribution_id, sort_order)
         VALUES (?1,?2,?3,?4,
                 (SELECT COALESCE(MAX(sort_order), -1) + 1
                    FROM inflation_profiles WHERE user_id = ?1))
         RETURNING id",
    )
    .bind(&user.id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(distribution_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "an inflation profile with that name already exists"))?;

    tx.commit().await?;

    state.telemetry.mutation(
        Resource::InflationProfile,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );

    Ok((
        StatusCode::CREATED,
        Json(Profile {
            id,
            name: body.name.trim().to_string(),
            description: body.description,
            asset_class: None,
            distribution: body.distribution,
            used_by: Vec::new(),
        }),
    ))
}

async fn delete_inflation(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE scenarios SET updated_at = datetime('now') WHERE inflation_profile_id = ?1 AND user_id = ?2")
        .bind(id).bind(&user.id).execute(&mut *tx).await?;
    let affected = sqlx::query("DELETE FROM inflation_profiles WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("inflation profile"));
    }
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::InflationProfile,
        Operation::Deleted,
        &EventFields {
            user_id: Some(&user.id),
            resource_id: Some(id),
            ..Default::default()
        },
    );

    Ok(StatusCode::NO_CONTENT)
}

async fn list_presets() -> Json<Vec<HistoryPreset>> {
    Json(finplan_plan::read::history_presets())
}

impl ActivityFields for CreateProfile {
    const FIELDS: &'static [&'static str] = &["name", "description", "asset_class", "distribution"];
}

impl ActivityFields for UpdateProfile {
    const FIELDS: &'static [&'static str] = &["name", "description", "asset_class", "distribution"];
}
