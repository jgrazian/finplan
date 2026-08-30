//! Return and inflation profiles: the user's reusable market-assumption library.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};

use crate::auth::session::CurrentUser;
use crate::compile::HISTORY_PRESETS;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::state::AppState;
use ts_rs::TS;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/return-profiles", get(list_return).post(create_return))
        .route(
            "/return-profiles/{id}",
            get(fetch_return).patch(update_return).delete(delete_return),
        )
        .route(
            "/inflation-profiles",
            get(list_inflation).post(create_inflation),
        )
        .route(
            "/inflation-profiles/{id}",
            axum::routing::delete(delete_inflation),
        )
        .route("/history-presets", get(list_presets))
}

/// The distribution shapes a profile can take. `RegimeSwitching` nests two more
/// distributions, so this mirrors the recursive Rust enum.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum DistributionSpec {
    None,
    Fixed {
        rate: f64,
    },
    Normal {
        mean: f64,
        std_dev: f64,
    },
    LogNormal {
        mean: f64,
        std_dev: f64,
    },
    StudentT {
        mean: f64,
        scale: f64,
        df: f64,
    },
    RegimeSwitching {
        bull: Box<DistributionSpec>,
        bear: Box<DistributionSpec>,
        bull_to_bear_prob: f64,
        bear_to_bull_prob: f64,
    },
    Bootstrap {
        preset: String,
        #[serde(default)]
        block_size: Option<i64>,
    },
}

impl DistributionSpec {
    fn insert<'a>(
        &'a self,
        tx: &'a mut Transaction<'_, Sqlite>,
        user_id: &'a str,
        depth: usize,
    ) -> std::pin::Pin<Box<dyn Future<Output = ApiResult<i64>> + Send + 'a>> {
        Box::pin(async move {
            if depth > 8 {
                return Err(ApiError::bad_request(
                    "distribution nests too deeply; regime models may not be recursive beyond 8 levels",
                ));
            }

            let (bull_id, bear_id) = match self {
                DistributionSpec::RegimeSwitching { bull, bear, .. } => (
                    Some(bull.insert(tx, user_id, depth + 1).await?),
                    Some(bear.insert(tx, user_id, depth + 1).await?),
                ),
                _ => (None, None),
            };

            let (kind, rate, mean, std_dev, scale, df, btb, btbull, preset, block) = match self {
                DistributionSpec::None => {
                    ("None", None, None, None, None, None, None, None, None, None)
                }
                DistributionSpec::Fixed { rate } => (
                    "Fixed",
                    Some(*rate),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                DistributionSpec::Normal { mean, std_dev } => (
                    "Normal",
                    None,
                    Some(*mean),
                    Some(*std_dev),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                DistributionSpec::LogNormal { mean, std_dev } => (
                    "LogNormal",
                    None,
                    Some(*mean),
                    Some(*std_dev),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                DistributionSpec::StudentT { mean, scale, df } => (
                    "StudentT",
                    None,
                    Some(*mean),
                    None,
                    Some(*scale),
                    Some(*df),
                    None,
                    None,
                    None,
                    None,
                ),
                DistributionSpec::RegimeSwitching {
                    bull_to_bear_prob,
                    bear_to_bull_prob,
                    ..
                } => (
                    "RegimeSwitching",
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(*bull_to_bear_prob),
                    Some(*bear_to_bull_prob),
                    None,
                    None,
                ),
                DistributionSpec::Bootstrap { preset, block_size } => {
                    if !HISTORY_PRESETS.contains(&preset.as_str()) {
                        return Err(ApiError::bad_request(format!(
                            "unknown history preset '{preset}'; expected one of {}",
                            HISTORY_PRESETS.join(", ")
                        )));
                    }
                    (
                        "Bootstrap",
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        Some(preset.as_str()),
                        *block_size,
                    )
                }
            };

            let id: i64 = sqlx::query_scalar(
                "INSERT INTO distributions
                    (user_id, kind, rate, mean, std_dev, scale, df, bull_id, bear_id,
                     bull_to_bear_prob, bear_to_bull_prob, history_preset, block_size)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) RETURNING id",
            )
            .bind(user_id)
            .bind(kind)
            .bind(rate)
            .bind(mean)
            .bind(std_dev)
            .bind(scale)
            .bind(df)
            .bind(bull_id)
            .bind(bear_id)
            .bind(btb)
            .bind(btbull)
            .bind(preset)
            .bind(block)
            .fetch_one(&mut **tx)
            .await?;

            Ok(id)
        })
    }
}

/// One `distributions` row, as read back for reassembly.
#[derive(Debug, sqlx::FromRow)]
struct DistRow {
    kind: String,
    rate: Option<f64>,
    mean: Option<f64>,
    std_dev: Option<f64>,
    scale: Option<f64>,
    df: Option<f64>,
    bull_id: Option<i64>,
    bear_id: Option<i64>,
    bull_to_bear_prob: Option<f64>,
    bear_to_bull_prob: Option<f64>,
    history_preset: Option<String>,
    block_size: Option<i64>,
}

/// Rebuild a nested `DistributionSpec` from its rows.
async fn load_distribution(state: &AppState, id: i64, depth: usize) -> ApiResult<DistributionSpec> {
    if depth > 8 {
        return Err(ApiError::internal("distribution graph is cyclic"));
    }

    let row: Option<DistRow> = sqlx::query_as(
        "SELECT kind, rate, mean, std_dev, scale, df, bull_id, bear_id,
                bull_to_bear_prob, bear_to_bull_prob, history_preset, block_size
           FROM distributions WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    let DistRow {
        kind,
        rate,
        mean,
        std_dev,
        scale,
        df,
        bull_id,
        bear_id,
        bull_to_bear_prob: btb,
        bear_to_bull_prob: btbull,
        history_preset: preset,
        block_size: block,
    } = row.ok_or(ApiError::NotFound("distribution"))?;

    Ok(match kind.as_str() {
        "Fixed" => DistributionSpec::Fixed {
            rate: rate.unwrap_or_default(),
        },
        "Normal" => DistributionSpec::Normal {
            mean: mean.unwrap_or_default(),
            std_dev: std_dev.unwrap_or_default(),
        },
        "LogNormal" => DistributionSpec::LogNormal {
            mean: mean.unwrap_or_default(),
            std_dev: std_dev.unwrap_or_default(),
        },
        "StudentT" => DistributionSpec::StudentT {
            mean: mean.unwrap_or_default(),
            scale: scale.unwrap_or_default(),
            df: df.unwrap_or(5.0),
        },
        "RegimeSwitching" => DistributionSpec::RegimeSwitching {
            bull: Box::new(
                Box::pin(load_distribution(
                    state,
                    bull_id.unwrap_or_default(),
                    depth + 1,
                ))
                .await?,
            ),
            bear: Box::new(
                Box::pin(load_distribution(
                    state,
                    bear_id.unwrap_or_default(),
                    depth + 1,
                ))
                .await?,
            ),
            bull_to_bear_prob: btb.unwrap_or_default(),
            bear_to_bull_prob: btbull.unwrap_or_default(),
        },
        "Bootstrap" => DistributionSpec::Bootstrap {
            preset: preset.unwrap_or_else(|| "sp500".to_string()),
            block_size: block,
        },
        _ => DistributionSpec::None,
    })
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub distribution: DistributionSpec,
    /// Names of assets and accounts pointing at this profile. Always present,
    /// empty when nothing references it: an omitted key would make the
    /// generated TypeScript claim a field the wire format does not carry.
    pub used_by: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateProfile {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub distribution: DistributionSpec,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateProfile {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub distribution: Option<DistributionSpec>,
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
    let rows: Vec<(i64, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, name, description, distribution_id
           FROM return_profiles WHERE user_id = ?1 ORDER BY name",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for (id, name, description, distribution_id) in rows {
        out.push(Profile {
            id,
            name,
            description,
            distribution: load_distribution(&state, distribution_id, 0).await?,
            used_by: references(&state, id).await?,
        });
    }
    Ok(Json(out))
}

async fn fetch_return(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Profile>> {
    let row: Option<(i64, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, name, description, distribution_id
           FROM return_profiles WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;

    let (id, name, description, distribution_id) =
        row.ok_or(ApiError::NotFound("return profile"))?;

    Ok(Json(Profile {
        id,
        name,
        description,
        distribution: load_distribution(&state, distribution_id, 0).await?,
        used_by: references(&state, id).await?,
    }))
}

async fn create_return(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<CreateProfile>,
) -> ApiResult<(StatusCode, Json<Profile>)> {
    let mut tx = state.db.begin().await?;
    let distribution_id = body.distribution.insert(&mut tx, &user.id, 0).await?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO return_profiles (user_id, name, description, distribution_id)
         VALUES (?1,?2,?3,?4) RETURNING id",
    )
    .bind(&user.id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(distribution_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "a return profile with that name already exists"))?;

    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(Profile {
            id,
            name: body.name.trim().to_string(),
            description: body.description,
            distribution: body.distribution,
            used_by: Vec::new(),
        }),
    ))
}

async fn update_return(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateProfile>,
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
        Some(spec) => spec.insert(&mut tx, &user.id, 0).await?,
        None => old_distribution,
    };

    sqlx::query(
        "UPDATE return_profiles SET
            name            = COALESCE(?3, name),
            description     = COALESCE(?4, description),
            distribution_id = ?5,
            updated_at      = datetime('now')
          WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(&user.id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(&body.description)
    .bind(distribution_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "a return profile with that name already exists"))?;

    if distribution_id != old_distribution {
        // Safe now that nothing points at it; a shared row would be blocked by
        // the foreign key, which is the desired outcome.
        let _ = sqlx::query("DELETE FROM distributions WHERE id = ?1")
            .bind(old_distribution)
            .execute(&mut *tx)
            .await;
    }

    tx.commit().await?;
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
    Ok(StatusCode::NO_CONTENT)
}

// ── inflation profiles ──────────────────────────────────────────────────────

/// `InflationProfile` has no StudentT or RegimeSwitching variant, so reject
/// those here rather than at compile time.
fn check_inflation_kind(spec: &DistributionSpec) -> ApiResult<()> {
    match spec {
        DistributionSpec::StudentT { .. } | DistributionSpec::RegimeSwitching { .. } => {
            Err(ApiError::bad_request(
                "inflation profiles support None, Fixed, Normal, LogNormal or Bootstrap",
            ))
        }
        _ => Ok(()),
    }
}

async fn list_inflation(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Vec<Profile>>> {
    let rows: Vec<(i64, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, name, description, distribution_id
           FROM inflation_profiles WHERE user_id = ?1 ORDER BY name",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for (id, name, description, distribution_id) in rows {
        out.push(Profile {
            id,
            name,
            description,
            distribution: load_distribution(&state, distribution_id, 0).await?,
            used_by: Vec::new(),
        });
    }
    Ok(Json(out))
}

async fn create_inflation(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<CreateProfile>,
) -> ApiResult<(StatusCode, Json<Profile>)> {
    check_inflation_kind(&body.distribution)?;

    let mut tx = state.db.begin().await?;
    let distribution_id = body.distribution.insert(&mut tx, &user.id, 0).await?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO inflation_profiles (user_id, name, description, distribution_id)
         VALUES (?1,?2,?3,?4) RETURNING id",
    )
    .bind(&user.id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(distribution_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "an inflation profile with that name already exists"))?;

    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(Profile {
            id,
            name: body.name.trim().to_string(),
            description: body.description,
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
    let affected = sqlx::query("DELETE FROM inflation_profiles WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(&user.id)
        .execute(&state.db)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("inflation profile"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_presets() -> Json<&'static [&'static str]> {
    Json(HISTORY_PRESETS)
}
