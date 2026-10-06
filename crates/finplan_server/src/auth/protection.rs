//! Explicit mutation origins and bounded expensive authentication work.
use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
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

/// Process-local bounded limiter, keyed on [`client_addr`]. Global cap also
/// protects deployments that have not supplied a trusted peer address.
pub async fn auth_throttle(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.config.hosted
        || req.method() != Method::POST
        || !req.uri().path().starts_with("/api/auth/")
    {
        return next.run(req).await;
    }
    let peer = client_addr(
        &state.config.trusted_proxies,
        req.extensions()
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|c| c.0),
        req.headers(),
    );
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

/// The visitor's address, which every limiter here keys on.
///
/// The connecting peer, unless it is one of `trusted` (the local Next.js
/// proxy in production). Then `X-Forwarded-For` is read right to left,
/// skipping trusted hops: the first other entry was appended by a proxy we
/// believe (Cloudflare appends the visitor; Next.js sets the header only when
/// absent), whereas anything further left is the client's own say-so.
#[must_use]
pub fn client_addr(trusted: &[IpAddr], connect: Option<SocketAddr>, headers: &HeaderMap) -> String {
    let Some(peer) = connect.map(|c| c.ip()) else {
        return "unknown-peer".into();
    };
    if !trusted.contains(&peer) {
        return peer.to_string();
    }
    headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .rev()
        .filter_map(|hop| hop.trim().parse::<IpAddr>().ok())
        .find(|ip| !trusted.contains(ip))
        .unwrap_or(peer)
        .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    const PROXY: &str = "127.0.0.1:50000";

    fn addr(trusted: &[&str], peer: &str, xff: &[&str]) -> String {
        let trusted: Vec<IpAddr> = trusted.iter().map(|t| t.parse().unwrap()).collect();
        let mut headers = HeaderMap::new();
        for v in xff {
            headers.append("x-forwarded-for", v.parse().unwrap());
        }
        client_addr(&trusted, Some(peer.parse().unwrap()), &headers)
    }

    #[test]
    fn untrusted_peer_ignores_forwarded_header() {
        assert_eq!(addr(&[], PROXY, &["203.0.113.9"]), "127.0.0.1");
        assert_eq!(
            addr(&["127.0.0.1"], "198.51.100.4:1", &["203.0.113.9"]),
            "198.51.100.4"
        );
    }

    #[test]
    fn trusted_peer_yields_rightmost_untrusted_hop() {
        let trusted = ["127.0.0.1", "::1"];
        assert_eq!(addr(&trusted, PROXY, &["203.0.113.9"]), "203.0.113.9");
        // A visitor's forged prefix is skipped: the proxy appended the real one.
        assert_eq!(
            addr(&trusted, PROXY, &["10.0.0.1, 203.0.113.9"]),
            "203.0.113.9"
        );
        assert_eq!(
            addr(&trusted, PROXY, &["203.0.113.9", "::1"]),
            "203.0.113.9"
        );
    }

    #[test]
    fn trusted_peer_without_usable_header_is_itself() {
        let trusted = ["127.0.0.1"];
        assert_eq!(addr(&trusted, PROXY, &[]), "127.0.0.1");
        assert_eq!(addr(&trusted, PROXY, &["not-an-ip"]), "127.0.0.1");
    }
}
