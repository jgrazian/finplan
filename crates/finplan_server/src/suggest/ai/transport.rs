//! One model request: the only place review notes touch the network.
//!
//! OpenRouter is the only provider, reached through `openrouter-rs` at its
//! Anthropic-compatible `/messages` endpoint. The request and reply are that
//! crate's typed Messages shapes; the call sits behind [`Transport`] so the
//! conversation loop can be driven by a scripted double in tests. Failures
//! come back as [`TransportError`], so the caller decides what is worth
//! retrying.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use openrouter_rs::OpenRouterClient;
use openrouter_rs::error::{ApiErrorKind, OpenRouterError};
use openrouter_rs::types::{DataCollectionPolicy, ProviderPreferences};

pub use openrouter_rs::api::messages::{
    AnthropicMessagesRequest as Request, AnthropicMessagesResponse as Reply,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq)]
pub enum TransportError {
    /// A non-2xx reply.
    Status {
        status: u16,
        /// What kind of failure OpenRouter reported: `moderation` or
        /// `provider:<name>`; `None` for a generic API error.
        error_type: Option<String>,
        message: String,
    },
    /// The request never got a reply: connect, TLS, timeout.
    Network(String),
    /// A 2xx reply that could not be read.
    Decode(String),
    /// The request could not be built or sent at all: configuration.
    Request(String),
}

impl TransportError {
    /// Worth sending again: rate limits, overload, server and upstream
    /// provider errors, and dropped connections. Other 4xx replies will fail
    /// the same way twice.
    pub fn retryable(&self) -> bool {
        match self {
            TransportError::Status { status, .. } => {
                matches!(status, 408 | 429 | 500 | 502 | 503 | 504 | 529)
            }
            TransportError::Network(_) => true,
            TransportError::Decode(_) | TransportError::Request(_) => false,
        }
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::Status {
                status,
                error_type,
                message,
            } => write!(
                f,
                "OpenRouter {status} {}: {message}",
                error_type.as_deref().unwrap_or("error")
            ),
            TransportError::Network(message) => write!(f, "OpenRouter unreachable: {message}"),
            TransportError::Decode(message) => write!(f, "OpenRouter reply unreadable: {message}"),
            TransportError::Request(message) => {
                write!(f, "OpenRouter request not sent: {message}")
            }
        }
    }
}

impl From<OpenRouterError> for TransportError {
    fn from(error: OpenRouterError) -> Self {
        match error {
            OpenRouterError::Api(context) => TransportError::Status {
                status: context.status.as_u16(),
                error_type: match &context.kind {
                    ApiErrorKind::Generic => None,
                    ApiErrorKind::Moderation { .. } => Some("moderation".into()),
                    ApiErrorKind::Provider { provider_name, .. } => {
                        Some(format!("provider:{provider_name}"))
                    }
                },
                message: context.message.clone(),
            },
            OpenRouterError::HttpRequest(e) => TransportError::Network(e.message().to_owned()),
            OpenRouterError::Io(e) => TransportError::Network(e.to_string()),
            // The crate reports a 2xx body it could not parse as `Unknown`.
            OpenRouterError::Serialization(e) => TransportError::Decode(e.to_string()),
            OpenRouterError::Unknown(message) => TransportError::Decode(message),
            OpenRouterError::ConfigError(message) => TransportError::Request(message),
            OpenRouterError::KeyNotConfigured => {
                TransportError::Request("API key not configured".into())
            }
            OpenRouterError::UninitializedFieldError(e) => TransportError::Request(e.to_string()),
        }
    }
}

/// OpenRouter's provider routing for a request. Every request asks only for
/// providers that honour every parameter (one that dropped `tools` would
/// answer in prose the loop cannot use) and that do not collect data.
///
/// A draft's request carries statements and tax returns (already redacted, but
/// images may not be), so it also sets `zdr`: only zero-data-retention
/// endpoints may serve it. Review requests keep the plain rule.
pub fn provider_preferences(zero_data_retention: bool) -> ProviderPreferences {
    let mut provider = ProviderPreferences::default();
    provider.require_parameters = Some(true);
    provider.data_collection = Some(DataCollectionPolicy::Deny);
    if zero_data_retention {
        provider.zdr = Some(true);
    }
    provider
}

/// A model's listed prices, in US dollars per token. Used to estimate a
/// request's cost when the reply does not report one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPrice {
    pub prompt: f64,
    pub completion: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl ModelPrice {
    /// Dollars for one request's tokens. `input` excludes cache reads and
    /// writes, as the Messages usage reports it.
    pub fn cost(&self, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
        input as f64 * self.prompt
            + output as f64 * self.completion
            + cache_read as f64 * self.cache_read
            + cache_write as f64 * self.cache_write
    }
}

pub trait Transport: Send + Sync {
    /// Send one Messages request and return the decoded reply.
    fn send<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Reply, TransportError>>;

    /// `model`'s listed prices, when the provider publishes them. Only
    /// consulted when a reply carries no cost of its own.
    fn price<'a>(
        &'a self,
        _model: &'a str,
    ) -> BoxFuture<'a, Result<Option<ModelPrice>, TransportError>> {
        Box::pin(async { Ok(None) })
    }
}

/// OpenRouter settings the transport needs.
pub struct OpenRouterSettings<'a> {
    pub base_url: &'a str,
    pub api_key: &'a str,
    /// `X-Title`, which names the app in OpenRouter's dashboards.
    pub app_title: &'a str,
    /// `HTTP-Referer`, OpenRouter's app attribution; optional.
    pub referer: Option<&'a str>,
    pub timeout: Duration,
}

/// The real thing: OpenRouter's `/messages` through `openrouter-rs`.
pub struct OpenRouterTransport {
    client: OpenRouterClient,
}

impl OpenRouterTransport {
    pub fn new(settings: &OpenRouterSettings<'_>) -> Result<Self, String> {
        // The crate's own client has no timeout; a hung provider must not
        // hold a review open forever.
        let http = reqwest::Client::builder()
            .timeout(settings.timeout)
            .build()
            .map_err(|e| format!("building the HTTP client: {e}"))?;
        let mut builder = OpenRouterClient::builder();
        builder
            .base_url(settings.base_url.trim_end_matches('/'))
            .api_key(settings.api_key)
            .x_title(settings.app_title)
            .http_client(http);
        if let Some(referer) = settings.referer {
            builder.http_referer(referer);
        }
        // The builder's error cannot carry the key: it only reports fields.
        let client = builder
            .build()
            .map_err(|e| format!("building the OpenRouter client: {e}"))?;
        Ok(Self { client })
    }
}

impl Transport for OpenRouterTransport {
    fn send<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Reply, TransportError>> {
        Box::pin(async move {
            self.client
                .messages()
                .create(request)
                .await
                .map_err(TransportError::from)
        })
    }

    fn price<'a>(
        &'a self,
        model: &'a str,
    ) -> BoxFuture<'a, Result<Option<ModelPrice>, TransportError>> {
        Box::pin(async move {
            let models = self
                .client
                .list_models()
                .await
                .map_err(TransportError::from)?;
            Ok(models
                .iter()
                .find(|m| m.id == model || m.canonical_slug.as_deref() == Some(model))
                .and_then(|m| price_of(&m.pricing)))
        })
    }
}

/// OpenRouter lists prices as decimal strings of dollars per token; a model
/// without prompt and completion prices has no estimate.
fn price_of(pricing: &openrouter_rs::api::models::Pricing) -> Option<ModelPrice> {
    let parse = |value: Option<&str>| {
        value
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
    };
    let prompt = parse(Some(&pricing.prompt))?;
    let completion = parse(Some(&pricing.completion))?;
    Some(ModelPrice {
        prompt,
        completion,
        // A model that lists no cache price bills cached tokens as prompt tokens.
        cache_read: parse(pricing.input_cache_read.as_deref()).unwrap_or(prompt),
        cache_write: parse(pricing.input_cache_write.as_deref()).unwrap_or(prompt),
    })
}

#[cfg(test)]
pub(super) fn price_from_json(value: serde_json::Value) -> Option<ModelPrice> {
    let pricing: openrouter_rs::api::models::Pricing = serde_json::from_value(value).ok()?;
    price_of(&pricing)
}
