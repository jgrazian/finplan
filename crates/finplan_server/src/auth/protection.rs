//! Explicit mutation origins and bounded expensive authentication work.
use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

pub async fn mutation_origin(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        let origin = req.headers().get("origin");
        let trusted = origin
            .and_then(|o| o.to_str().ok())
            .is_some_and(|o| state.config.cors_origins.iter().any(|allowed| allowed == o));
        // Bearer clients without cookies do not carry ambient browser authority.
        let bearer = req
            .headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|s| s.starts_with("Bearer "))
            && !req.headers().contains_key("cookie");
        if (origin.is_some() && !trusted) || (state.config.hosted && !trusted && !bearer) {
            return (StatusCode::FORBIDDEN, "request origin is not permitted").into_response();
        }
    }
    next.run(req).await
}

struct Window {
    started: Instant,
    attempts: u32,
}
static WINDOWS: OnceLock<Mutex<HashMap<String, Window>>> = OnceLock::new();
static HASH_SLOTS: OnceLock<std::sync::Arc<tokio::sync::Semaphore>> = OnceLock::new();

/// Process-local bounded limiter. Never trusts forwarded headers. Global cap also
/// protects deployments that have not supplied a trusted peer address.
pub async fn auth_throttle(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.config.hosted
        || req.method() != Method::POST
        || !req.uri().path().starts_with("/api/auth/")
    {
        return next.run(req).await;
    }
    let peer = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|p| p.0.ip().to_string())
        .unwrap_or_else(|| "unknown-peer".into());
    let limited = {
        let mut windows = WINDOWS.get_or_init(Default::default).lock().unwrap();
        let now = Instant::now();
        windows.retain(|_, w| now.duration_since(w.started) < Duration::from_secs(60));
        if windows.len() >= 4096 && !windows.contains_key(&peer) {
            true
        } else {
            let window = windows.entry(peer).or_insert(Window {
                started: now,
                attempts: 0,
            });
            window.attempts += 1;
            window.attempts > 30
        }
    };
    if limited {
        state.telemetry.auth_throttled();
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "60")],
            "Too many attempts. Retry in one minute.",
        )
            .into_response();
    }
    let slots = HASH_SLOTS.get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4)));
    let Ok(_permit) = slots.clone().try_acquire_owned() else {
        state.telemetry.auth_throttled();
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "2")],
            "Authentication busy. Retry shortly.",
        )
            .into_response();
    };
    next.run(req).await
}

pub fn account_attempt(state: &AppState, email: &str) -> crate::error::ApiResult<()> {
    let key = format!("account:{}", super::session::hash_token(email));
    let mut windows = WINDOWS.get_or_init(Default::default).lock().unwrap();
    let now = Instant::now();
    windows.retain(|_, w| now.duration_since(w.started) < Duration::from_secs(60));
    if windows.len() >= 4096 && !windows.contains_key(&key) {
        state.telemetry.auth_throttled();
        return Err(crate::error::ApiError::Conflict(
            "Too many attempts. Retry in one minute.".into(),
        ));
    }
    let w = windows.entry(key).or_insert(Window {
        started: now,
        attempts: 0,
    });
    w.attempts += 1;
    if w.attempts > 10 {
        state.telemetry.auth_throttled();
        return Err(crate::error::ApiError::Conflict(
            "Too many attempts. Retry in one minute.".into(),
        ));
    }
    Ok(())
}

/// Guests one address may create per hour, and all addresses together per
/// minute (spec 17). Creating a guest is free and unauthenticated, so this is
/// the only thing between a script and a table of throwaway users.
const GUESTS_PER_IP_PER_HOUR: u32 = 5;
const GUESTS_PER_MINUTE: u32 = 60;
const GUEST_IP_WINDOW: Duration = Duration::from_secs(3600);
const GUEST_GLOBAL_WINDOW: Duration = Duration::from_secs(60);
/// Distinct addresses tracked at once; past this, unseen addresses are refused
/// rather than letting the map grow.
const GUEST_WINDOW_KEYS: usize = 4096;

/// Kept apart from `WINDOWS`, which forgets everything after 60 seconds.
static GUEST_WINDOWS: OnceLock<Mutex<HashMap<String, Window>>> = OnceLock::new();
static GUEST_GLOBAL: OnceLock<Mutex<Window>> = OnceLock::new();

/// The address the request arrived from, as `auth_throttle` reads it. Forwarded
/// headers are never trusted.
#[must_use]
pub fn peer_of(connect: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>) -> String {
    connect
        .map(|p| p.0.ip().to_string())
        .unwrap_or_else(|| "unknown-peer".into())
}

/// Count one guest creation against the per-address and process-wide limits.
pub fn guest_attempt(state: &AppState, peer: &str) -> crate::error::ApiResult<()> {
    let limited = |retry_after| {
        state.telemetry.auth_throttled();
        Err(crate::error::ApiError::rate_limited(
            "Too many guest sessions. Try again later or sign in.",
            retry_after,
        ))
    };
    let now = Instant::now();
    {
        let mut windows = GUEST_WINDOWS.get_or_init(Default::default).lock().unwrap();
        windows.retain(|_, w| now.duration_since(w.started) < GUEST_IP_WINDOW);
        let key = format!("guest:{peer}");
        if windows.len() >= GUEST_WINDOW_KEYS && !windows.contains_key(&key) {
            return limited(3600);
        }
        let w = windows.entry(key).or_insert(Window {
            started: now,
            attempts: 0,
        });
        w.attempts += 1;
        if w.attempts > GUESTS_PER_IP_PER_HOUR {
            let wait = GUEST_IP_WINDOW.saturating_sub(now.duration_since(w.started));
            return limited(wait.as_secs().max(1));
        }
    }
    let mut global = GUEST_GLOBAL
        .get_or_init(|| {
            Mutex::new(Window {
                started: now,
                attempts: 0,
            })
        })
        .lock()
        .unwrap();
    if now.duration_since(global.started) >= GUEST_GLOBAL_WINDOW {
        *global = Window {
            started: now,
            attempts: 0,
        };
    }
    global.attempts += 1;
    if global.attempts > GUESTS_PER_MINUTE {
        return limited(60);
    }
    Ok(())
}
