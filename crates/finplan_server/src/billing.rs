//! Provider-neutral access policy. Only trusted server adapters may reconcile
//! subscriptions: there is intentionally no browser endpoint granting paid access.
use crate::{
    auth::session::CurrentUser,
    config::{HostedAccessMode, ServerConfig},
    db::Db,
    error::{ApiError, ApiResult},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AccessMode {
    SelfHosted,
    Beta,
    Subscription,
}

impl AccessMode {
    fn from_config(config: &ServerConfig) -> Self {
        if !config.hosted {
            Self::SelfHosted
        } else if config.access_mode == HostedAccessMode::Beta {
            Self::Beta
        } else {
            Self::Subscription
        }
    }
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct Entitlements {
    /// Full planning capabilities, including beta and self-hosted access.
    pub pro: bool,
    pub access_mode: AccessMode,
    pub hosted: bool,
    pub goal_seeks_used_this_month: i64,
    pub max_iterations: usize,
    pub saved_plan_limit: Option<usize>,
    pub goal_seeks_per_month: Option<usize>,
    pub editable_scenario_id: Option<i64>,
    pub annual_price_usd: u32,
    pub monthly_price_usd: u32,
}
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/entitlements", get(get_entitlements))
        .route("/editable-plan", post(select_editable))
}
async fn get_entitlements(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Entitlements>> {
    Ok(Json(
        entitlements(&state.db, &user.id, &state.config).await?,
    ))
}
pub async fn entitlements(db: &Db, user: &str, config: &ServerConfig) -> ApiResult<Entitlements> {
    let access_mode = AccessMode::from_config(config);
    let pro = access_mode != AccessMode::Subscription || sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM subscriptions WHERE user_id = ? AND state IN ('active','canceling','past_due') AND access_until > unixepoch())").bind(user).fetch_one(db).await?;
    let editable = if pro {
        None
    } else {
        sqlx::query_scalar::<_, Option<i64>>("SELECT COALESCE((SELECT scenario_id FROM editable_plans WHERE user_id = ?1), (SELECT MIN(id) FROM scenarios WHERE user_id = ?1))").bind(user).fetch_one(db).await?
    };
    let used:i64=sqlx::query_scalar("SELECT COALESCE((SELECT used FROM monthly_goal_seeks WHERE user_id=? AND month=strftime('%Y-%m','now')),0)").bind(user).fetch_one(db).await?;
    Ok(Entitlements {
        pro,
        hosted: config.hosted,
        access_mode,
        goal_seeks_used_this_month: used,
        max_iterations: if !config.hosted {
            config.max_iterations
        } else {
            config.max_iterations.min(if pro { 50_000 } else { 1_000 })
        },
        saved_plan_limit: if pro { None } else { Some(1) },
        goal_seeks_per_month: if pro { None } else { Some(1) },
        editable_scenario_id: editable,
        annual_price_usd: 80,
        monthly_price_usd: 10,
    })
}
pub async fn require_pro(db: &Db, user: &str, config: &ServerConfig) -> ApiResult<()> {
    if entitlements(db, user, config).await?.pro {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "This action requires Pro. Your existing plans and exports remain available.".into(),
        ))
    }
}
pub async fn require_editable(
    db: &Db,
    user: &str,
    scenario: i64,
    config: &ServerConfig,
) -> ApiResult<()> {
    let e = entitlements(db, user, config).await?;
    if e.pro || e.editable_scenario_id == Some(scenario) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "Free includes one editable plan. Existing plans remain readable and exportable."
                .into(),
        ))
    }
}
/// Call once after request validation, before accepting a goal seek. Invalid
/// requests must never consume usage. A failed accepted job still counts.
pub async fn reserve_goal_seek(db: &Db, user: &str, config: &ServerConfig) -> ApiResult<()> {
    if entitlements(db, user, config).await?.pro {
        return Ok(());
    }
    let accepted = sqlx::query("INSERT INTO monthly_goal_seeks(user_id,month,used) VALUES (?,strftime('%Y-%m','now'),1) ON CONFLICT(user_id,month) DO UPDATE SET used=used+1 WHERE used < 1")
        .bind(user).execute(db).await?.rows_affected();
    if accepted == 0 {
        return Err(ApiError::Forbidden("Free includes one goal seek per calendar month (UTC). Try next month or upgrade to Pro.".into()));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionState {
    Active,
    Canceling,
    PastDue,
    Expired,
}
impl SubscriptionState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Canceling => "canceling",
            Self::PastDue => "past_due",
            Self::Expired => "expired",
        }
    }
}
/// A normalized authoritative provider snapshot, not a checkout redirect. The
/// adapter must authenticate the provider and fetch current state before calling.
#[derive(Clone, Debug)]
pub struct ProviderSnapshot {
    pub provider: String,
    pub event_id: String,
    pub user_id: String,
    pub subscription_id: String,
    pub state: SubscriptionState,
    pub access_until: i64,
    pub revision: i64,
}
/// Transactional event deduplication, monotonic reconciliation, ownership pinning.
/// Returns false for duplicate or obsolete events. Provider adapters remain gated
/// on selection and sandbox signature tests; no external billing is wired.
pub async fn reconcile(db: &Db, event: &ProviderSnapshot) -> ApiResult<bool> {
    if event.revision < 0
        || event.provider.is_empty()
        || event.event_id.is_empty()
        || event.subscription_id.is_empty()
    {
        return Err(ApiError::bad_request("invalid provider snapshot"));
    }
    let mut tx = db.begin().await?;
    let owner: Option<String> = sqlx::query_scalar(
        "SELECT user_id FROM subscriptions WHERE provider = ? AND subscription_id = ?",
    )
    .bind(&event.provider)
    .bind(&event.subscription_id)
    .fetch_optional(&mut *tx)
    .await?;
    if owner.is_some_and(|owner| owner != event.user_id) {
        return Err(ApiError::Forbidden(
            "subscription belongs to another account".into(),
        ));
    }
    let inserted =
        sqlx::query("INSERT OR IGNORE INTO billing_receipts(provider,event_id) VALUES (?,?)")
            .bind(&event.provider)
            .bind(&event.event_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if inserted == 0 {
        tracing::debug!(event = "billing.reconciled", user_id = %event.user_id, replay = true);
        return Ok(false);
    }
    let changed = sqlx::query("INSERT INTO subscriptions(user_id,provider,subscription_id,state,access_until,revision) VALUES (?,?,?,?,?,?) ON CONFLICT(user_id) DO UPDATE SET state=excluded.state, access_until=excluded.access_until, revision=excluded.revision WHERE subscriptions.provider=excluded.provider AND subscriptions.subscription_id=excluded.subscription_id AND subscriptions.revision < excluded.revision")
        .bind(&event.user_id).bind(&event.provider).bind(&event.subscription_id).bind(event.state.as_str()).bind(event.access_until).bind(event.revision).execute(&mut *tx).await?.rows_affected();
    tx.commit().await?;
    if changed == 1 {
        tracing::info!(event = "billing.reconciled", user_id = %event.user_id, state = event.state.as_str(), replay = false);
    } else {
        tracing::debug!(event = "billing.reconciled", user_id = %event.user_id, replay = true);
    }
    Ok(changed == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(hosted: bool) -> ServerConfig {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            config: ServerConfig,
        }
        let mut config = Cli::parse_from(["test"]).config;
        config.hosted = hosted;
        config
    }
    async fn fixture() -> Db {
        let db = crate::db::connect("sqlite::memory:", 1).await.unwrap();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES ('u','u@example.com','unused'),('v','v@example.com','unused')").execute(&db).await.unwrap();
        db
    }
    #[tokio::test]
    async fn sandbox_lifecycle_is_idempotent_ordered_and_preserves_ownership() {
        let db = fixture().await;
        let mut e = ProviderSnapshot {
            provider: "fake".into(),
            event_id: "first".into(),
            user_id: "u".into(),
            subscription_id: "sub".into(),
            state: SubscriptionState::Active,
            access_until: 4_000_000_000,
            revision: 1,
        };
        assert!(reconcile(&db, &e).await.unwrap());
        assert!(!reconcile(&db, &e).await.unwrap());
        assert!(entitlements(&db, "u", &config(true)).await.unwrap().pro);
        e.event_id = "cancel".into();
        e.revision = 2;
        e.state = SubscriptionState::Canceling;
        assert!(reconcile(&db, &e).await.unwrap());
        assert!(entitlements(&db, "u", &config(true)).await.unwrap().pro);
        e.event_id = "late".into();
        e.revision = 1;
        assert!(!reconcile(&db, &e).await.unwrap());
        e.event_id = "failed".into();
        e.revision = 3;
        e.state = SubscriptionState::PastDue;
        assert!(reconcile(&db, &e).await.unwrap());
        e.event_id = "expired".into();
        e.revision = 4;
        e.access_until = 0;
        e.state = SubscriptionState::Expired;
        assert!(reconcile(&db, &e).await.unwrap());
        assert!(!entitlements(&db, "u", &config(true)).await.unwrap().pro);
        e.user_id = "v".into();
        e.event_id = "steal".into();
        e.revision = 5;
        assert!(reconcile(&db, &e).await.is_err());
        assert!(entitlements(&db, "u", &config(false)).await.unwrap().pro);
    }
    #[tokio::test]
    async fn access_modes_do_not_create_subscription_state() {
        let db = fixture().await;
        let mut config = config(true);
        let free = entitlements(&db, "u", &config).await.unwrap();
        assert_eq!(free.access_mode, AccessMode::Subscription);
        assert!(!free.pro);
        assert_eq!(free.saved_plan_limit, Some(1));
        assert_eq!(free.goal_seeks_per_month, Some(1));
        assert_eq!(free.max_iterations, 1_000);
        config.access_mode = HostedAccessMode::Beta;
        let beta = entitlements(&db, "u", &config).await.unwrap();
        assert_eq!(beta.access_mode, AccessMode::Beta);
        assert!(beta.pro);
        config.hosted = false;
        config.max_iterations = 100_000;
        let local = entitlements(&db, "u", &config).await.unwrap();
        assert_eq!(local.access_mode, AccessMode::SelfHosted);
        assert!(local.pro);
        assert_eq!(local.max_iterations, 100_000);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM subscriptions")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn monthly_goal_seek_reservation_is_atomic() {
        let db = fixture().await;
        let hosted = config(true);
        let (a, b) = tokio::join!(
            reserve_goal_seek(&db, "u", &hosted),
            reserve_goal_seek(&db, "u", &hosted)
        );
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
        assert!(reserve_goal_seek(&db, "u", &config(false)).await.is_ok());
    }
}

#[derive(Deserialize, ts_rs::TS)]
#[ts(export)]
struct EditablePlan {
    scenario_id: i64,
}
async fn select_editable(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<EditablePlan>,
) -> ApiResult<Json<Entitlements>> {
    let updated=sqlx::query("INSERT INTO editable_plans(user_id,scenario_id) SELECT ?1,id FROM scenarios WHERE id=?2 AND user_id=?1 ON CONFLICT(user_id) DO UPDATE SET scenario_id=excluded.scenario_id")
        .bind(&user.id).bind(body.scenario_id).execute(&state.db).await?.rows_affected();
    if updated == 0 {
        return Err(ApiError::NotFound("scenario"));
    }
    get_entitlements(State(state), user).await
}

/// Enforce edits centrally so nested endpoints cannot accidentally bypass the
/// selected Free plan. Reads, export, validation and deleting a whole plan stay
/// available after downgrade. Referenced library definitions are guarded too.
pub async fn mutation_entitlements(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::extract::FromRequestParts;
    use axum::http::Method;
    use axum::response::IntoResponse;
    if !state.config.hosted || matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
    {
        return next.run(req).await;
    }
    let path = req.uri().path().trim_start_matches("/api/").to_string();
    let segments: Vec<_> = path.split('/').collect();
    let scenario = segments.first() == Some(&"scenarios")
        && segments
            .get(1)
            .and_then(|s| s.parse::<i64>().ok())
            .is_some();
    let library = matches!(
        segments.first().copied(),
        Some("return-profiles" | "inflation-profiles" | "tax-configs")
    ) && segments
        .get(1)
        .and_then(|s| s.parse::<i64>().ok())
        .is_some();
    if !scenario && !library {
        return next.run(req).await;
    }
    if scenario
        && ((segments.len() == 2 && req.method() == Method::DELETE)
            || matches!(
                segments.get(2).copied(),
                Some("compile" | "preflight" | "archive")
            ))
    {
        return next.run(req).await;
    }
    let (mut parts, body) = req.into_parts();
    let user = match CurrentUser::from_request_parts(&mut parts, &state).await {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    let check = async {
        if scenario {
            let id=segments[1].parse::<i64>().unwrap();
            crate::api::owned_scenario(&state.db,id,&user.id).await?;
            require_editable(&state.db,&user.id,id,&state.config).await?;
        } else {
            let e=entitlements(&state.db,&user.id,&state.config).await?;
            if !e.pro {
                let id=segments[1].parse::<i64>().unwrap();
                let sql=match segments[0] {
                    "tax-configs"=>"SELECT EXISTS(SELECT 1 FROM scenarios WHERE user_id=?1 AND id<>COALESCE(?2,-1) AND tax_config_id=?3)",
                    "inflation-profiles"=>"SELECT EXISTS(SELECT 1 FROM scenarios WHERE user_id=?1 AND id<>COALESCE(?2,-1) AND inflation_profile_id=?3)",
                    _=>"SELECT EXISTS(SELECT 1 FROM scenarios s WHERE s.user_id=?1 AND s.id<>COALESCE(?2,-1) AND (EXISTS(SELECT 1 FROM assets a WHERE a.scenario_id=s.id AND a.return_profile_id=?3) OR EXISTS(SELECT 1 FROM accounts a JOIN account_bank c ON c.account_id=a.id WHERE a.scenario_id=s.id AND c.return_profile_id=?3) OR EXISTS(SELECT 1 FROM accounts a JOIN account_investment i ON i.account_id=a.id WHERE a.scenario_id=s.id AND i.cash_return_profile_id=?3)))",
                };
                let locked:bool=sqlx::query_scalar(sql).bind(&user.id).bind(e.editable_scenario_id).bind(id).fetch_one(&state.db).await?;
                if locked { return Err(ApiError::Forbidden("This definition is used by a read-only plan. Create a separate definition for your editable plan.".into())); }
            }
        }
        Ok::<_,ApiError>(())
    }.await;
    if let Err(e) = check {
        return e.into_response();
    }
    next.run(axum::extract::Request::from_parts(parts, body))
        .await
}

#[derive(Default)]
struct ComputeCounts {
    total: usize,
    users: std::collections::HashMap<String, usize>,
}
static COMPUTE: std::sync::OnceLock<std::sync::Mutex<ComputeCounts>> = std::sync::OnceLock::new();
/// Counts queued and running work together; releasing a worker permit alone is
/// insufficient to bound retained jobs. Hold this through the job's terminal state.
pub struct ComputePermit {
    user: String,
}
pub const COMPUTE_LIMIT: usize = 16;

/// Snapshot the existing process-wide admission gate; no shadow permit count.
pub fn compute_admitted() -> usize {
    COMPUTE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .total
}

pub fn admit_compute(user: &str) -> ApiResult<ComputePermit> {
    admit_compute_inner(user).map_err(|_| capacity_error())
}

pub fn admit_compute_observed(
    user: &str,
    telemetry: &crate::observability::Telemetry,
    origin: crate::observability::Origin,
) -> ApiResult<ComputePermit> {
    admit_compute_inner(user).map_err(|reason| {
        telemetry.rejection(reason, origin);
        capacity_error()
    })
}

fn capacity_error() -> ApiError {
    ApiError::Conflict("Compute capacity is busy. Wait for an existing run or analysis to finish, or cancel it, then retry.".into())
}

fn admit_compute_inner(user: &str) -> Result<ComputePermit, crate::observability::RejectionReason> {
    use crate::observability::RejectionReason;
    let mut counts = COMPUTE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if counts.total >= COMPUTE_LIMIT {
        return Err(RejectionReason::GlobalLimit);
    }
    if counts.users.get(user).copied().unwrap_or(0) >= 2 {
        return Err(RejectionReason::UserLimit);
    }
    counts.total += 1;
    *counts.users.entry(user.to_string()).or_default() += 1;
    Ok(ComputePermit {
        user: user.to_string(),
    })
}
impl Drop for ComputePermit {
    fn drop(&mut self) {
        let mut counts = COMPUTE
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        counts.total -= 1;
        if let Some(n) = counts.users.get_mut(&self.user) {
            *n -= 1;
            if *n == 0 {
                counts.users.remove(&self.user);
            }
        }
    }
}

/// Call after BEGIN IMMEDIATE and before inserting any new plan. The SQLite
/// writer lock serializes regular creation, onboarding, duplication and import.
pub async fn check_plan_slot(
    connection: &mut sqlx::SqliteConnection,
    user: &str,
    config: &ServerConfig,
    additional: i64,
) -> ApiResult<()> {
    if AccessMode::from_config(config) != AccessMode::Subscription {
        return Ok(());
    }
    let pro:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM subscriptions WHERE user_id=? AND state IN ('active','canceling','past_due') AND access_until>unixepoch())").bind(user).fetch_one(&mut *connection).await?;
    if pro {
        return Ok(());
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM scenarios WHERE user_id=?")
        .bind(user)
        .fetch_one(&mut *connection)
        .await?;
    if count.saturating_add(additional) > 1 {
        return Err(ApiError::Forbidden(
            "Free includes one saved plan. Existing plans remain readable and exportable.".into(),
        ));
    }
    Ok(())
}
