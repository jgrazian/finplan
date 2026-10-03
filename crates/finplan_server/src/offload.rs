//! Server offload (spec 19): the monthly budget and the `compute_jobs` table.
//!
//! A local plan's run can be sent here when the device is too slow for it. The
//! plan itself is never stored; what is kept is the job's progress and, for an
//! hour after it ends, its results. Spend lives in `monthly_offload_spend`,
//! apart from the jobs, so deleting or purging a job does not give the budget
//! back.

use crate::billing::Entitlements;
use crate::config::ServerConfig;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};

/// How long a finished job's results stay readable.
pub const RESULT_TTL: &str = "+1 hour";

/// Cost units a user's tier may offload per calendar month (UTC).
#[must_use]
pub fn monthly_budget(entitlements: &Entitlements, config: &ServerConfig) -> i64 {
    if entitlements.guest {
        0
    } else if entitlements.pro {
        config.offload.budget_pro
    } else {
        config.offload.budget_free
    }
}

/// Where a user stands this month.
pub struct Standing {
    pub used: i64,
    /// First instant of next month, UTC, as RFC 3339.
    pub resets_at: String,
}

pub async fn standing(db: &Db, user: &str) -> ApiResult<Standing> {
    let (used, resets_at): (i64, String) = sqlx::query_as(
        "SELECT COALESCE((SELECT used FROM monthly_offload_spend
                           WHERE user_id = ?1 AND month = strftime('%Y-%m', 'now')), 0),
                strftime('%Y-%m-%dT00:00:00Z', 'now', 'start of month', '+1 month')",
    )
    .bind(user)
    .fetch_one(db)
    .await?;
    Ok(Standing { used, resets_at })
}

/// Charge `cost` to this month's budget, or refuse with a 403 that says why and
/// when the budget resets. Returns the month charged, for [`refund`].
///
/// The check and the charge are one statement, so two submissions racing for
/// the last of the budget cannot both get it.
pub async fn reserve(db: &Db, user: &str, cost: i64, monthly: i64) -> ApiResult<String> {
    if cost <= monthly {
        let month: Option<String> = sqlx::query_scalar(
            "INSERT INTO monthly_offload_spend (user_id, month, used)
             VALUES (?1, strftime('%Y-%m', 'now'), ?2)
             ON CONFLICT (user_id, month) DO UPDATE SET used = used + ?2
              WHERE used + ?2 <= ?3
             RETURNING month",
        )
        .bind(user)
        .bind(cost)
        .bind(monthly)
        .fetch_optional(db)
        .await?;
        if let Some(month) = month {
            return Ok(month);
        }
    }
    let standing = standing(db, user).await?;
    let reset = standing.resets_at.get(..10).unwrap_or(&standing.resets_at);
    Err(if cost > monthly {
        ApiError::Coded(
            axum::http::StatusCode::FORBIDDEN,
            "offload_run_exceeds_budget",
            format!(
                "This run is larger than your monthly server-offload budget ({monthly} units). \
                 Reduce iterations or plan size."
            ),
        )
    } else {
        ApiError::Coded(
            axum::http::StatusCode::FORBIDDEN,
            "offload_budget_spent",
            format!(
                "Your monthly server-offload budget is spent ({} of {monthly} units used). \
                 It resets on {reset} (UTC).",
                standing.used
            ),
        )
    })
}

/// Give a charge back: the job never ran, or the server failed it.
pub async fn refund(db: &Db, user: &str, month: &str, cost: i64) {
    let result = sqlx::query(
        "UPDATE monthly_offload_spend SET used = MAX(used - ?3, 0)
          WHERE user_id = ?1 AND month = ?2",
    )
    .bind(user)
    .bind(month)
    .bind(cost)
    .execute(db)
    .await;
    if result.is_err() {
        tracing::warn!(event = "offload.refund_failed", error_class = "database");
    }
}

/// Delete jobs whose results have expired. Run by the maintenance loop.
pub async fn purge_expired(db: &Db) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "DELETE FROM compute_jobs
          WHERE expires_at IS NOT NULL AND expires_at <= datetime('now')",
    )
    .execute(db)
    .await?
    .rows_affected())
}

/// A job's plan lives only in the process that accepted it, so a job still
/// queued or running when the process ends cannot be resumed. Fail it, and give
/// its charge back: a restart is not the user's doing.
pub async fn fail_orphans(db: &Db) -> Result<u64, sqlx::Error> {
    let orphans: Vec<(String, String, i64)> = sqlx::query_as(
        "UPDATE compute_jobs
            SET status = 'failed',
                error = 'The server restarted before this run finished. Run it again.',
                finished_at = datetime('now'),
                expires_at = datetime('now', ?1)
          WHERE status IN ('queued', 'running')
      RETURNING user_id, month, cost",
    )
    .bind(RESULT_TTL)
    .fetch_all(db)
    .await?;
    for (user, month, cost) in &orphans {
        refund(db, user, month, *cost).await;
    }
    Ok(orphans.len() as u64)
}
