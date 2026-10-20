//! The admin listener (`--admin-bind`): a local-only HTML tool over the
//! database — users and their plans, subscriptions and monthly allowances, AI
//! spend from the `ai_usage` ledger (`usage`), and the contact inbox — with
//! the few writes an operator needs: comp Pro, reset a month's allowances,
//! sign a user out, delete one, and triage a message.
//!
//! There is no sign-in; what keeps it private is where it listens. It binds
//! loopback only (`ServerConfig::validate`); it answers only requests whose
//! `Host` is a loopback name, so a web page cannot reach it by rebinding a DNS
//! name to 127.0.0.1; and it takes a POST only from its own origin, so a page
//! the operator has open elsewhere cannot submit its forms. Pages run no
//! script, and the CSP says so. On a remote server, reach it through an SSH
//! tunnel (`ssh -L 8090:127.0.0.1:8090 host`).

mod actions;
mod html;
mod pages;

use std::net::IpAddr;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(pages::overview))
        .route("/users", get(pages::users))
        .route("/users/{id}", get(pages::user))
        .route("/users/{id}/pro", post(actions::grant_pro))
        .route("/users/{id}/pro/revoke", post(actions::revoke_pro))
        .route("/users/{id}/quotas/reset", post(actions::reset_quotas))
        .route("/users/{id}/sessions/revoke", post(actions::sign_out))
        .route("/users/{id}/delete", post(actions::delete_user))
        .route("/usage", get(pages::usage))
        .route("/usage/months", get(pages::usage_months))
        .route("/subscriptions", get(pages::subscriptions))
        .route("/inbox", get(pages::inbox))
        .route("/inbox/{id}/status", post(actions::set_message_status))
        .layer(middleware::from_fn(guard))
        .with_state(state)
}

/// Refuse a request that did not come from a browser pointed at this
/// listener on loopback, and keep every answer out of caches and frames.
async fn guard(request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .or_else(|| request.uri().authority().map(|a| a.as_str()))
        .filter(|h| loopback_host(h))
        .map(str::to_owned);
    let Some(host) = host else {
        tracing::warn!(event = "admin.refused", reason = "host");
        return refused("This tool answers only on localhost.");
    };
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|o| o.to_str().ok());
        if origin != Some(format!("http://{host}").as_str()) {
            tracing::warn!(event = "admin.refused", reason = "origin");
            return refused("Changes are accepted only from this tool's own pages.");
        }
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; \
             frame-ancestors 'none'; base-uri 'none'",
        ),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    // Not `no-referrer`: that would make browsers send `Origin: null` on the
    // pages' own form posts, which the origin check above refuses.
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    response
}

fn refused(message: &str) -> Response {
    (StatusCode::FORBIDDEN, message.to_owned()).into_response()
}

/// `localhost`, or a loopback address, with or without a port.
fn loopback_host(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        // [::1] or [::1]:8090
        match rest.split_once(']') {
            Some((name, port)) if port.is_empty() || valid_port(port) => name,
            _ => return false,
        }
    } else {
        match host.rsplit_once(':') {
            Some((name, port)) if valid_port(port) => name,
            Some(_) => return false,
            None => host,
        }
    };
    name.eq_ignore_ascii_case("localhost")
        || name.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn valid_port(port: &str) -> bool {
    port.strip_prefix(':')
        .unwrap_or(port)
        .parse::<u16>()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::loopback_host;

    #[test]
    fn only_loopback_hosts_pass() {
        for host in [
            "localhost",
            "localhost:8090",
            "LOCALHOST:1",
            "127.0.0.1",
            "127.0.0.1:8090",
            "127.8.9.1:80",
            "[::1]",
            "[::1]:8090",
        ] {
            assert!(loopback_host(host), "{host}");
        }
        for host in [
            "evil.example",
            "evil.example:8090",
            "localhost.evil.example",
            "10.0.0.1:8090",
            "[::2]:8090",
            "localhost:notaport",
            "",
        ] {
            assert!(!loopback_host(host), "{host}");
        }
    }
}
