//! Provider-neutral access policy. Only trusted server adapters may reconcile
//! subscriptions: there is intentionally no browser endpoint granting paid access.
use crate::{
    auth::session::CurrentUser,
    config::{HostedAccessMode, ServerConfig},
    db::Db,
    error::{ApiError, ApiResult},
    observability::Tier,
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
    /// A guest (spec 17): signed in without an account. On a hosted server a
    /// guest is never `pro`, whatever the access mode, and gets the guest
    /// iteration cap, no goal seeks and no AI. Self-hosted guests are
    /// unrestricted; this still says they have no account.
    pub guest: bool,
    /// Days without a visit before a guest and its plans are deleted; set
    /// for every guest, so the banner can say so.
    pub guest_retention_days: Option<i64>,
    pub access_mode: AccessMode,
    pub hosted: bool,
    pub goal_seeks_used_this_month: i64,
    pub max_iterations: usize,
    pub saved_plan_limit: Option<usize>,
    pub goal_seeks_per_month: Option<usize>,
    pub editable_scenario_id: Option<i64>,
    pub annual_price_usd: u32,
    pub monthly_price_usd: u32,
    /// AI-guided drafts (`api::drafts`); null when the server has no review
    /// model, so the web hides the option entirely. Filled in by
    /// [`entitlements_for`], which knows whether a model is configured;
    /// [`entitlements`] alone leaves it null.
    pub ai_drafts: Option<AiDrafts>,
    /// Plan chat on the Review tab (`api::plan_chat`); null when the server
    /// has no review model. Filled in by [`entitlements_for`], as `ai_drafts`.
    pub ai_plan_chat: Option<AiPlanChat>,
}

/// What a user may do with AI-guided drafts right now.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct AiDrafts {
    /// The user can start a draft: quota remains and a plan slot is free.
    pub enabled: bool,
    /// Drafts left this calendar month (UTC).
    pub remaining: u32,
    /// Drafts the tier gets each calendar month, for "1 of 2 left".
    pub per_month: u32,
    pub max_files: u32,
    pub max_bytes: u64,
    pub max_pages: u32,
}
/// What a user may do with plan chat right now.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct AiPlanChat {
    /// The user can send a message: this month's allowance is not used up.
    pub enabled: bool,
    /// Messages left this calendar month (UTC).
    pub remaining: u32,
    /// Messages the tier gets each calendar month.
    pub per_month: u32,
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
    Ok(Json(entitlements_for(&state, &user.id).await?))
}

/// [`entitlements`] with the AI draft limits and quota, which need to know
/// whether the server has a review model.
pub async fn entitlements_for(state: &AppState, user: &str) -> ApiResult<Entitlements> {
    let mut out = entitlements(&state.db, user, &state.config).await?;
    if state.review_ai.is_some() && !limited_guest(&out, &state.config) {
        let limits = state.config.draft.limits(out.pro);
        let used = ai_drafts_used(&state.db, user).await?;
        let remaining = limits.drafts_per_month.saturating_sub(used);
        // Read before the connection below is taken: a pool of one would
        // otherwise wait on itself.
        let chat_per_month = state.config.plan_chat.per_month(out.pro);
        let chat_remaining =
            chat_per_month.saturating_sub(ai_plan_chats_used(&state.db, user).await?);
        let mut conn = state.db.acquire().await?;
        let slot = check_plan_slot(&mut conn, user, &state.config, 1)
            .await
            .is_ok();
        drop(conn);
        out.ai_drafts = Some(AiDrafts {
            enabled: remaining > 0 && slot,
            remaining,
            per_month: limits.drafts_per_month,
            max_files: limits.max_files,
            max_bytes: limits.max_bytes,
            max_pages: limits.max_pages,
        });
        out.ai_plan_chat = Some(AiPlanChat {
            enabled: chat_remaining > 0,
            remaining: chat_remaining,
            per_month: chat_per_month,
        });
    }
    Ok(out)
}

async fn ai_plan_chats_used(db: &Db, user: &str) -> ApiResult<u32> {
    let used: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT used FROM monthly_ai_plan_chats
                           WHERE user_id = ? AND month = strftime('%Y-%m','now')), 0)",
    )
    .bind(user)
    .fetch_one(db)
    .await?;
    Ok(used.clamp(0, i64::from(u32::MAX)) as u32)
}

async fn ai_drafts_used(db: &Db, user: &str) -> ApiResult<u32> {
    let used: i64 = sqlx::query_scalar("SELECT COALESCE((SELECT used FROM monthly_ai_drafts WHERE user_id=? AND month=strftime('%Y-%m','now')),0)").bind(user).fetch_one(db).await?;
    Ok(used.clamp(0, i64::from(u32::MAX)) as u32)
}
/// Whether `user` is a guest. Read on its own so every caller of
/// [`entitlements`] gets the guest rules without passing the session along.
async fn is_guest(db: &Db, user: &str) -> ApiResult<bool> {
    Ok(
        sqlx::query_scalar::<_, bool>("SELECT kind = 'guest' FROM users WHERE id = ?")
            .bind(user)
            .fetch_optional(db)
            .await?
            .unwrap_or(false),
    )
}

/// A guest on a hosted server, where the guest limits apply. Self-hosted
/// guests are the single local user and are not limited.
fn limited_guest(e: &Entitlements, config: &ServerConfig) -> bool {
    e.guest && config.hosted
}

/// The most iterations any simulation path may spend for this caller: runs,
/// previews, what-ifs and analyses each clamp to `min(their own constant,
/// iteration_cap)`, so no path can outrun the tier's run cap.
#[must_use]
pub fn iteration_cap(e: &Entitlements, config: &ServerConfig) -> usize {
    e.max_iterations.min(config.max_iterations)
}

pub async fn entitlements(db: &Db, user: &str, config: &ServerConfig) -> ApiResult<Entitlements> {
    let access_mode = AccessMode::from_config(config);
    let guest = is_guest(db, user).await?;
    // Guests are checked before the access mode: in beta every account is
    // `pro`, and a hosted guest must not inherit that.
    let limited = guest && config.hosted;
    let pro = !limited && (access_mode != AccessMode::Subscription || sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM subscriptions WHERE user_id = ? AND state IN ('active','canceling','past_due') AND access_until > unixepoch())").bind(user).fetch_one(db).await?);
    let editable = if pro {
        None
    } else {
        sqlx::query_scalar::<_, Option<i64>>("SELECT COALESCE((SELECT scenario_id FROM editable_plans WHERE user_id = ?1), (SELECT MIN(id) FROM scenarios WHERE user_id = ?1 AND status = 'active'))").bind(user).fetch_one(db).await?
    };
    let used:i64=sqlx::query_scalar("SELECT COALESCE((SELECT used FROM monthly_goal_seeks WHERE user_id=? AND month=strftime('%Y-%m','now')),0)").bind(user).fetch_one(db).await?;
    Ok(Entitlements {
        pro,
        guest,
        guest_retention_days: guest.then_some(config.guest_retention_days),
        hosted: config.hosted,
        access_mode,
        goal_seeks_used_this_month: used,
        max_iterations: if !config.hosted {
            config.max_iterations
        } else if limited {
            config.max_iterations.min(config.guest_max_iterations)
        } else {
            config.max_iterations.min(if pro { 50_000 } else { 1_000 })
        },
        saved_plan_limit: if pro { None } else { Some(1) },
        goal_seeks_per_month: if pro {
            None
        } else if limited {
            Some(0)
        } else {
            Some(1)
        },
        editable_scenario_id: editable,
        annual_price_usd: 80,
        monthly_price_usd: 10,
        ai_drafts: None,
        ai_plan_chat: None,
    })
}
pub async fn require_pro(db: &Db, user: &str, config: &ServerConfig) -> ApiResult<()> {
    let e = entitlements(db, user, config).await?;
    if e.pro {
        Ok(())
    } else if limited_guest(&e, config) {
        Err(ApiError::Forbidden(
            "Create a free account to use this. Your guest plan comes with you.".into(),
        ))
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
    // A draft is being written, not saved: the plan-slot rule applies when it
    // becomes a plan (`check_plan_slot` at Create), not while it is edited.
    let draft: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM scenarios WHERE id = ? AND user_id = ? AND status = 'draft')",
    )
    .bind(scenario)
    .bind(user)
    .fetch_one(db)
    .await?;
    if e.pro || draft || e.editable_scenario_id == Some(scenario) {
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
    let e = entitlements(db, user, config).await?;
    if e.pro {
        return Ok(());
    }
    if limited_guest(&e, config) {
        return Err(ApiError::Forbidden(
            "Create a free account to use goal seek.".into(),
        ));
    }
    let accepted = sqlx::query("INSERT INTO monthly_goal_seeks(user_id,month,used) VALUES (?,strftime('%Y-%m','now'),1) ON CONFLICT(user_id,month) DO UPDATE SET used=used+1 WHERE used < 1")
        .bind(user).execute(db).await?.rows_affected();
    if accepted == 0 {
        return Err(ApiError::Forbidden("Free includes one goal seek per calendar month (UTC). Try next month or upgrade to Pro.".into()));
    }
    Ok(())
}

/// Call once after request validation, before starting an AI draft, and after
/// the plan slot has been checked so a draft is never spent on a plan that
/// cannot be created. Runs on the caller's write transaction, so the count and
/// the increment cannot race. A started draft counts even if it is discarded.
pub async fn reserve_ai_draft(
    connection: &mut sqlx::SqliteConnection,
    user: &str,
    limit: u32,
) -> ApiResult<()> {
    let accepted = sqlx::query("INSERT INTO monthly_ai_drafts(user_id,month,used) VALUES (?1,strftime('%Y-%m','now'),1) ON CONFLICT(user_id,month) DO UPDATE SET used=used+1 WHERE used < ?2")
        .bind(user).bind(i64::from(limit)).execute(&mut *connection).await?.rows_affected();
    if accepted == 0 {
        return Err(ApiError::Forbidden(
            "You have used this month's AI drafts (calendar month, UTC). Try next month or upgrade to Pro.".into(),
        ));
    }
    Ok(())
}

/// Call once per plan chat message, after it is validated and the thread is
/// known to be free, on the caller's write transaction so the count and the
/// increment cannot race. An accepted message counts even if its turn fails.
pub async fn reserve_ai_plan_chat(
    connection: &mut sqlx::SqliteConnection,
    user: &str,
    limit: u32,
) -> ApiResult<()> {
    let accepted = sqlx::query(
        "INSERT INTO monthly_ai_plan_chats(user_id, month, used)
         SELECT ?1, strftime('%Y-%m','now'), 1 WHERE ?2 > 0
         ON CONFLICT(user_id, month) DO UPDATE SET used = used + 1 WHERE used < ?2",
    )
    .bind(user)
    .bind(i64::from(limit))
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if accepted == 0 {
        return Err(ApiError::Forbidden(
            "You have used this month's plan chat messages (calendar month, UTC). Try next month or upgrade to Pro.".into(),
        ));
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
    #[test]
    fn a_guest_holds_one_compute_permit_and_an_account_two() {
        use crate::observability::RejectionReason::*;
        let mut counts = ComputeCounts::default();
        assert_eq!(counts.admit("g", Tier::Guest), Ok(()));
        assert_eq!(counts.admit("g", Tier::Guest), Err(UserLimit));
        assert_eq!(counts.admit("a", Tier::Free), Ok(()));
        assert_eq!(counts.admit("a", Tier::Free), Ok(()));
        assert_eq!(counts.admit("a", Tier::Free), Err(UserLimit));
        assert_eq!(counts.admit("p", Tier::Pro), Ok(()));
        assert_eq!(counts.admit("p", Tier::Pro), Ok(()));
        assert_eq!(counts.admit("p", Tier::Pro), Err(UserLimit));
    }
    #[test]
    fn guests_together_stop_at_half_the_limit_and_accounts_still_get_in() {
        use crate::observability::RejectionReason::*;
        let mut counts = ComputeCounts::default();
        for n in 0..GUEST_COMPUTE_LIMIT {
            assert_eq!(counts.admit(&format!("g{n}"), Tier::Guest), Ok(()));
        }
        assert_eq!(GUEST_COMPUTE_LIMIT, COMPUTE_LIMIT / 2);
        assert_eq!(counts.admit("late", Tier::Guest), Err(GuestLimit));
        // The other half of the limit is the accounts'.
        for n in 0..COMPUTE_LIMIT - GUEST_COMPUTE_LIMIT {
            assert_eq!(counts.admit(&format!("a{n}"), Tier::Free), Ok(()));
        }
        assert_eq!(counts.admit("late", Tier::Pro), Err(GlobalLimit));
    }
    #[test]
    fn releasing_guest_permits_frees_the_guest_share() {
        use crate::observability::RejectionReason::*;
        let mut counts = ComputeCounts::default();
        for n in 0..GUEST_COMPUTE_LIMIT {
            counts.admit(&format!("g{n}"), Tier::Guest).unwrap();
        }
        assert_eq!(counts.admit("late", Tier::Guest), Err(GuestLimit));
        counts.release("g0", Tier::Guest);
        assert_eq!(counts.guests, GUEST_COMPUTE_LIMIT - 1);
        assert_eq!(counts.total, GUEST_COMPUTE_LIMIT - 1);
        assert_eq!(counts.admit("late", Tier::Guest), Ok(()));
        // The released guest may start again.
        assert_eq!(counts.admit("g0", Tier::Guest), Err(GuestLimit));
        counts.release("late", Tier::Guest);
        assert_eq!(counts.admit("g0", Tier::Guest), Ok(()));
        for n in 0..GUEST_COMPUTE_LIMIT {
            counts.release(&format!("g{n}"), Tier::Guest);
        }
        assert_eq!((counts.total, counts.guests), (0, 0));
        assert!(counts.users.is_empty());
    }
    #[test]
    fn a_guest_permit_remembers_it_is_a_guest_when_dropped() {
        // The gate is process-wide and other tests hold permits, so wait for a
        // free guest slot rather than assert on totals.
        let user = "tier-test-guest";
        let first = (0..200)
            .find_map(|_| {
                admit_compute_inner(user, Tier::Guest).ok().or_else(|| {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    None
                })
            })
            .expect("a guest slot frees up");
        assert_eq!(first.tier(), Tier::Guest);
        // One job per guest, however much room is left.
        assert!(admit_compute_inner(user, Tier::Guest).is_err());
        drop(first);
        let second = admit_compute_inner(user, Tier::Guest);
        // Another test's guests may have taken the freed slot; the guest's own
        // limit no longer blocks it.
        assert!(matches!(
            second.as_ref().err(),
            None | Some(
                crate::observability::RejectionReason::GuestLimit
                    | crate::observability::RejectionReason::GlobalLimit
            )
        ));
    }
    #[tokio::test]
    async fn tier_is_guest_only_where_guest_limits_apply() {
        let db = fixture().await;
        sqlx::query("INSERT INTO users(id,email,password_hash,kind) VALUES ('gst','g@guest.invalid','!','guest')").execute(&db).await.unwrap();
        for hosted in [true, false] {
            let config = config(hosted);
            assert_eq!(
                entitlements(&db, "gst", &config)
                    .await
                    .unwrap()
                    .tier(&config),
                if hosted { Tier::Guest } else { Tier::Pro }
            );
        }
        let config = config(true);
        assert_eq!(
            entitlements(&db, "u", &config).await.unwrap().tier(&config),
            Tier::Free
        );
        sqlx::query("INSERT INTO subscriptions(user_id,provider,subscription_id,state,access_until,revision) VALUES ('v','fake','s','active',4000000000,1)").execute(&db).await.unwrap();
        assert_eq!(
            entitlements(&db, "v", &config).await.unwrap().tier(&config),
            Tier::Pro
        );
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

    #[tokio::test]
    async fn plan_chat_reservations_stop_at_the_limit_per_user() {
        let db = fixture().await;
        let mut conn = db.acquire().await.unwrap();
        for _ in 0..3 {
            reserve_ai_plan_chat(&mut conn, "u", 3).await.unwrap();
        }
        assert!(reserve_ai_plan_chat(&mut conn, "u", 3).await.is_err());
        // Another user's allowance is their own; a zero limit allows nothing.
        reserve_ai_plan_chat(&mut conn, "v", 3).await.unwrap();
        assert!(reserve_ai_plan_chat(&mut conn, "v", 0).await.is_err());
        drop(conn);
        assert_eq!(ai_plan_chats_used(&db, "u").await.unwrap(), 3);
        assert_eq!(ai_plan_chats_used(&db, "v").await.unwrap(), 1);
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
    let updated=sqlx::query("INSERT INTO editable_plans(user_id,scenario_id) SELECT ?1,id FROM scenarios WHERE id=?2 AND user_id=?1 AND status='active' ON CONFLICT(user_id) DO UPDATE SET scenario_id=excluded.scenario_id")
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
                    "tax-configs"=>"SELECT EXISTS(SELECT 1 FROM scenarios WHERE user_id=?1 AND status='active' AND id<>COALESCE(?2,-1) AND tax_config_id=?3)",
                    "inflation-profiles"=>"SELECT EXISTS(SELECT 1 FROM scenarios WHERE user_id=?1 AND status='active' AND id<>COALESCE(?2,-1) AND inflation_profile_id=?3)",
                    _=>"SELECT EXISTS(SELECT 1 FROM scenarios s WHERE s.user_id=?1 AND s.status='active' AND s.id<>COALESCE(?2,-1) AND (EXISTS(SELECT 1 FROM assets a WHERE a.scenario_id=s.id AND a.return_profile_id=?3) OR EXISTS(SELECT 1 FROM accounts a JOIN account_bank c ON c.account_id=a.id WHERE a.scenario_id=s.id AND c.return_profile_id=?3) OR EXISTS(SELECT 1 FROM accounts a JOIN account_investment i ON i.account_id=a.id WHERE a.scenario_id=s.id AND i.cash_return_profile_id=?3)))",
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
    /// Permits held by guests, all of them together (spec 17).
    guests: usize,
    users: std::collections::HashMap<String, usize>,
}
static COMPUTE: std::sync::OnceLock<std::sync::Mutex<ComputeCounts>> = std::sync::OnceLock::new();
/// Counts queued and running work together; releasing a worker permit alone is
/// insufficient to bound retained jobs. Hold this through the job's terminal state.
pub struct ComputePermit {
    user: String,
    /// Remembered so [`Drop`] gives back the guest share too.
    tier: Tier,
}
impl ComputePermit {
    pub fn tier(&self) -> Tier {
        self.tier
    }
}
pub const COMPUTE_LIMIT: usize = 16;
/// Guests together hold at most half the limit, so a burst of guests queues
/// behind itself rather than in front of paying users.
pub const GUEST_COMPUTE_LIMIT: usize = COMPUTE_LIMIT / 2;
/// Concurrent jobs per account.
const USER_COMPUTE_LIMIT: usize = 2;
/// Concurrent jobs per guest.
const GUEST_USER_COMPUTE_LIMIT: usize = 1;

impl Entitlements {
    /// The label admission is counted under: a limited guest, a Pro account,
    /// or a Free account.
    #[must_use]
    pub fn tier(&self, config: &ServerConfig) -> Tier {
        if limited_guest(self, config) {
            Tier::Guest
        } else if self.pro {
            Tier::Pro
        } else {
            Tier::Free
        }
    }
}

/// Snapshot the existing process-wide admission gate; no shadow permit count.
pub fn compute_admitted() -> usize {
    COMPUTE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .total
}

/// Admit as an account, unobserved.
pub fn admit_compute(user: &str) -> ApiResult<ComputePermit> {
    admit_compute_inner(user, Tier::Free).map_err(|_| capacity_error())
}

pub fn admit_compute_observed(
    user: &str,
    tier: Tier,
    telemetry: &crate::observability::Telemetry,
    origin: crate::observability::Origin,
) -> ApiResult<ComputePermit> {
    match admit_compute_inner(user, tier) {
        Ok(permit) => {
            telemetry.admission(tier, origin);
            Ok(permit)
        }
        Err(reason) => {
            telemetry.rejection(reason, tier, origin);
            Err(capacity_error())
        }
    }
}

fn capacity_error() -> ApiError {
    ApiError::Conflict("Compute capacity is busy. Wait for an existing run or analysis to finish, or cancel it, then retry.".into())
}

fn admit_compute_inner(
    user: &str,
    tier: Tier,
) -> Result<ComputePermit, crate::observability::RejectionReason> {
    COMPUTE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .admit(user, tier)?;
    Ok(ComputePermit {
        user: user.to_string(),
        tier,
    })
}
impl ComputeCounts {
    fn admit(
        &mut self,
        user: &str,
        tier: Tier,
    ) -> Result<(), crate::observability::RejectionReason> {
        use crate::observability::RejectionReason;
        let guest = tier == Tier::Guest;
        if self.total >= COMPUTE_LIMIT {
            return Err(RejectionReason::GlobalLimit);
        }
        if guest && self.guests >= GUEST_COMPUTE_LIMIT {
            return Err(RejectionReason::GuestLimit);
        }
        let per_user = if guest {
            GUEST_USER_COMPUTE_LIMIT
        } else {
            USER_COMPUTE_LIMIT
        };
        if self.users.get(user).copied().unwrap_or(0) >= per_user {
            return Err(RejectionReason::UserLimit);
        }
        self.total += 1;
        self.guests += usize::from(guest);
        *self.users.entry(user.to_string()).or_default() += 1;
        Ok(())
    }
    fn release(&mut self, user: &str, tier: Tier) {
        self.total -= 1;
        self.guests -= usize::from(tier == Tier::Guest);
        if let Some(n) = self.users.get_mut(user) {
            *n -= 1;
            if *n == 0 {
                self.users.remove(user);
            }
        }
    }
}
impl Drop for ComputePermit {
    fn drop(&mut self) {
        COMPUTE
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .release(&self.user, self.tier);
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
    // A hosted guest gets one plan in every access mode, beta included.
    let guest: bool = config.hosted
        && sqlx::query_scalar("SELECT kind = 'guest' FROM users WHERE id = ?")
            .bind(user)
            .fetch_optional(&mut *connection)
            .await?
            .unwrap_or(false);
    if !guest && AccessMode::from_config(config) != AccessMode::Subscription {
        return Ok(());
    }
    let pro: bool = !guest
        && sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM subscriptions WHERE user_id=? AND state IN ('active','canceling','past_due') AND access_until>unixepoch())")
            .bind(user)
            .fetch_one(&mut *connection)
            .await?;
    if pro {
        return Ok(());
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM scenarios WHERE user_id=? AND status='active'")
            .bind(user)
            .fetch_one(&mut *connection)
            .await?;
    if count.saturating_add(additional) > 1 {
        return Err(ApiError::Forbidden(if guest {
            "A guest keeps one plan. Create a free account to keep it, or sign in.".into()
        } else {
            "Free includes one saved plan. Existing plans remain readable and exportable.".into()
        }));
    }
    Ok(())
}
