use std::time::Instant;

use axum::extract::{MatchedPath, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Router, routing::get};
use tracing::Instrument;

use super::context::REQUEST_CONTEXT;
use super::{Component, ErrorClass, RequestContext, Telemetry};

/// Attached by ApiError without carrying the raw underlying error into logs.
#[derive(Clone, Copy)]
pub(crate) struct HttpFailure(pub ErrorClass);

pub async fn request_telemetry(
    State(telemetry): State<Telemetry>,
    mut request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let _active = telemetry.request_started();
    let method = normalized_method(request.method());
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_else(|| "unmatched".to_owned());
    let context = RequestContext::new(method, route);
    request.extensions_mut().insert(context.clone());
    let span = tracing::info_span!("http.request", request_id = %context.request_id,
        method, route = %context.route, user_id = tracing::field::Empty);
    REQUEST_CONTEXT
        .scope(
            context.clone(),
            async {
                let mut response = next.run(request).await;
                response.headers_mut().insert(
                    "x-request-id",
                    HeaderValue::from_str(&context.request_id).expect("UUID is a valid header"),
                );
                let status = response.status();
                let elapsed = started.elapsed().as_secs_f64();
                telemetry.request_completed(method, &context.route, status.as_u16(), elapsed);
                if status.is_server_error() {
                    let class = response
                        .extensions()
                        .get::<HttpFailure>()
                        .map_or(ErrorClass::Internal, |f| f.0);
                    telemetry.count_error(Component::Http, class);
                    tracing::error!(
                        event = "request.failed",
                        class = class.as_str(),
                        status = status.as_u16()
                    );
                } else if status.is_client_error() {
                    tracing::debug!(
                        event = "request.rejected",
                        status = status.as_u16(),
                        reason = rejection_reason(status)
                    );
                }
                if elapsed >= 1.0 || !matches!(method, "GET" | "HEAD" | "OPTIONS") {
                    tracing::info!(
                        event = "request.completed",
                        status = status.as_u16(),
                        duration_seconds = elapsed
                    );
                } else {
                    tracing::debug!(
                        event = "request.completed",
                        status = status.as_u16(),
                        duration_seconds = elapsed
                    );
                }
                response
            }
            .instrument(span),
        )
        .await
}

fn normalized_method(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        Method::CONNECT => "CONNECT",
        Method::TRACE => "TRACE",
        _ => "other",
    }
}

fn rejection_reason(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
        StatusCode::CONFLICT => "conflict",
        StatusCode::UNPROCESSABLE_ENTITY => "unprocessable",
        StatusCode::TOO_MANY_REQUESTS => "throttled",
        _ => "invalid_request",
    }
}

/// This router belongs only on an explicitly configured private listener.
/// Scrapes are excluded from HTTP metrics and require no browser cookie.
pub fn metrics_router(telemetry: Telemetry) -> Router {
    Router::new()
        .route("/metrics", get(scrape))
        .with_state(telemetry)
}

async fn scrape(State(telemetry): State<Telemetry>) -> Response {
    match telemetry.encode() {
        Ok(body) => (
            [(
                header::CONTENT_TYPE,
                "application/openmetrics-text; version=1.0.0; charset=utf-8",
            )],
            body,
        )
            .into_response(),
        Err(_) => {
            telemetry.error(Component::Metrics, ErrorClass::Internal);
            (StatusCode::INTERNAL_SERVER_ERROR, "metrics encoding failed").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt;

    #[tokio::test]
    async fn requests_have_local_ids_and_bounded_paths_even_for_rejections() {
        let telemetry = Telemetry::new(1);
        let app = Router::new()
            .route("/things/{id}", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                telemetry.clone(),
                request_telemetry,
            ));
        let mut ids = Vec::new();
        for (method, uri) in [
            ("GET", "/things/secret-1?token=secret-query"),
            ("GET", "/things/secret-2"),
            ("POST", "/things/secret-3"),
            ("SECRET-METHOD", "/secret-path"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("x-request-id", "untrusted-id")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let id = response.headers()["x-request-id"]
                .to_str()
                .unwrap()
                .to_owned();
            assert!(uuid::Uuid::parse_str(&id).is_ok());
            assert!(!ids.contains(&id));
            ids.push(id);
        }
        let body = telemetry.encode().unwrap();
        assert!(body.contains("method=\"GET\",route=\"/things/{id}\",status=\"200\"} 2"));
        assert!(body.contains("method=\"POST\",route=\"/things/{id}\",status=\"405\"} 1"));
        assert!(body.contains("method=\"other\",route=\"unmatched\",status=\"404\"} 1"));
        assert!(!body.contains("secret"));
        assert!(body.contains("finplan_http_requests_in_flight 0"));
    }

    #[tokio::test]
    async fn scrapes_are_openmetrics_and_do_not_record_http_activity() {
        let telemetry = Telemetry::new(1);
        let response = metrics_router(telemetry.clone())
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/openmetrics-text; version=1.0.0; charset=utf-8"
        );
        let body = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.starts_with("# HELP finplan_http_requests"));
        assert!(body.ends_with("# EOF\n"));
        assert!(!body.contains("finplan_http_requests_total{"));
    }

    #[tokio::test]
    async fn aborted_request_releases_in_flight_gauge() {
        let telemetry = Telemetry::new(1);
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let signal = entered.clone();
        let app = Router::new()
            .route(
                "/blocked",
                get(move || async move {
                    signal.notify_one();
                    std::future::pending::<()>().await;
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                telemetry.clone(),
                request_telemetry,
            ));
        let request = tokio::spawn(
            app.oneshot(
                Request::builder()
                    .uri("/blocked")
                    .body(Body::empty())
                    .unwrap(),
            ),
        );
        entered.notified().await;
        assert!(
            telemetry
                .encode()
                .unwrap()
                .contains("finplan_http_requests_in_flight 1")
        );
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        assert!(
            telemetry
                .encode()
                .unwrap()
                .contains("finplan_http_requests_in_flight 0")
        );
    }
}
