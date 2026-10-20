//! The admin listener's pages. Each is one handler: a few queries, rendered.

// Rows are read as tuples straight off each page's own query, and used once.
#![allow(clippy::type_complexity)]

use std::fmt::Write as _;

use axum::extract::{Path, Query, State};
use axum::response::Html;
use serde::Deserialize;

use super::html::{
    Nav, PageResult, cents, col, count, esc, facts, money, num, opt, page, seg, table, tiles,
    user_link, yes_no,
};
use crate::billing::{self, AccessMode};
use crate::db::Db;
use crate::offload;
use crate::state::AppState;
use crate::usage::Feature;

/// Rows a list page shows at once.
const PAGE_SIZE: i64 = 100;

/// A subscription that grants Pro right now (as `billing::entitlements`).
const SUBSCRIBED: &str =
    "state IN ('active','canceling','past_due') AND access_until > unixepoch()";

/// This calendar month (UTC), as the `created_at` lower bound.
const MONTH_START: &str = "datetime('now', 'start of month')";

#[derive(Deserialize)]
pub struct NoticeQuery {
    notice: Option<String>,
}

/// What an action's redirect reports. Codes rather than text, so a link
/// cannot put words on the page.
fn notice_text(code: Option<&str>) -> Option<&'static str> {
    Some(match code? {
        "granted" => "Pro granted.",
        "revoked" => "Pro grant revoked.",
        "quotas_reset" => "This month's allowances were reset.",
        "signed_out" => "Every session was signed out.",
        "deleted" => "The user and everything they owned were deleted.",
        "status" => "Message updated.",
        _ => return None,
    })
}

fn access_line(state: &AppState) -> String {
    match AccessMode::from_config(&state.config) {
        AccessMode::SelfHosted => "Self-hosted: every account has Pro features.".into(),
        AccessMode::Beta => "Hosted beta: every account has Pro features.".into(),
        AccessMode::Subscription => {
            "Hosted, subscription access: Pro needs an active subscription.".into()
        }
    }
}

// ── overview ────────────────────────────────────────────────────────────────

pub async fn overview(
    State(state): State<AppState>,
    Query(q): Query<NoticeQuery>,
) -> PageResult<Html<String>> {
    let db = &state.db;
    let (accounts, verified, guests): (i64, i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(kind = 'account'), 0),
                COALESCE(SUM(kind = 'account' AND email_verified_at IS NOT NULL), 0),
                COALESCE(SUM(kind = 'guest'), 0)
           FROM users",
    )
    .fetch_one(db)
    .await?;
    let subscribed: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM subscriptions WHERE {SUBSCRIBED}"
    ))
    .fetch_one(db)
    .await?;
    let (sessions, active_users): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(DISTINCT user_id) FROM sessions
          WHERE expires_at > datetime('now') AND last_seen > datetime('now', '-7 days')",
    )
    .fetch_one(db)
    .await?;
    let plans: i64 = sqlx::query_scalar("SELECT count(*) FROM scenarios WHERE status = 'active'")
        .fetch_one(db)
        .await?;
    let (month_cost, month_passes): (f64, i64) = sqlx::query_as(&format!(
        "SELECT COALESCE(SUM(cost_usd), 0.0), count(*) FROM ai_usage WHERE created_at >= {MONTH_START}"
    ))
    .fetch_one(db)
    .await?;
    let total_cost: f64 = sqlx::query_scalar("SELECT COALESCE(SUM(cost_usd), 0.0) FROM ai_usage")
        .fetch_one(db)
        .await?;
    let offload: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(used), 0) FROM monthly_offload_spend
          WHERE month = strftime('%Y-%m', 'now')",
    )
    .fetch_one(db)
    .await?;
    let inbox: i64 =
        sqlx::query_scalar("SELECT count(*) FROM contact_messages WHERE status = 'new'")
            .fetch_one(db)
            .await?;

    let mut body = format!("<p class=muted>{}</p>", esc(&access_line(&state)));
    body.push_str(&tiles(&[
        (
            "Accounts",
            format!(
                "{} <span class=muted>({} verified)</span>",
                count(accounts),
                count(verified)
            ),
        ),
        ("Guests", count(guests)),
        ("Subscribed", count(subscribed)),
        (
            "Active in 7 days",
            format!(
                "{} <span class=muted>({} sessions)</span>",
                count(active_users),
                count(sessions)
            ),
        ),
        ("Plans", count(plans)),
        (
            "AI spend this month",
            format!(
                "{} <span class=muted>({} passes)</span>",
                money(month_cost),
                count(month_passes)
            ),
        ),
        ("AI spend all time", money(total_cost)),
        ("Offload units this month", count(offload)),
        (
            "New messages",
            format!("<a href=\"/inbox\">{}</a>", count(inbox)),
        ),
    ]));

    body.push_str("<h2>AI spend this month, by feature</h2>");
    body.push_str(&by_feature(db, Some(MONTH_START)).await?);

    body.push_str("<h2>Top AI spenders this month</h2>");
    let spenders: Vec<(Option<String>, Option<String>, i64, i64, f64)> = sqlx::query_as(&format!(
        "SELECT u.id, u.email, count(*), SUM(a.input_tokens + a.output_tokens), SUM(a.cost_usd)
           FROM ai_usage a LEFT JOIN users u ON u.id = a.user_id
          WHERE a.created_at >= {MONTH_START}
          GROUP BY a.user_id ORDER BY SUM(a.cost_usd) DESC LIMIT 10"
    ))
    .fetch_all(db)
    .await?;
    body.push_str(&table(
        &[col("User"), num("Passes"), num("Tokens"), num("Cost")],
        spenders
            .into_iter()
            .map(|(id, email, passes, tokens, cost)| {
                vec![
                    user_link(id.as_deref(), email.as_deref()),
                    count(passes),
                    count(tokens),
                    money(cost),
                ]
            }),
        "No AI use this month.",
    ));

    body.push_str("<h2>Newest accounts</h2>");
    let newest: Vec<(String, String, Option<String>, String, bool)> = sqlx::query_as(
        "SELECT id, email, display_name, created_at, email_verified_at IS NOT NULL
           FROM users WHERE kind = 'account' ORDER BY created_at DESC, rowid DESC LIMIT 10",
    )
    .fetch_all(db)
    .await?;
    body.push_str(&table(
        &[col("Email"), col("Name"), col("Created"), col("Verified")],
        newest
            .into_iter()
            .map(|(id, email, name, created, verified)| {
                vec![
                    user_link(Some(&id), Some(&email)),
                    opt(name.as_deref()),
                    esc(&created).to_string(),
                    yes_no(verified).into(),
                ]
            }),
        "No accounts yet.",
    ));

    Ok(page(
        "Overview",
        Nav::Overview,
        notice_text(q.notice.as_deref()),
        &body,
    ))
}

/// AI spend by feature since `since` (an SQL expression), or all time.
async fn by_feature(db: &Db, since: Option<&str>) -> PageResult<String> {
    by_feature_where(db, since.map(|s| format!("created_at >= {s}")), None).await
}

/// AI spend by feature, filtered by an SQL condition and, optionally, a user.
async fn by_feature_where(
    db: &Db,
    condition: Option<String>,
    user: Option<&str>,
) -> PageResult<String> {
    let mut filter = vec!["(?1 IS NULL OR user_id = ?1)".to_owned()];
    filter.extend(condition);
    let rows: Vec<(String, i64, i64, i64, i64, f64)> = sqlx::query_as(&format!(
        "SELECT feature, count(*), SUM(turns), SUM(input_tokens), SUM(output_tokens),
                SUM(cost_usd)
           FROM ai_usage WHERE {} GROUP BY feature",
        filter.join(" AND ")
    ))
    .bind(user)
    .fetch_all(db)
    .await?;
    // In a fixed order, with the features nobody used left out.
    let ordered = Feature::ALL
        .iter()
        .filter_map(|f| rows.iter().find(|r| r.0 == f.as_str()));
    Ok(table(
        &[
            col("Feature"),
            num("Passes"),
            num("Turns"),
            num("Input tokens"),
            num("Output tokens"),
            num("Cost"),
        ],
        ordered.map(|(feature, passes, turns, input, output, cost)| {
            vec![
                esc(feature).to_string(),
                count(*passes),
                count(*turns),
                count(*input),
                count(*output),
                money(*cost),
            ]
        }),
        "No AI use.",
    ))
}

// ── users ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UsersQuery {
    #[serde(default)]
    q: String,
    kind: Option<String>,
    sort: Option<String>,
    #[serde(default)]
    page: i64,
    notice: Option<String>,
}

pub async fn users(
    State(state): State<AppState>,
    Query(q): Query<UsersQuery>,
) -> PageResult<Html<String>> {
    let kind = match q.kind.as_deref() {
        Some("guest") => "guest",
        Some("all") => "all",
        _ => "account",
    };
    let (sort, order) = match q.sort.as_deref() {
        Some("seen") => ("seen", "last_seen DESC NULLS LAST"),
        Some("spend") => ("spend", "ai_month DESC"),
        Some("plans") => ("plans", "plans DESC"),
        _ => ("created", "u.created_at DESC, u.rowid DESC"),
    };
    let search = q.q.trim();
    let page_no = q.page.max(0);

    let rows: Vec<(
        String,
        String,
        Option<String>,
        String,
        bool,
        String,
        Option<String>,
        i64,
        bool,
        f64,
    )> = sqlx::query_as(&format!(
        "SELECT u.id, u.email, u.display_name, u.kind, u.email_verified_at IS NOT NULL,
                    u.created_at,
                    (SELECT MAX(last_seen) FROM sessions s WHERE s.user_id = u.id) AS last_seen,
                    (SELECT count(*) FROM scenarios s
                      WHERE s.user_id = u.id AND s.status = 'active') AS plans,
                    EXISTS(SELECT 1 FROM subscriptions b WHERE b.user_id = u.id AND {SUBSCRIBED}),
                    COALESCE((SELECT SUM(cost_usd) FROM ai_usage a
                               WHERE a.user_id = u.id AND a.created_at >= {MONTH_START}), 0.0)
                      AS ai_month
               FROM users u
              WHERE (?1 = '' OR u.id = ?1 OR instr(lower(u.email), lower(?1)) > 0
                     OR instr(lower(COALESCE(u.display_name, '')), lower(?1)) > 0)
                AND (?2 = 'all' OR u.kind = ?2)
              ORDER BY {order}
              LIMIT ?3 OFFSET ?4"
    ))
    .bind(search)
    .bind(kind)
    .bind(PAGE_SIZE + 1)
    .bind(page_no * PAGE_SIZE)
    .fetch_all(&state.db)
    .await?;
    let more = rows.len() as i64 > PAGE_SIZE;

    let mut body = String::from("<form class=filters method=get action=\"/users\">");
    let _ = write!(
        body,
        "<input type=search name=q placeholder=\"Email, name or id\" value=\"{}\" size=30>",
        esc(search)
    );
    body.push_str(&select(
        "kind",
        kind,
        &[
            ("account", "Accounts"),
            ("guest", "Guests"),
            ("all", "Everyone"),
        ],
    ));
    body.push_str(&select(
        "sort",
        sort,
        &[
            ("created", "Newest"),
            ("seen", "Last seen"),
            ("spend", "AI spend this month"),
            ("plans", "Most plans"),
        ],
    ));
    body.push_str("<button>Filter</button></form>");

    body.push_str(&table(
        &[
            col("Email"),
            col("Name"),
            col("Kind"),
            col("Verified"),
            col("Created"),
            col("Last seen"),
            num("Plans"),
            col("Subscribed"),
            num("AI this month"),
        ],
        rows.into_iter().take(PAGE_SIZE as usize).map(
            |(id, email, name, kind, verified, created, seen, plans, subscribed, ai)| {
                vec![
                    user_link(Some(&id), Some(&email)),
                    opt(name.as_deref()),
                    esc(&kind).to_string(),
                    yes_no(verified).into(),
                    esc(&created).to_string(),
                    opt(seen.as_deref()),
                    count(plans),
                    yes_no(subscribed).into(),
                    money(ai),
                ]
            },
        ),
        "No users match.",
    ));

    let link = |page: i64| {
        format!(
            "/users?q={}&kind={kind}&sort={sort}&page={page}",
            seg(search)
        )
    };
    body.push_str("<p>");
    if page_no > 0 {
        let _ = write!(
            body,
            "<a href=\"{}\">← Previous</a> ",
            esc(&link(page_no - 1))
        );
    }
    if more {
        let _ = write!(body, "<a href=\"{}\">Next →</a>", esc(&link(page_no + 1)));
    }
    body.push_str("</p>");

    Ok(page(
        "Users",
        Nav::Users,
        notice_text(q.notice.as_deref()),
        &body,
    ))
}

fn select(name: &str, current: &str, options: &[(&str, &str)]) -> String {
    let mut out = format!("<select name={name}>");
    for (value, label) in options {
        let selected = if *value == current { " selected" } else { "" };
        let _ = write!(out, "<option value={value}{selected}>{label}</option>");
    }
    out.push_str("</select>");
    out
}

// ── one user ────────────────────────────────────────────────────────────────

pub async fn user(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<NoticeQuery>,
) -> PageResult<Html<String>> {
    let db = &state.db;
    type Profile = (
        String,
        Option<String>,
        String,
        Option<String>,
        String,
        String,
    );
    let profile: Option<Profile> = sqlx::query_as(
        "SELECT email, display_name, kind, email_verified_at, created_at, updated_at
           FROM users WHERE id = ?1",
    )
    .bind(&id)
    .fetch_optional(db)
    .await?;
    let Some((email, name, kind, verified, created, updated)) = profile else {
        return Err(super::html::Failure::not_found("user"));
    };
    let guest = kind == "guest";
    let entitlements = billing::entitlements_for(&state, &id).await?;
    let offload = offload::standing(db, &id).await?;
    let offload_budget = offload::monthly_budget(&entitlements, &state.config);
    let (goal_seeks, drafts, chats): (i64, i64, i64) = sqlx::query_as(
        "SELECT COALESCE((SELECT used FROM monthly_goal_seeks
                           WHERE user_id = ?1 AND month = strftime('%Y-%m', 'now')), 0),
                COALESCE((SELECT used FROM monthly_ai_drafts
                           WHERE user_id = ?1 AND month = strftime('%Y-%m', 'now')), 0),
                COALESCE((SELECT used FROM monthly_ai_plan_chats
                           WHERE user_id = ?1 AND month = strftime('%Y-%m', 'now')), 0)",
    )
    .bind(&id)
    .fetch_one(db)
    .await?;
    let subscription: Option<(String, String, String, String, i64)> = sqlx::query_as(
        "SELECT provider, subscription_id, state, datetime(access_until, 'unixepoch'), revision
           FROM subscriptions WHERE user_id = ?1",
    )
    .bind(&id)
    .fetch_optional(db)
    .await?;

    let mut body = facts(&[
        ("Id", format!("<code>{}</code>", esc(&id))),
        ("Email", esc(&email).to_string()),
        ("Name", opt(name.as_deref())),
        ("Kind", esc(&kind).to_string()),
        ("Verified", opt(verified.as_deref())),
        ("Created", esc(&created).to_string()),
        ("Updated", esc(&updated).to_string()),
    ]);

    body.push_str("<h2>Access</h2>");
    let limit = |used: i64, of: Option<u32>| match of {
        Some(of) => format!("{} of {}", count(used), count(i64::from(of))),
        None => count(used),
    };
    body.push_str(&facts(&[
        ("Pro features", yes_no(entitlements.pro).into()),
        (
            "Subscription",
            match &subscription {
                Some((provider, sub_id, sub_state, until, revision)) => format!(
                    "{} via {} <span class=muted>({}, until {} UTC, revision {})</span>",
                    esc(sub_state),
                    esc(provider),
                    esc(sub_id),
                    esc(until),
                    revision
                ),
                None => opt(None),
            },
        ),
        ("Max iterations", count(entitlements.max_iterations as i64)),
        (
            "Saved plan limit",
            entitlements
                .saved_plan_limit
                .map_or_else(|| "none".into(), |n| count(n as i64)),
        ),
        (
            "Goal seeks this month",
            limit(
                goal_seeks,
                entitlements.goal_seeks_per_month.map(|n| n as u32),
            ),
        ),
        (
            "AI drafts this month",
            limit(drafts, entitlements.ai_drafts.as_ref().map(|d| d.per_month)),
        ),
        (
            "Plan chat messages this month",
            limit(
                chats,
                entitlements.ai_plan_chat.as_ref().map(|c| c.per_month),
            ),
        ),
        (
            "Offload units this month",
            format!(
                "{} of {} <span class=muted>(resets {})</span>",
                count(offload.used),
                count(offload_budget),
                esc(&offload.resets_at)
            ),
        ),
    ]));

    body.push_str("<h2>Actions</h2><div class=actions>");
    let path = format!("/users/{}", seg(&id));
    if !guest {
        let default_until: String = sqlx::query_scalar("SELECT date('now', '+1 year')")
            .fetch_one(db)
            .await?;
        let _ = write!(
            body,
            "<form method=post action=\"{path}/pro\"><label>Grant Pro until (UTC, inclusive)</label>\
             <input type=date name=until value=\"{}\" required> <button>Grant</button></form>",
            esc(&default_until)
        );
        if subscription
            .as_ref()
            .is_some_and(|s| s.0 == "manual" && s.2 != "expired")
        {
            let _ = write!(
                body,
                "<form method=post action=\"{path}/pro/revoke\"><label>Manual Pro grant</label>\
                 <button>Revoke</button></form>"
            );
        }
    }
    let _ = write!(
        body,
        "<form method=post action=\"{path}/quotas/reset\"><label>This month's goal seeks, AI drafts, \
         plan chats and offload</label><button>Reset allowances</button></form>\
         <form method=post action=\"{path}/sessions/revoke\"><label>Every device</label>\
         <button>Sign out</button></form>\
         <form method=post action=\"{path}/delete\"><label>Type the email to delete the user and all \
         their plans</label><input name=confirm autocomplete=off size=24 required> \
         <button class=danger>Delete user</button></form></div>"
    );

    body.push_str("<h2>Plans</h2>");
    let plans: Vec<(i64, String, String, String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT s.id, s.name, s.status, s.created_at, s.updated_at,
                (SELECT count(*) FROM runs r WHERE r.scenario_id = s.id),
                (SELECT MAX(r.created_at) FROM runs r WHERE r.scenario_id = s.id)
           FROM scenarios s WHERE s.user_id = ?1 ORDER BY s.updated_at DESC",
    )
    .bind(&id)
    .fetch_all(db)
    .await?;
    body.push_str(&table(
        &[
            num("Id"),
            col("Name"),
            col("Status"),
            col("Created"),
            col("Updated"),
            num("Runs"),
            col("Last run"),
        ],
        plans
            .into_iter()
            .map(|(plan, name, status, created, updated, runs, last)| {
                vec![
                    plan.to_string(),
                    esc(&name).to_string(),
                    esc(&status).to_string(),
                    esc(&created).to_string(),
                    esc(&updated).to_string(),
                    count(runs),
                    opt(last.as_deref()),
                ]
            }),
        "No plans on the server.",
    ));

    body.push_str("<h2>AI spend</h2>");
    let (month, all): (f64, f64) = sqlx::query_as(&format!(
        "SELECT COALESCE(SUM(CASE WHEN created_at >= {MONTH_START} THEN cost_usd END), 0.0),
                COALESCE(SUM(cost_usd), 0.0)
           FROM ai_usage WHERE user_id = ?1"
    ))
    .bind(&id)
    .fetch_one(db)
    .await?;
    body.push_str(&facts(&[
        ("This month", money(month)),
        ("All time", money(all)),
    ]));
    body.push_str("<p class=muted>All time, by feature:</p>");
    body.push_str(&by_feature_where(db, None, Some(&id)).await?);
    body.push_str("<p class=muted>Latest passes:</p>");
    body.push_str(&passes(db, Some(&id), None, 20).await?);

    body.push_str("<h2>Sessions</h2>");
    let sessions: Vec<(String, Option<String>, String, String, String)> = sqlx::query_as(
        "SELECT public_id, user_agent, created_at, last_seen, expires_at
           FROM sessions WHERE user_id = ?1 ORDER BY last_seen DESC",
    )
    .bind(&id)
    .fetch_all(db)
    .await?;
    body.push_str(&table(
        &[
            col("Session"),
            col("User agent"),
            col("Signed in"),
            col("Last seen"),
            col("Expires"),
        ],
        sessions
            .into_iter()
            .map(|(public, agent, created, seen, expires)| {
                vec![
                    format!("<code>{}</code>", esc(&public)),
                    opt(agent.as_deref()),
                    esc(&created).to_string(),
                    esc(&seen).to_string(),
                    esc(&expires).to_string(),
                ]
            }),
        "Not signed in anywhere.",
    ));

    body.push_str("<h2>Messages</h2>");
    body.push_str(&messages(db, Some(&id), None, &path).await?);

    Ok(page(
        &email,
        Nav::Users,
        notice_text(q.notice.as_deref()),
        &body,
    ))
}

/// The newest `limit` ledger rows, for one user or everyone, in `month`
/// (`YYYY-MM`) or ever.
async fn passes(
    db: &Db,
    user: Option<&str>,
    month: Option<&str>,
    limit: i64,
) -> PageResult<String> {
    type Pass = (
        String,
        Option<String>,
        Option<String>,
        String,
        Option<i64>,
        String,
        i64,
        i64,
        i64,
        f64,
        i64,
        Option<String>,
    );
    let rows: Vec<Pass> = sqlx::query_as(
        "SELECT a.created_at, u.id, u.email, a.feature, a.scenario_id, a.model, a.turns,
                a.input_tokens, a.output_tokens, a.cost_usd, a.unpriced_turns, a.request_id
           FROM ai_usage a LEFT JOIN users u ON u.id = a.user_id
          WHERE (?1 IS NULL OR a.user_id = ?1)
            AND (?2 IS NULL OR (a.created_at >= ?2 || '-01'
                                AND a.created_at < date(?2 || '-01', '+1 month')))
          ORDER BY a.id DESC LIMIT ?3",
    )
    .bind(user)
    .bind(month)
    .bind(limit)
    .fetch_all(db)
    .await?;
    Ok(table(
        &[
            col("When (UTC)"),
            col("User"),
            col("Feature"),
            num("Plan"),
            col("Model"),
            num("Turns"),
            num("Input"),
            num("Output"),
            num("Cost"),
            col("Request"),
        ],
        rows.into_iter().map(
            |(
                when,
                id,
                email,
                feature,
                plan,
                model,
                turns,
                input,
                output,
                cost,
                unpriced,
                request,
            )| {
                let cost = if unpriced > 0 {
                    format!(
                        "{} <span class=muted title=\"turns without a price\">+{unpriced}?</span>",
                        money(cost)
                    )
                } else {
                    money(cost)
                };
                vec![
                    esc(&when).to_string(),
                    user_link(id.as_deref(), email.as_deref()),
                    esc(&feature).to_string(),
                    plan.map_or_else(|| opt(None), |p| p.to_string()),
                    esc(&model).to_string(),
                    count(turns),
                    count(input),
                    count(output),
                    cost,
                    request.map_or_else(|| opt(None), |r| format!("<code>{}</code>", esc(&r))),
                ]
            },
        ),
        "No AI passes recorded.",
    ))
}

// ── AI spend ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UsageQuery {
    month: Option<String>,
}

/// `YYYY-MM`, or `None`.
fn parse_month(text: &str) -> Option<jiff::civil::Date> {
    let (year, month) = text.split_once('-')?;
    if year.len() != 4 || month.len() != 2 {
        return None;
    }
    jiff::civil::Date::new(year.parse().ok()?, month.parse().ok()?, 1).ok()
}

pub async fn usage(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> PageResult<Html<String>> {
    let db = &state.db;
    let this_month: String = sqlx::query_scalar("SELECT strftime('%Y-%m', 'now')")
        .fetch_one(db)
        .await?;
    let first = q
        .month
        .as_deref()
        .and_then(parse_month)
        .or_else(|| parse_month(&this_month))
        .expect("SQLite formats the month");
    let month = first.strftime("%Y-%m").to_string();
    let step = |months: i32| {
        first
            .checked_add(jiff::Span::new().months(months))
            .map(|d| d.strftime("%Y-%m").to_string())
            .ok()
    };
    // `month` is rebuilt from a parsed date, so it is safe to splice.
    let within = |column: &str| {
        format!("{column} >= '{month}-01' AND {column} < date('{month}-01', '+1 month')")
    };
    let in_month = within("created_at");

    let mut body = String::from("<p class=filters><a href=\"/usage/months\">By month</a> ·");
    if let Some(prev) = step(-1) {
        let _ = write!(body, "<a href=\"/usage?month={prev}\">← {prev}</a>");
    }
    let _ = write!(body, "<strong>{month}</strong>");
    if let Some(next) = step(1).filter(|n| *n <= this_month) {
        let _ = write!(body, "<a href=\"/usage?month={next}\">{next} →</a>");
    }
    body.push_str("</p>");

    let (passes_n, turns, input, output, cache_read, cost, unpriced): (
        i64,
        i64,
        i64,
        i64,
        i64,
        f64,
        i64,
    ) = sqlx::query_as(&format!(
        "SELECT count(*), COALESCE(SUM(turns), 0), COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0), COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cost_usd), 0.0), COALESCE(SUM(unpriced_turns), 0)
           FROM ai_usage WHERE {in_month}"
    ))
    .fetch_one(db)
    .await?;
    body.push_str(&tiles(&[
        ("Cost", money(cost)),
        ("Passes", count(passes_n)),
        ("Turns", count(turns)),
        ("Input tokens", count(input)),
        ("Output tokens", count(output)),
        ("Cache-read tokens", count(cache_read)),
    ]));
    if unpriced > 0 {
        let _ = write!(
            body,
            "<p class=muted>{} turns had no reported or listed price; the cost above leaves them out.</p>",
            count(unpriced)
        );
    }

    body.push_str("<h2>By feature</h2>");
    body.push_str(&by_feature_where(db, Some(in_month.clone()), None).await?);

    body.push_str("<h2>By model</h2>");
    let models: Vec<(String, i64, i64, i64, f64)> = sqlx::query_as(&format!(
        "SELECT model, count(*), SUM(input_tokens), SUM(output_tokens), SUM(cost_usd)
           FROM ai_usage WHERE {in_month} GROUP BY model ORDER BY SUM(cost_usd) DESC"
    ))
    .fetch_all(db)
    .await?;
    body.push_str(&table(
        &[
            col("Model"),
            num("Passes"),
            num("Input tokens"),
            num("Output tokens"),
            num("Cost"),
        ],
        models
            .into_iter()
            .map(|(model, passes, input, output, cost)| {
                vec![
                    esc(&model).to_string(),
                    count(passes),
                    count(input),
                    count(output),
                    money(cost),
                ]
            }),
        "No AI use this month.",
    ));

    body.push_str("<h2>By user</h2>");
    let users: Vec<(Option<String>, Option<String>, i64, i64, f64)> = sqlx::query_as(&format!(
        "SELECT u.id, u.email, count(*), SUM(a.input_tokens + a.output_tokens), SUM(a.cost_usd)
           FROM ai_usage a LEFT JOIN users u ON u.id = a.user_id
          WHERE {}
          GROUP BY a.user_id ORDER BY SUM(a.cost_usd) DESC LIMIT 50",
        within("a.created_at")
    ))
    .fetch_all(db)
    .await?;
    body.push_str(&table(
        &[col("User"), num("Passes"), num("Tokens"), num("Cost")],
        users.into_iter().map(|(id, email, passes, tokens, cost)| {
            vec![
                user_link(id.as_deref(), email.as_deref()),
                count(passes),
                count(tokens),
                money(cost),
            ]
        }),
        "No AI use this month.",
    ));

    body.push_str("<h2>By day</h2>");
    let days: Vec<(String, i64, i64, f64)> = sqlx::query_as(&format!(
        "SELECT date(created_at), count(*), SUM(input_tokens + output_tokens), SUM(cost_usd)
           FROM ai_usage WHERE {in_month} GROUP BY date(created_at) ORDER BY 1 DESC"
    ))
    .fetch_all(db)
    .await?;
    body.push_str(&table(
        &[col("Day (UTC)"), num("Passes"), num("Tokens"), num("Cost")],
        days.into_iter().map(|(day, passes, tokens, cost)| {
            vec![
                esc(&day).to_string(),
                count(passes),
                count(tokens),
                money(cost),
            ]
        }),
        "No AI use this month.",
    ));

    body.push_str("<h2>Latest passes</h2>");
    body.push_str(&passes(db, None, Some(&month), 50).await?);
    body.push_str(
        "<p class=muted>Recorded from the ai_usage ledger: passes before it existed are not \
         counted.</p>",
    );

    Ok(page("AI spend", Nav::Usage, None, &body))
}

/// One month of the rollup, as the query below returns it.
struct MonthRow {
    month: String,
    users: i64,
    passes: i64,
    turns: i64,
    input: i64,
    output: i64,
    cache_read: i64,
    /// Cost per feature, in `Feature::ALL` order.
    by_feature: [f64; 4],
    cost: f64,
    unpriced: i64,
}

/// Every month from the first ledger row to this one, newest first; a month
/// without AI use is a row of zeros rather than a gap.
pub async fn usage_months(State(state): State<AppState>) -> PageResult<Html<String>> {
    let db = &state.db;
    type Row = (
        String,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        f64,
        f64,
        f64,
        f64,
        f64,
        i64,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "WITH RECURSIVE months(m) AS (
             SELECT date(MIN(created_at), 'start of month') FROM ai_usage
             UNION ALL
             SELECT date(m, '+1 month') FROM months WHERE m < date('now', 'start of month')
         )
         SELECT strftime('%Y-%m', m), count(DISTINCT a.user_id), count(a.id),
                COALESCE(SUM(a.turns), 0), COALESCE(SUM(a.input_tokens), 0),
                COALESCE(SUM(a.output_tokens), 0), COALESCE(SUM(a.cache_read_tokens), 0),
                COALESCE(SUM(CASE WHEN a.feature = 'review' THEN a.cost_usd END), 0.0),
                COALESCE(SUM(CASE WHEN a.feature = 'note_chat' THEN a.cost_usd END), 0.0),
                COALESCE(SUM(CASE WHEN a.feature = 'plan_chat' THEN a.cost_usd END), 0.0),
                COALESCE(SUM(CASE WHEN a.feature = 'draft' THEN a.cost_usd END), 0.0),
                COALESCE(SUM(a.cost_usd), 0.0), COALESCE(SUM(a.unpriced_turns), 0)
           FROM months
           LEFT JOIN ai_usage a ON a.created_at >= m AND a.created_at < date(m, '+1 month')
          WHERE m IS NOT NULL
          GROUP BY m ORDER BY m DESC",
    )
    .fetch_all(db)
    .await?;
    let months: Vec<MonthRow> = rows
        .into_iter()
        .map(
            |(
                month,
                users,
                passes,
                turns,
                input,
                output,
                cache_read,
                r,
                n,
                p,
                d,
                cost,
                unpriced,
            )| {
                MonthRow {
                    month,
                    users,
                    passes,
                    turns,
                    input,
                    output,
                    cache_read,
                    by_feature: [r, n, p, d],
                    cost,
                    unpriced,
                }
            },
        )
        .collect();

    // This month so far, and at its pace for the whole month.
    let (month_cost, elapsed): (f64, f64) = sqlx::query_as(&format!(
        "SELECT COALESCE((SELECT SUM(cost_usd) FROM ai_usage WHERE created_at >= {MONTH_START}), 0.0),
                (julianday('now') - julianday('now', 'start of month'))
                  / (julianday('now', 'start of month', '+1 month')
                     - julianday('now', 'start of month'))"
    ))
    .fetch_one(db)
    .await?;
    let total: f64 = months.iter().map(|m| m.cost).sum();
    let spent: Vec<&MonthRow> = months.iter().filter(|m| m.passes > 0).collect();
    let average = if spent.is_empty() {
        0.0
    } else {
        total / spent.len() as f64
    };

    let mut body = String::from(
        "<p class=filters><a href=\"/usage\">This month in detail</a><strong>By month</strong></p>",
    );
    body.push_str(&tiles(&[
        ("All time", cents(total)),
        ("This month so far", cents(month_cost)),
        (
            "This month, on pace for",
            if elapsed > 0.0 {
                cents(month_cost / elapsed)
            } else {
                cents(month_cost)
            },
        ),
        ("Average month with use", cents(average)),
        ("Months with use", count(spent.len() as i64)),
    ]));

    let peak = months.iter().map(|m| m.cost).fold(0.0, f64::max);
    let cols = [
        col("Month"),
        num("Users"),
        num("Passes"),
        num("Turns"),
        num("Input tokens"),
        num("Output tokens"),
        num("Cache-read tokens"),
        num("Review"),
        num("Note chat"),
        num("Plan chat"),
        num("Draft"),
        num("Cost"),
        num("vs. prior"),
        col(""),
    ];
    let mut rendered: Vec<Vec<String>> = months
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let prior = months.get(i + 1).map(|p| p.cost);
            let change = match prior {
                Some(prior) if prior > 0.0 => {
                    format!("{:+.0}%", (m.cost - prior) / prior * 100.0)
                }
                _ => opt(None),
            };
            let cost = if m.unpriced > 0 {
                format!(
                    "{} <span class=muted title=\"turns without a price\">+{}?</span>",
                    cents(m.cost),
                    m.unpriced
                )
            } else {
                cents(m.cost)
            };
            let width = if peak > 0.0 {
                m.cost / peak * 100.0
            } else {
                0.0
            };
            let mut row = vec![
                format!("<a href=\"/usage?month={0}\">{0}</a>", esc(&m.month)),
                count(m.users),
                count(m.passes),
                count(m.turns),
                count(m.input),
                count(m.output),
                count(m.cache_read),
            ];
            row.extend(m.by_feature.iter().map(|c| cents(*c)));
            row.extend([
                cost,
                change,
                format!("<div class=bar><span style=\"width:{width:.1}%\"></span></div>"),
            ]);
            row
        })
        .collect();
    if !months.is_empty() {
        let sum = |f: fn(&MonthRow) -> i64| count(months.iter().map(f).sum());
        let mut row = vec![
            "<strong>Total</strong>".to_owned(),
            opt(None),
            sum(|m| m.passes),
            sum(|m| m.turns),
            sum(|m| m.input),
            sum(|m| m.output),
            sum(|m| m.cache_read),
        ];
        row.extend((0..4).map(|f| cents(months.iter().map(|m| m.by_feature[f]).sum())));
        row.extend([
            format!("<strong>{}</strong>", cents(total)),
            opt(None),
            String::new(),
        ]);
        rendered.push(row);
    }
    body.push_str(&table(&cols, rendered, "No AI passes recorded yet."));
    body.push_str(
        "<p class=muted>Months are calendar months, UTC. Users counts accounts that still \
         exist. Recorded from the ai_usage ledger: passes before it existed are not counted.</p>",
    );

    Ok(page("AI spend by month", Nav::Usage, None, &body))
}

// ── subscriptions ───────────────────────────────────────────────────────────

pub async fn subscriptions(State(state): State<AppState>) -> PageResult<Html<String>> {
    let rows: Vec<(String, String, String, String, String, String, i64, bool)> =
        sqlx::query_as(&format!(
            "SELECT u.id, u.email, b.provider, b.subscription_id, b.state,
                    datetime(b.access_until, 'unixepoch'), b.revision, {SUBSCRIBED}
               FROM subscriptions b JOIN users u ON u.id = b.user_id
              ORDER BY b.access_until DESC"
        ))
        .fetch_all(&state.db)
        .await?;
    let mut body = format!("<p class=muted>{}</p>", esc(&access_line(&state)));
    body.push_str(&table(
        &[
            col("User"),
            col("Provider"),
            col("Subscription"),
            col("State"),
            col("Access until (UTC)"),
            num("Revision"),
            col("Grants Pro now"),
        ],
        rows.into_iter().map(
            |(id, email, provider, sub_id, sub_state, until, revision, now)| {
                vec![
                    user_link(Some(&id), Some(&email)),
                    esc(&provider).to_string(),
                    format!("<code>{}</code>", esc(&sub_id)),
                    esc(&sub_state).to_string(),
                    esc(&until).to_string(),
                    count(revision),
                    yes_no(now).into(),
                ]
            },
        ),
        "No subscriptions.",
    ));
    Ok(page("Subscriptions", Nav::Subscriptions, None, &body))
}

// ── inbox ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct InboxQuery {
    status: Option<String>,
    notice: Option<String>,
}

/// The message statuses, as `contact_messages.status`.
pub const MESSAGE_STATUSES: [&str; 3] = ["new", "reviewed", "closed"];

pub async fn inbox(
    State(state): State<AppState>,
    Query(q): Query<InboxQuery>,
) -> PageResult<Html<String>> {
    let status = q
        .status
        .as_deref()
        .filter(|s| *s == "all" || MESSAGE_STATUSES.contains(s))
        .unwrap_or("new");
    let mut body = String::from("<p class=filters>");
    for option in MESSAGE_STATUSES.iter().chain(&["all"]) {
        if *option == status {
            let _ = write!(body, "<strong>{option}</strong>");
        } else {
            let _ = write!(body, "<a href=\"/inbox?status={option}\">{option}</a>");
        }
    }
    body.push_str("</p>");
    let back = format!("/inbox?status={status}");
    body.push_str(&messages(&state.db, None, Some(status), &back).await?);
    Ok(page(
        "Inbox",
        Nav::Inbox,
        notice_text(q.notice.as_deref()),
        &body,
    ))
}

/// Contact messages, for one user or by status, each with buttons to move
/// it to the other statuses that come back to `back`.
async fn messages(
    db: &Db,
    user: Option<&str>,
    status: Option<&str>,
    back: &str,
) -> PageResult<String> {
    let status = status.filter(|s| *s != "all");
    let rows: Vec<(i64, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT m.id, u.id, u.email, m.topic, m.message, m.status, m.created_at
           FROM contact_messages m JOIN users u ON u.id = m.user_id
          WHERE (?1 IS NULL OR m.user_id = ?1) AND (?2 IS NULL OR m.status = ?2)
          ORDER BY m.created_at DESC, m.id DESC LIMIT 200",
    )
    .bind(user)
    .bind(status)
    .fetch_all(db)
    .await?;
    Ok(table(
        &[
            col("Received (UTC)"),
            col("From"),
            col("Topic"),
            col("Message"),
            col("Status"),
            col(""),
        ],
        rows.into_iter()
            .map(|(id, user_id, email, topic, message, current, created)| {
                let mut buttons = String::new();
                for next in MESSAGE_STATUSES.iter().filter(|s| **s != current) {
                    let _ = write!(
                        buttons,
                        "<form class=inline method=post action=\"/inbox/{id}/status\">\
                         <input type=hidden name=status value={next}>\
                         <input type=hidden name=back value=\"{}\">\
                         <button>Mark {next}</button></form> ",
                        esc(back)
                    );
                }
                vec![
                    esc(&created).to_string(),
                    user_link(Some(&user_id), Some(&email)),
                    esc(&topic.replace('_', " ")).to_string(),
                    format!("<div class=msg>{}</div>", esc(&message)),
                    esc(&current).to_string(),
                    buttons,
                ]
            }),
        "No messages.",
    ))
}

#[cfg(test)]
mod tests {
    use super::parse_month;

    #[test]
    fn months_parse_strictly() {
        assert_eq!(
            parse_month("2026-10").map(|d| d.to_string()),
            Some("2026-10-01".into())
        );
        for bad in ["2026-13", "2026-1", "26-10", "2026/10", "", "2026-10-01"] {
            assert!(parse_month(bad).is_none(), "{bad}");
        }
    }
}
