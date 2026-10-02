//! One model request: the only place review notes touch the network.
//!
//! OpenRouter is the only provider, reached through `openrouter-rs` at its
//! Anthropic-compatible `/messages` endpoint. The request and reply are that
//! crate's typed Messages shapes; the call sits behind [`Transport`] so the
//! conversation loop can be driven by a scripted double in tests. Failures
//! come back as [`TransportError`], so the caller decides what is worth
//! retrying.
//!
//! Every request is streamed and folded back into one reply ([`Accumulator`]).
//! Nothing upstream sees the stream: it is here so a slow model that is still
//! writing is told apart from a provider that has stopped answering — the
//! first keeps the request alive, the second trips the idle timeout.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures_util::StreamExt;
use openrouter_rs::OpenRouterClient;
use openrouter_rs::api::messages::AnthropicMessagesStreamEvent as StreamEvent;
use openrouter_rs::error::{ApiErrorKind, OpenRouterError};
use openrouter_rs::types::{DataCollectionPolicy, ProviderPreferences};
use serde_json::{Map, Value};

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

/// What OpenRouter's model listing says about the configured model that
/// changes how it is asked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelInfo {
    /// Listed prices, for replies that carry no cost of their own.
    pub price: Option<ModelPrice>,
    /// Lists the `reasoning` parameter. OpenRouter maps the Messages
    /// `thinking` and `output_config.effort` onto it for every such model,
    /// not only Claude — and with `require_parameters` set, a request that
    /// carries them to a model without it would find no provider at all.
    pub reasons: bool,
}

pub trait Transport: Send + Sync {
    /// Send one Messages request and return the decoded reply.
    fn send<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Reply, TransportError>>;

    /// `model`'s listing; `None` when the provider does not list it.
    fn model_info<'a>(
        &'a self,
        _model: &'a str,
    ) -> BoxFuture<'a, Result<Option<ModelInfo>, TransportError>> {
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
    /// The most one request may take, streaming included.
    pub timeout: Duration,
    /// The longest silence a stream may keep — before its first event, or
    /// between two — before it is abandoned. OpenRouter pings a live stream,
    /// so silence means the request is lost, not slow.
    pub idle_timeout: Duration,
}

/// The real thing: OpenRouter's `/messages` through `openrouter-rs`.
pub struct OpenRouterTransport {
    client: OpenRouterClient,
    idle_timeout: Duration,
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
        Ok(Self {
            client,
            idle_timeout: settings.idle_timeout,
        })
    }
}

impl Transport for OpenRouterTransport {
    fn send<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Reply, TransportError>> {
        Box::pin(async move {
            let idle = self.idle_timeout;
            let silent = || {
                TransportError::Network(format!("no data from the model for {}s", idle.as_secs()))
            };
            let mut stream = tokio::time::timeout(idle, self.client.stream_messages(request))
                .await
                .map_err(|_| silent())??;
            let mut reply = Accumulator::default();
            loop {
                match tokio::time::timeout(idle, stream.next()).await {
                    Err(_) => return Err(silent()),
                    Ok(None) => return reply.finish(),
                    Ok(Some(event)) => {
                        if reply.push(event?.data)? {
                            return reply.finish();
                        }
                    }
                }
            }
        })
    }

    fn model_info<'a>(
        &'a self,
        model: &'a str,
    ) -> BoxFuture<'a, Result<Option<ModelInfo>, TransportError>> {
        Box::pin(async move {
            let models = self
                .client
                .list_models()
                .await
                .map_err(TransportError::from)?;
            Ok(models
                .iter()
                .find(|m| m.id == model || m.canonical_slug.as_deref() == Some(model))
                .map(|m| ModelInfo {
                    price: price_of(&m.pricing),
                    reasons: m.supported_parameters.iter().any(|p| p == "reasoning"),
                }))
        })
    }
}

/// A streamed `/messages` reply, folded back into the reply a non-streaming
/// request would have returned.
///
/// The message and its blocks are kept as JSON and only typed at the end, so
/// a block or delta this code has never heard of passes through intact
/// rather than failing the request: deltas it knows (`text_delta`,
/// `thinking_delta`, `signature_delta`, `input_json_delta`,
/// `citations_delta`) are applied, and anything else is ignored.
#[derive(Default)]
pub(crate) struct Accumulator {
    message: Option<Map<String, Value>>,
    blocks: Vec<Value>,
    /// Tool input arrives as fragments of one JSON text per block.
    partial_json: Vec<String>,
    stopped: bool,
}

impl Accumulator {
    /// Apply one event. `Ok(true)` once the message is complete.
    pub(crate) fn push(&mut self, event: StreamEvent) -> Result<bool, TransportError> {
        match event {
            StreamEvent::MessageStart { message } => {
                let Value::Object(mut message) = to_json(&*message)? else {
                    return Err(TransportError::Decode(
                        "message_start is not an object".into(),
                    ));
                };
                // Any blocks listed at the start are replayed as block events.
                message.remove("content");
                self.message = Some(message);
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let index = index as usize;
                if self.blocks.len() <= index {
                    self.blocks.resize(index + 1, Value::Null);
                    self.partial_json.resize(index + 1, String::new());
                }
                self.blocks[index] = to_json(&*content_block)?;
            }
            StreamEvent::ContentBlockDelta { index, delta } => {
                let index = index as usize;
                let block = self.blocks.get_mut(index).ok_or_else(|| {
                    TransportError::Decode(format!("delta for unopened block {index}"))
                })?;
                apply_delta(block, &mut self.partial_json[index], &delta);
            }
            StreamEvent::ContentBlockStop { index } => {
                let index = index as usize;
                let fragments = self
                    .partial_json
                    .get_mut(index)
                    .map(std::mem::take)
                    .unwrap_or_default();
                if let Some(Value::Object(block)) = self.blocks.get_mut(index)
                    && !fragments.trim().is_empty()
                {
                    let input = serde_json::from_str(&fragments).map_err(|e| {
                        TransportError::Decode(format!("tool input for block {index}: {e}"))
                    })?;
                    block.insert("input".into(), input);
                }
            }
            StreamEvent::MessageDelta { delta, usage } => {
                let message = self.message.get_or_insert_with(Map::new);
                if let Value::Object(delta) = delta {
                    for (key, value) in delta {
                        message.insert(key, value);
                    }
                }
                // Counts in a delta are running totals: they replace, and a
                // field the delta leaves out (the input count) is kept.
                if let Value::Object(usage) = usage {
                    let into = message
                        .entry("usage")
                        .or_insert_with(|| Value::Object(Map::new()));
                    if let Value::Object(into) = into {
                        for (key, value) in usage {
                            if !value.is_null() {
                                into.insert(key, value);
                            }
                        }
                    }
                }
            }
            StreamEvent::MessageStop { .. } => {
                self.stopped = true;
                return Ok(true);
            }
            StreamEvent::Error { error } => return Err(stream_error(&error)),
            // Ping, and whatever a later crate adds.
            _ => {}
        }
        Ok(false)
    }

    /// The reply, once `message_stop` has arrived; a stream cut off before it
    /// is a dropped connection, worth sending again.
    pub(crate) fn finish(self) -> Result<Reply, TransportError> {
        if !self.stopped {
            return Err(TransportError::Network(
                "the stream ended before the message did".into(),
            ));
        }
        let mut message = self
            .message
            .ok_or_else(|| TransportError::Decode("stream carried no message_start".into()))?;
        message.insert(
            "content".into(),
            Value::Array(self.blocks.into_iter().filter(|b| !b.is_null()).collect()),
        );
        serde_json::from_value(Value::Object(message))
            .map_err(|e| TransportError::Decode(format!("assembled reply: {e}")))
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<Value, TransportError> {
    serde_json::to_value(value).map_err(|e| TransportError::Decode(e.to_string()))
}

/// Append one delta to its block. Text-like deltas concatenate; a tool's
/// input is buffered in `partial_json` until the block stops.
fn apply_delta(block: &mut Value, partial_json: &mut String, delta: &Value) {
    let Value::Object(block) = block else { return };
    let append = |block: &mut Map<String, Value>, key: &str, more: Option<&str>| {
        if let Some(more) = more {
            let mut text = block
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            text.push_str(more);
            block.insert(key.into(), Value::String(text));
        }
    };
    match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => append(block, "text", delta["text"].as_str()),
        Some("thinking_delta") => append(block, "thinking", delta["thinking"].as_str()),
        Some("signature_delta") => append(block, "signature", delta["signature"].as_str()),
        Some("input_json_delta") => {
            partial_json.push_str(delta["partial_json"].as_str().unwrap_or_default());
        }
        Some("citations_delta") => {
            let citations = block
                .entry("citations")
                .or_insert_with(|| Value::Array(Vec::new()));
            if let (Value::Array(citations), Some(citation)) = (citations, delta.get("citation")) {
                citations.push(citation.clone());
            }
        }
        _ => {}
    }
}

/// A mid-stream `error` event, given the status its non-streaming twin would
/// have carried so [`TransportError::retryable`] judges it the same way.
fn stream_error(error: &Value) -> TransportError {
    let kind = error.get("type").and_then(Value::as_str);
    let status = match kind {
        Some("invalid_request_error") => 400,
        Some("authentication_error") => 401,
        Some("permission_error") => 403,
        Some("not_found_error") => 404,
        Some("request_too_large") => 413,
        Some("rate_limit_error") => 429,
        Some("overloaded_error") => 529,
        Some("timeout_error") => 504,
        Some("api_error") => 500,
        // Anything else is upstream failing mid-reply.
        _ => 502,
    };
    TransportError::Status {
        status,
        error_type: kind.map(str::to_owned),
        message: error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the stream reported an error")
            .to_owned(),
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
