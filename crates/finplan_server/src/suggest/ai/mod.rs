//! AI-written review notes: a tool-using conversation with a model on
//! OpenRouter (its Anthropic-compatible Messages endpoint, through
//! `openrouter-rs`) over one plan and one run.
//!
//! [`generate`] sends the static instructions ([`prompt`]) and the rendered
//! plan and run ([`ReviewContext`]), then serves the model its tools until it
//! ends its turn or a cap is hit. The shared tools (previews, validation,
//! preflight, run inspection and the calculators) live in [`tools`] as a
//! registry the loop serves whole; the loop itself owns `submit_suggestion`:
//!
//! - `preview_changes` simulates changes against the run's own draws through
//!   [`ToolHost::preview`] — the caller's preview core, so this module never
//!   touches the database or the engine;
//! - the rest of [`tools::Registry`], whose outputs are kept by call id so a
//!   note can cite them as [`Evidence::Computed`];
//! - `submit_suggestion` validates a note (its changes through
//!   [`ToolHost::resolve`], its evidence against the run, its text, and
//!   that it neither repeats an existing note nor quotes changes it never
//!   previewed) and either keeps it as an [`AiDraft`] or tells the model what
//!   to fix.
//!
//! Model output is untrusted: nothing it sends is kept without passing those
//! checks, and every cap (turns, notes, previews, tokens) is enforced here
//! rather than asked of the model. The API key only ever travels in a header;
//! error text is scrubbed of it before it leaves this module.
//!
//! Requests ask OpenRouter to route only to providers that honour every
//! parameter sent (tools above all) and that do not collect prompt data: the
//! context is a user's financial plan.

pub mod chat;
mod config;
mod context;
pub mod draft;
mod prompt;
pub mod tools;
mod transport;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use openrouter_rs::api::chat::CacheControl;
use openrouter_rs::api::messages::{
    AnthropicContentPart, AnthropicMessage, AnthropicMessageContent, AnthropicOutputConfig,
    AnthropicOutputEffort, AnthropicRole, AnthropicSystemPrompt, AnthropicSystemTextBlock,
    AnthropicThinking, AnthropicTool,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use config::{
    AiConfig, DEFAULT_APP_TITLE, DEFAULT_BASE_URL, DEFAULT_MODEL, DraftConfig, DraftLimits,
    PlanChatConfig, ThinkingMode,
};
pub use context::{NoteOutline, ReviewContext, render_path};
pub use transport::{
    BoxFuture, ModelInfo, ModelPrice, OpenRouterSettings, OpenRouterTransport, Reply, Request,
    Transport, TransportError,
};

use crate::api::suggestion_paths::{self, PathShape, StepShape};
use crate::observability::{AiCostSource, AiMotive, AiRetryReason, AiTool, AiToolOutcome};
use crate::suggest::rules::{Evidence, Kind, Section};
use crate::suggest::{Change, ChangeProblem, DiffLine};
use tools::{MAX_CHANGES, Registry, ToolEnv, batch_key};

/// What a review needs from the rest of the server: [`ToolHost`], the plan
/// and simulations the shared tools work on.
pub use tools::ToolHost;

/// Where a pass reports what it spends and does, as it happens. The server's
/// implementation feeds its metrics; every method defaults to nothing.
pub trait Observer: Send + Sync {
    /// One answered model request.
    fn turn(&self, _turn: &TurnReport) {}
    /// One tool call served.
    fn tool(&self, _tool: AiTool, _outcome: AiToolOutcome, _seconds: f64) {}
    /// A model request about to be sent again.
    fn retry(&self, _reason: AiRetryReason) {}
    /// A submission the checks accepted or sent back.
    fn submission(&self, _accepted: bool) {}
    /// The motive a parsed submission gave, and whether it was accepted.
    fn motive(&self, _motive: AiMotive, _accepted: bool) {}
    /// A model request about to go out; [`Observer::turn`] reports its answer.
    fn thinking(&self) {}
    /// A tool call about to be served, by the name the model called.
    fn tool_started(&self, _name: &str) {}
    /// That call served; `failed` when the model is handed an error.
    fn tool_finished(&self, _name: &str, _failed: bool) {}
    /// What the model said alongside its tool calls — narration on the way,
    /// not the answer, which is the reply that ends the turn.
    fn narration(&self, _text: &str) {}
}

/// An observer that records nothing.
pub struct NoObserver;
impl Observer for NoObserver {}

/// What one answered model request took and spent.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnReport {
    pub turn: u32,
    /// Latency of the request that was answered (not earlier failed tries).
    pub seconds: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    /// Dollars, and whether OpenRouter reported them or they were estimated
    /// from the model's prices; `None` when neither was available.
    pub cost: Option<(f64, AiCostSource)>,
}

/// The model's own guess at an unpreviewed note's effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    #[serde(default)]
    pub success_rate: Option<f64>,
    #[serde(default)]
    pub funding_success_rate: Option<f64>,
}

/// A note the model submitted and the checks accepted.
#[derive(Debug, Clone, Serialize)]
pub struct AiDraft {
    pub kind: Kind,
    pub section: Section,
    pub title: String,
    /// One sentence under the title; the reasoning sits behind a disclosure.
    pub summary: String,
    pub reasoning: String,
    pub evidence: Vec<Evidence>,
    /// The courses of action; empty on a read note.
    pub paths: Vec<AiPath>,
    /// The board note this one rewrites in place, keeping its id and thread;
    /// `None` for a new note.
    pub replaces: Option<i64>,
}

impl AiDraft {
    /// Every change of every step of every path.
    pub fn all_changes(&self) -> Vec<Change> {
        self.paths.iter().flat_map(AiPath::changes).collect()
    }
}

/// One accepted path.
#[derive(Debug, Clone, Serialize)]
pub struct AiPath {
    pub key: String,
    pub label: String,
    pub reasoning: Option<String>,
    pub recommended: bool,
    pub steps: Vec<AiStep>,
    /// Only on paths that were never previewed.
    pub estimate: Option<Estimate>,
    /// The model previewed exactly these steps before submitting.
    pub previewed: bool,
    /// That preview's result (the preview endpoint's `Preview` JSON), so
    /// the caller can store it as the path's check without simulating again.
    pub preview: Option<Value>,
}

impl AiPath {
    /// Every change of every step, in order: the path as one batch.
    pub fn changes(&self) -> Vec<Change> {
        self.steps
            .iter()
            .flat_map(|s| s.changes.iter().cloned())
            .collect()
    }
}

/// One accepted step, with the diff the server rendered for it.
#[derive(Debug, Clone, Serialize)]
pub struct AiStep {
    pub key: String,
    pub title: String,
    pub reasoning: Option<String>,
    pub changes: Vec<Change>,
    pub diff: Vec<DiffLine>,
}

/// Why the conversation ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stop {
    /// The model ended its turn.
    Finished,
    /// `max_turns` requests were spent.
    TurnLimit,
    /// `max_suggestions` notes were accepted.
    SuggestionLimit,
    /// A reply hit `max_tokens`; anything it was about to call is dropped.
    MaxTokens,
    /// The model declined.
    Refused { category: Option<String> },
    /// A later request failed after notes were already accepted; they are kept.
    Interrupted { error: String },
    /// A reply the loop does not know how to continue from.
    Unexpected { stop_reason: String },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    /// Model requests made (retries not counted).
    pub turns: u32,
    pub previews: u32,
    /// Goal seeks run (each is also charged to `previews`).
    pub goal_seeks: u32,
    /// Submissions the checks sent back.
    pub rejected: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    /// Dollars spent: `cost_reported_usd + cost_estimated_usd`.
    pub cost_usd: f64,
    /// The part OpenRouter reported on its replies.
    pub cost_reported_usd: f64,
    /// The part estimated from the model's listed prices.
    pub cost_estimated_usd: f64,
    /// Answered requests with neither a reported cost nor a price to estimate from.
    pub unpriced_turns: u32,
}

impl Usage {
    /// Where the pass's cost figure came from, for logs.
    pub fn cost_source(&self) -> &'static str {
        match (
            self.cost_reported_usd > 0.0,
            self.cost_estimated_usd > 0.0 || self.unpriced_turns > 0,
        ) {
            (true, false) => "reported",
            (false, true) if self.unpriced_turns == 0 => "estimated",
            (true, true) => "mixed",
            _ if self.unpriced_turns > 0 => "unknown",
            _ => "none",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AiOutcome {
    pub drafts: Vec<AiDraft>,
    pub stop: Stop,
    pub usage: Usage,
    /// The model that answered last, as the provider named it.
    pub model: Option<String>,
    /// The model's closing line, if it wrote one.
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    /// The API could not be reached or refused the request, after retries.
    Api(String),
    /// The API answered with something the loop cannot read.
    Malformed(String),
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiError::Api(message) => write!(f, "review AI request failed: {message}"),
            AiError::Malformed(message) => write!(f, "review AI reply unreadable: {message}"),
        }
    }
}

impl std::error::Error for AiError {}

/// Caps and request settings for one review.
#[derive(Debug, Clone)]
pub struct Settings {
    pub model: String,
    pub max_turns: u32,
    pub max_suggestions: usize,
    pub max_previews: u32,
    pub max_tokens: u32,
    /// When to send adaptive thinking and `output_config.effort`; `Auto` is
    /// settled per model by [`AiClient::thinks`].
    pub thinking: ThinkingMode,
    /// `output_config.effort` for Anthropic models, when `thinking` is on;
    /// other models get [`OTHER_EFFORT`] (see [`AiClient::effort`]).
    pub effort: &'static str,
    pub max_retries: u32,
    /// First retry delay; doubles after, then a random 50–100% of it is
    /// waited (OpenRouter's errors carry no `retry-after`).
    pub retry_base: Duration,
    /// Longest single wait between retries.
    pub retry_cap: Duration,
    /// What an optimization or risk note's paths must move.
    pub materiality: Materiality,
}

/// The smallest simulated effect worth a user's attention, for notes filed
/// as optimization or risk: any one of these clears it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Materiality {
    /// Success or funding success, in percentage points.
    pub rate_pts: f64,
    /// Real median final net worth, in percent.
    pub median_pct: f64,
    /// Real P10 final net worth, in percent.
    pub p10_pct: f64,
}

impl Default for Materiality {
    fn default() -> Self {
        Self {
            rate_pts: config::DEFAULT_MIN_RATE_PTS,
            median_pct: config::DEFAULT_MIN_MEDIAN_PCT,
            p10_pct: config::DEFAULT_MIN_P10_PCT,
        }
    }
}

/// Why the model thinks a note matters. Not stored: it steers the checks
/// (only risk and optimization must clear `Materiality`) and is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motive {
    /// The plan misstates how the model works or has an evident modeling error.
    Correctness,
    /// Something the person almost certainly has or will face is missing or unrealistic.
    Realism,
    /// Where and why the plan fails.
    Risk,
    /// A better outcome from the same facts.
    Optimization,
}

impl Motive {
    fn metric(self) -> AiMotive {
        match self {
            Motive::Correctness => AiMotive::Correctness,
            Motive::Realism => AiMotive::Realism,
            Motive::Risk => AiMotive::Risk,
            Motive::Optimization => AiMotive::Optimization,
        }
    }

    /// Whether the note's paths must prove a material effect: they change
    /// the plan to move the numbers, not to make it truer.
    fn needs_materiality(self) -> bool {
        matches!(self, Motive::Risk | Motive::Optimization)
    }
}

impl Settings {
    pub fn from_config(cfg: &AiConfig) -> Self {
        Self {
            model: cfg.model.trim().to_owned(),
            max_turns: cfg.max_turns,
            max_suggestions: cfg.max_suggestions,
            max_previews: cfg.max_previews,
            max_tokens: cfg.max_tokens,
            thinking: cfg.thinking,
            // Intelligence-sensitive work: the note quality is the product.
            effort: "high",
            max_retries: 3,
            retry_base: Duration::from_secs(2),
            retry_cap: Duration::from_secs(30),
            materiality: Materiality {
                rate_pts: cfg.min_rate_pts,
                median_pct: cfg.min_median_pct,
                p10_pct: cfg.min_p10_pct,
            },
        }
    }
}

pub struct AiClient {
    settings: Settings,
    transport: Arc<dyn Transport>,
    /// Kept only to scrub it out of error text.
    secret: Option<String>,
    /// Route only to zero-data-retention providers (a drafting client).
    zdr: bool,
    /// The shared tools this client's requests offer and its loop serves.
    registry: Registry,
    /// The model's listing, when last looked up. Shared with the clients
    /// derived from this one (chat, drafts), which ask the same model.
    listing: Listing,
}

type Listing = Arc<tokio::sync::Mutex<Option<(Instant, Option<ModelInfo>)>>>;

/// How long a listing lookup stands: prices and parameters change rarely,
/// and a failed lookup is not retried on every request.
const PRICE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const PRICE_RETRY: Duration = Duration::from_secs(10 * 60);

impl AiClient {
    /// `Ok(None)` when AI notes are off or have no key.
    pub fn from_config(cfg: &AiConfig) -> Result<Option<Self>, String> {
        let Some(key) = cfg.active_key() else {
            return Ok(None);
        };
        let transport = OpenRouterTransport::new(&OpenRouterSettings {
            base_url: &cfg.openrouter_base_url,
            api_key: key,
            app_title: cfg.openrouter_app_title.trim(),
            referer: cfg
                .openrouter_referer
                .as_deref()
                .map(str::trim)
                .filter(|r| !r.is_empty()),
            timeout: cfg.timeout(),
            idle_timeout: cfg.idle_timeout(),
        })?;
        Ok(Some(Self::new(
            Settings::from_config(cfg),
            Arc::new(transport),
            Some(key.to_owned()),
        )))
    }

    /// Any transport — a proxy, or a scripted double in tests.
    pub fn new(settings: Settings, transport: Arc<dyn Transport>, secret: Option<String>) -> Self {
        Self {
            settings,
            transport,
            secret,
            zdr: false,
            registry: Registry::all(),
            listing: Listing::default(),
        }
    }

    /// Serve only these shared tools (a loop with other needs picks a subset).
    pub fn with_registry(mut self, registry: Registry) -> Self {
        self.registry = registry;
        self
    }

    /// A client for the drafting agent: the same transport, key and model, with
    /// the draft budget (turns, previews, output tokens), the ZDR routing the
    /// draft settings ask for and the shared tools a draft is given.
    pub fn for_drafts(&self, cfg: &DraftConfig) -> Self {
        let mut settings = self.settings.clone();
        settings.max_turns = cfg.max_turns;
        settings.max_previews = cfg.max_previews;
        settings.max_tokens = cfg.max_tokens;
        Self {
            settings,
            transport: self.transport.clone(),
            secret: self.secret.clone(),
            zdr: cfg.require_zdr,
            registry: draft::shared_tools(),
            listing: self.listing.clone(),
        }
    }

    /// This client's requests may only be served by zero-data-retention
    /// providers. Drafting clients (documents in the context) turn it on; the
    /// review client does not, so review routing is unchanged.
    pub fn with_zero_data_retention(mut self, zdr: bool) -> Self {
        self.zdr = zdr;
        self
    }

    pub fn zero_data_retention(&self) -> bool {
        self.zdr
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    fn scrub(&self, text: &str) -> String {
        match self.secret.as_deref() {
            Some(secret) if !secret.is_empty() => text.replace(secret, "<redacted>"),
            _ => text.to_owned(),
        }
    }

    /// One request, retried on rate limits, overload, server and provider
    /// errors, and dropped connections. Returns the reply and the latency of
    /// the attempt that was answered.
    async fn create(
        &self,
        request: &Request,
        observer: &dyn Observer,
    ) -> Result<(Reply, f64), AiError> {
        let mut attempt = 0;
        loop {
            let started = Instant::now();
            let sent = self.transport.send(request).await;
            let seconds = started.elapsed().as_secs_f64();
            match sent {
                Ok(reply) => return Ok((reply, seconds)),
                Err(error) if error.retryable() && attempt < self.settings.max_retries => {
                    let delay = self.backoff(attempt);
                    let reason = retry_reason(&error);
                    observer.retry(reason);
                    tracing::warn!(
                        event = "review_ai.retry",
                        attempt = attempt + 1,
                        reason = reason.as_str(),
                        status = status_of(&error),
                        latency_ms = (seconds * 1000.0) as u64,
                        backoff_ms = delay.as_millis() as u64,
                        error = %self.scrub(&error.to_string()),
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
                Err(error) => {
                    tracing::warn!(
                        event = "review_ai.request_failed",
                        attempts = attempt + 1,
                        status = status_of(&error),
                        latency_ms = (seconds * 1000.0) as u64,
                        error = %self.scrub(&error.to_string()),
                    );
                    return Err(AiError::Api(self.scrub(&error.to_string())));
                }
            }
        }
    }

    /// The model's listing, looked up once and kept for [`PRICE_TTL`];
    /// `None` when it is not listed. A failed lookup is logged and retried
    /// after [`PRICE_RETRY`].
    async fn listing(&self) -> Option<ModelInfo> {
        let mut cached = self.listing.lock().await;
        if let Some((at, info)) = *cached {
            let ttl = if info.is_some() {
                PRICE_TTL
            } else {
                PRICE_RETRY
            };
            if at.elapsed() < ttl {
                return info;
            }
        }
        let info = match self.transport.model_info(&self.settings.model).await {
            Ok(info) => {
                if info.is_none() {
                    tracing::info!(event = "review_ai.model_unlisted", model = %self.settings.model);
                }
                info
            }
            Err(error) => {
                tracing::warn!(
                    event = "review_ai.model_lookup_failed",
                    model = %self.settings.model,
                    error = %self.scrub(&error.to_string()),
                );
                None
            }
        };
        *cached = Some((Instant::now(), info));
        info
    }

    /// The model's listed prices; `None` when they are unknown.
    async fn price(&self) -> Option<ModelPrice> {
        self.listing().await.and_then(|info| info.price)
    }

    /// Whether this client's requests carry thinking and effort. `Auto`
    /// asks the listing, so only a model that is `Auto` pays for a lookup.
    pub async fn thinks(&self) -> bool {
        let mode = self.settings.thinking;
        let reasons = match mode {
            ThinkingMode::Auto => self.listing().await.map(|info| info.reasons),
            ThinkingMode::On | ThinkingMode::Off => None,
        };
        mode.applies_to(&self.settings.model, reasons)
    }

    /// Exponential, capped, with jitter so concurrent reviews that hit the
    /// same rate limit do not retry in lockstep.
    fn backoff(&self, attempt: u32) -> Duration {
        let ceiling =
            (self.settings.retry_base * 2u32.saturating_pow(attempt)).min(self.settings.retry_cap);
        ceiling.mul_f64(rand::rng().random_range(0.5..=1.0))
    }

    async fn request(&self, messages: &[AnthropicMessage]) -> Result<Request, AiError> {
        let mut reference = AnthropicSystemTextBlock::text(prompt::reference());
        // The static instructions stay cached whatever happens after them.
        reference.cache_control = Some(CacheControl::ephemeral());
        self.request_with(
            vec![
                AnthropicSystemTextBlock::text(prompt::SYSTEM_PROMPT),
                reference,
            ],
            tools(&self.registry),
            messages,
        )
        .await
    }

    /// One request with its own instructions and tools: the review's, or the
    /// drafting agent's.
    async fn request_with(
        &self,
        system: Vec<AnthropicSystemTextBlock>,
        tools: Vec<AnthropicTool>,
        messages: &[AnthropicMessage],
    ) -> Result<Request, AiError> {
        let settings = &self.settings;
        let provider = transport::provider_preferences(self.zdr);
        // First, so it sits inside every request's cached prefix.
        let mut system = system;
        system.insert(0, AnthropicSystemTextBlock::text(prompt::WRITING_STYLE));

        let mut builder = Request::builder();
        builder
            .model(settings.model.as_str())
            .max_tokens(settings.max_tokens)
            .messages(with_breakpoint(messages))
            .system(AnthropicSystemPrompt::Blocks(system))
            .tools(tools)
            .provider(provider);
        if self.thinks().await {
            builder
                .thinking(AnthropicThinking::adaptive())
                .output_config(AnthropicOutputConfig::with_effort(effort(self.effort())));
        }
        builder
            .build()
            .map_err(|e| AiError::Api(format!("request could not be built: {e}")))
    }
}

/// The tool definitions, typed from [`prompt::tools`].
fn tools(registry: &Registry) -> Vec<AnthropicTool> {
    typed_tools(&prompt::tools(registry))
}

/// Tool definitions as JSON (`name`, `description`, `input_schema`), typed.
fn typed_tools(definitions: &Value) -> Vec<AnthropicTool> {
    definitions
        .as_array()
        .into_iter()
        .flatten()
        .map(|tool| {
            AnthropicTool::custom(
                tool["name"].as_str().unwrap_or_default(),
                tool["description"].as_str().unwrap_or_default(),
                tool["input_schema"].clone(),
            )
        })
        .collect()
}

fn retry_reason(error: &TransportError) -> AiRetryReason {
    match error {
        TransportError::Status { status: 429, .. } => AiRetryReason::RateLimit,
        TransportError::Status {
            status: 408 | 504, ..
        } => AiRetryReason::Timeout,
        TransportError::Status {
            error_type: Some(kind),
            ..
        } if kind.starts_with("provider") => AiRetryReason::Provider,
        TransportError::Status { .. } => AiRetryReason::Server,
        TransportError::Network(message) if message.contains("timed out") => AiRetryReason::Timeout,
        _ => AiRetryReason::Network,
    }
}

/// The HTTP status of a failed request, 0 when there was no reply.
fn status_of(error: &TransportError) -> u16 {
    match error {
        TransportError::Status { status, .. } => *status,
        _ => 0,
    }
}

/// A stop reason as logged: the ones the loop knows, else `other`, so a
/// provider cannot put arbitrary text in a log field.
fn stop_label(stop_reason: &str) -> &'static str {
    match stop_reason {
        "end_turn" => "end_turn",
        "stop_sequence" => "stop_sequence",
        "tool_use" => "tool_use",
        "max_tokens" => "max_tokens",
        "refusal" => "refusal",
        "pause_turn" => "pause_turn",
        "" => "none",
        _ => "other",
    }
}

/// Effort for a reasoning model that is not Claude. OpenRouter turns an
/// effort into a share of `max_tokens` for reasoning on such models (high is
/// about 80%, medium about 50%), and their reasoning runs long: at high, GLM
/// spent a whole request's budget thinking and never reached a tool call.
/// Claude takes the effort natively and paces its thinking across turns.
const OTHER_EFFORT: &str = "medium";

impl AiClient {
    /// The effort this client's requests carry when they think.
    fn effort(&self) -> &'static str {
        if self.settings.model.trim().starts_with("anthropic/") {
            self.settings.effort
        } else {
            OTHER_EFFORT
        }
    }
}

fn effort(level: &str) -> AnthropicOutputEffort {
    serde_json::from_value(Value::String(level.to_owned())).unwrap_or(AnthropicOutputEffort::High)
}

/// The conversation with a cache breakpoint on its newest block, so each
/// turn reads everything before it from the cache. Only the copy sent
/// carries it: the stored history stays unmarked, which keeps one
/// breakpoint per request here plus the system prompt's.
fn with_breakpoint(messages: &[AnthropicMessage]) -> Vec<AnthropicMessage> {
    let mut messages = messages.to_vec();
    if let Some(AnthropicMessageContent::Parts(parts)) =
        messages.last_mut().map(|message| &mut message.content)
        && let Some(
            AnthropicContentPart::Text { cache_control, .. }
            | AnthropicContentPart::ToolResult { cache_control, .. },
        ) = parts.last_mut()
    {
        *cache_control = Some(CacheControl::ephemeral());
    }
    messages
}

/// What a submission looks like on the wire.
#[derive(Debug, Deserialize)]
struct Submission {
    kind: Kind,
    section: Section,
    /// Required; optional here so a missing one is a named problem, not a
    /// parse error.
    #[serde(default)]
    motive: Option<Motive>,
    title: String,
    /// Required; optional here so a missing one is a named problem.
    #[serde(default)]
    summary: Option<String>,
    reasoning: String,
    /// Why a check note offers no path.
    #[serde(default)]
    no_change_reason: Option<String>,
    #[serde(default)]
    evidence: Vec<Evidence>,
    #[serde(default)]
    paths: Vec<SubmittedPath>,
    /// A board note to rewrite in place rather than add beside.
    #[serde(default)]
    replaces: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SubmittedPath {
    key: String,
    label: String,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    recommended: bool,
    #[serde(default)]
    estimate: Option<Estimate>,
    #[serde(default)]
    steps: Vec<SubmittedStep>,
}

#[derive(Debug, Deserialize)]
struct SubmittedStep {
    key: String,
    title: String,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    changes: Vec<Change>,
}

impl SubmittedPath {
    fn batches(&self) -> Vec<Vec<Change>> {
        self.steps.iter().map(|s| s.changes.clone()).collect()
    }
}

const MAX_TITLE: usize = 120;
const MAX_REASONING: usize = 1_200;
/// The lead sentence under a note's title.
pub(crate) const MAX_SUMMARY: usize = 200;

/// Why a note's summary is refused, if it is: it is one line of 1 to
/// `MAX_SUMMARY` characters.
pub(crate) fn summary_problem(summary: &str) -> Option<String> {
    (summary.is_empty() || summary.chars().count() > MAX_SUMMARY || summary.contains('\n'))
        .then(|| format!("summary must be one sentence on one line, 1 to {MAX_SUMMARY} characters"))
}
const MAX_EVIDENCE: usize = 10;
const MAX_STAT_NAME: usize = 80;
const MAX_NO_CHANGE_REASON: usize = 200;
const MAX_EXCERPT: usize = 400;
/// `funding_diagnostics` fields a `diagnostic` evidence entry may name.
const DIAGNOSTIC_FIELDS: &[&str] = &[
    "iterations",
    "failed",
    "cash_shortfall",
    "event_failure",
    "iteration_limit",
    "failed_solvent",
    "first_shortfall_years",
    "median_first_shortfall_year",
    "shortfall_accounts",
    "event_failures",
    "liquid_depleted_years",
    "median_max_deficit",
    "median_shortfall_years",
];

/// What one tool call came to, for its log line and metrics. Never carries
/// model text or plan content: kinds and counts only.
struct ToolReport {
    tool: AiTool,
    outcome: AiToolOutcome,
    /// preview: whether the edit shares the base run's draws.
    paired: Option<bool>,
    /// Problems found with the call's changes or submission.
    problems: usize,
    /// submit: the kinds of problem that sent it back.
    problem_kinds: Vec<&'static str>,
    /// submit: the motive the note gave, once it parsed.
    motive: Option<AiMotive>,
}

impl ToolReport {
    fn new(tool: AiTool, outcome: AiToolOutcome) -> Self {
        Self {
            tool,
            outcome,
            paired: None,
            problems: 0,
            problem_kinds: Vec::new(),
            motive: None,
        }
    }
}

/// A reason a submission was sent back: a fixed kind for logs, and the
/// message the model reads.
type Problem = (&'static str, Value);

fn change_problem_kind(problem: &ChangeProblem) -> &'static str {
    match problem {
        ChangeProblem::Stale { .. } => "change_stale",
        ChangeProblem::BadPath { .. } => "change_bad_path",
        ChangeProblem::InvalidBody { .. } => "change_invalid_body",
        ChangeProblem::UnknownTarget { .. } => "change_unknown_target",
        ChangeProblem::UnsupportedOp { .. } => "change_unsupported_op",
        ChangeProblem::DuplicateKey { .. } => "change_duplicate_key",
        ChangeProblem::UnknownReference { .. } => "change_unknown_reference",
        ChangeProblem::WrongReferenceKind { .. } => "change_wrong_reference_kind",
        ChangeProblem::ReferenceCycle { .. } => "change_reference_cycle",
    }
}

/// The output of a tool call that succeeded, kept for the session so a note
/// can cite it as `Evidence::Computed`.
#[derive(Debug, Clone)]
struct Computed {
    tool: String,
    /// The tool result the model was shown; kept for loops that check a
    /// note's figures against what its tools returned.
    #[allow(dead_code)]
    output: String,
}

/// One review's state across turns.
struct Session<'a> {
    client: &'a AiClient,
    context: &'a ReviewContext,
    tools: &'a dyn ToolHost,
    observer: &'a dyn Observer,
    drafts: Vec<AiDraft>,
    usage: Usage,
    /// Serialized change batches whose preview ran without problems, and
    /// what the preview returned.
    previewed: HashMap<String, Value>,
    /// Successful shared-tool calls by `tool_use` id.
    computed: HashMap<String, Computed>,
}

impl Session<'_> {
    /// Serve one tool call, then log and count it.
    async fn run_tool(&mut self, call_id: &str, name: &str, input: &Value) -> (String, bool) {
        self.observer.tool_started(name);
        let started = Instant::now();
        let (output, is_error, report) = if name == prompt::SUBMIT_TOOL {
            self.submit(input)
        } else {
            self.serve(call_id, name, input).await
        };
        let seconds = started.elapsed().as_secs_f64();
        self.observer.tool_finished(name, is_error);
        self.observer.tool(report.tool, report.outcome, seconds);
        if report.tool == AiTool::Submit {
            let accepted = report.outcome == AiToolOutcome::Accepted;
            self.observer.submission(accepted);
            if let Some(motive) = report.motive {
                self.observer.motive(motive, accepted);
            }
        }
        tracing::info!(
            event = "review_ai.tool",
            tool = report.tool.as_str(),
            outcome = report.outcome.as_str(),
            duration_ms = (seconds * 1000.0) as u64,
            paired = report.paired,
            problems = report.problems,
            problem_kinds = %report.problem_kinds.join(","),
            motive = report.motive.map_or("", AiMotive::as_str),
            previews = self.usage.previews,
            notes = self.drafts.len(),
        );
        (output, is_error)
    }

    /// A call to one of the registry's tools.
    async fn serve(
        &mut self,
        call_id: &str,
        name: &str,
        input: &Value,
    ) -> (String, bool, ToolReport) {
        let env = ToolEnv {
            host: self.tools,
            previews_left: self
                .client
                .settings
                .max_previews
                .saturating_sub(self.usage.previews),
            goal_seeks_left: tools::goal_seek::MAX_GOAL_SEEKS.saturating_sub(self.usage.goal_seeks),
            failure_profile: self.context.failure_profile.as_ref(),
        };
        let Some((tool, out)) = self.client.registry.dispatch(name, input, &env).await else {
            return (
                format!("unknown tool `{name}`"),
                true,
                ToolReport::new(AiTool::Unknown, AiToolOutcome::Invalid),
            );
        };
        self.usage.previews += out.previews_spent;
        self.usage.goal_seeks += out.goal_seeks_spent;
        self.previewed.extend(out.previewed);
        let text = if out.is_error {
            // Not the model's to read: a key must never reach it.
            self.client.scrub(&out.text)
        } else {
            self.computed.insert(
                call_id.to_owned(),
                Computed {
                    tool: name.to_owned(),
                    output: out.text.clone(),
                },
            );
            out.text
        };
        let mut report = ToolReport::new(tool, out.outcome);
        report.paired = out.paired;
        report.problems = out.problems;
        (text, out.is_error, report)
    }

    fn submit(&mut self, input: &Value) -> (String, bool, ToolReport) {
        let settings = &self.client.settings;
        if self.drafts.len() >= settings.max_suggestions {
            self.usage.rejected += 1;
            return (
                format!(
                    "This review already has {} notes, the most allowed. Stop here.",
                    settings.max_suggestions
                ),
                true,
                ToolReport::new(AiTool::Submit, AiToolOutcome::BudgetExhausted),
            );
        }
        let submission = match serde_json::from_value::<Submission>(input.clone()) {
            Ok(s) => s,
            Err(e) => {
                self.usage.rejected += 1;
                let mut report = ToolReport::new(AiTool::Submit, AiToolOutcome::Invalid);
                report.problems = 1;
                report.problem_kinds = vec!["parse"];
                return (
                    format!("rejected: the note does not parse: {e}"),
                    true,
                    report,
                );
            }
        };
        let motive = Some(submission.motive.map_or(AiMotive::Missing, Motive::metric));
        match self.check(submission) {
            Ok(draft) => {
                let diffs: Vec<Value> = draft
                    .paths
                    .iter()
                    .map(|p| {
                        json!({
                            "path": p.key,
                            "steps": p.steps.iter().map(|s| json!({"step": s.key, "diff": s.diff})).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                let reply = json!({
                    "accepted": true,
                    "diffs": diffs,
                    "remaining": settings.max_suggestions - self.drafts.len() - 1,
                });
                self.drafts.push(draft);
                let mut report = ToolReport::new(AiTool::Submit, AiToolOutcome::Accepted);
                report.motive = motive;
                (reply.to_string(), false, report)
            }
            Err(problems) => {
                self.usage.rejected += 1;
                let mut report = ToolReport::new(AiTool::Submit, AiToolOutcome::Rejected);
                report.motive = motive;
                report.problems = problems.len();
                for (kind, _) in &problems {
                    if !report.problem_kinds.contains(kind) {
                        report.problem_kinds.push(kind);
                    }
                }
                let messages: Vec<Value> = problems.into_iter().map(|(_, m)| m).collect();
                (
                    json!({"accepted": false, "problems": messages}).to_string(),
                    true,
                    report,
                )
            }
        }
    }

    /// Every check a note must pass, collected rather than stopping at the
    /// first, so the model can fix them in one go.
    fn check(&self, s: Submission) -> Result<AiDraft, Vec<Problem>> {
        let mut messages: Vec<(&'static str, String)> = Vec::new();
        let mut problem = |kind: &'static str, text: String| messages.push((kind, text));

        let title = s.title.trim().to_owned();
        let reasoning = s.reasoning.trim().to_owned();
        let summary = s.summary.as_deref().unwrap_or("").trim().to_owned();
        if title.is_empty() || title.chars().count() > MAX_TITLE || title.contains('\n') {
            problem(
                "title",
                format!("title must be one line of 1 to {MAX_TITLE} characters"),
            );
        }
        if let Some(text) = summary_problem(&summary) {
            problem("summary", text);
        }
        if reasoning.is_empty() || reasoning.chars().count() > MAX_REASONING {
            problem(
                "reasoning",
                format!("reasoning must be 1 to {MAX_REASONING} characters"),
            );
        }
        if s.kind == Kind::Add {
            problem(
                "kind",
                "add notes are for drafting a new plan; a review writes fix, check, stress or read notes".into(),
            );
        }
        let motive = s.motive;
        if motive.is_none() {
            problem(
                "motive",
                "motive is required: correctness, realism, risk or optimization".into(),
            );
        }
        let no_change_reason = s
            .no_change_reason
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_owned);
        if let Some(reason) = &no_change_reason
            && (reason.chars().count() > MAX_NO_CHANGE_REASON || reason.contains('\n'))
        {
            problem(
                "no_change_reason",
                format!(
                    "no_change_reason must be one line of at most {MAX_NO_CHANGE_REASON} characters"
                ),
            );
        }
        if s.kind == Kind::Check && s.paths.is_empty() && no_change_reason.is_none() {
            problem(
                "no_change",
                "a check note needs at least one path; if the issue truly cannot be expressed as a plan change, say why in no_change_reason. Missing income or spending can: add it with a new_event".into(),
            );
        }
        if s.evidence.len() > MAX_EVIDENCE {
            problem(
                "evidence_count",
                format!("at most {MAX_EVIDENCE} evidence entries"),
            );
        }
        for (i, e) in s.evidence.iter().enumerate() {
            if let Err(why) = self.check_evidence(e) {
                problem("evidence", format!("evidence[{i}]: {why}"));
            }
        }
        let shapes: Vec<PathShape<'_>> = s
            .paths
            .iter()
            .map(|p| PathShape {
                key: &p.key,
                label: &p.label,
                reasoning: p.reasoning.as_deref(),
                recommended: p.recommended,
                steps: p
                    .steps
                    .iter()
                    .map(|st| StepShape {
                        key: &st.key,
                        title: &st.title,
                        reasoning: st.reasoning.as_deref(),
                        changes: &st.changes,
                    })
                    .collect(),
            })
            .collect();
        for shape in suggestion_paths::shape_problems(s.kind, &shapes, MAX_CHANGES) {
            problem("shape", shape);
        }
        for path in &s.paths {
            if let Some(estimate) = &path.estimate {
                for (name, value) in [
                    ("success_rate", estimate.success_rate),
                    ("funding_success_rate", estimate.funding_success_rate),
                ] {
                    if value.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
                        problem(
                            "estimate",
                            format!(
                                "path \"{}\": estimate.{name} must be a fraction between 0 and 1",
                                path.key
                            ),
                        );
                    }
                }
            }
        }

        let replaces = s.replaces;
        if let Some(id) = replaces {
            if !self.context.editable.contains(&id) {
                problem(
                    "replaces",
                    format!(
                        "note #{id} cannot be rewritten: only an open note on this board with \
                         no step applied can be; submit a new note instead"
                    ),
                );
            } else if self.drafts.iter().any(|d| d.replaces == Some(id)) {
                problem(
                    "replaces",
                    format!("note #{id} was already rewritten this turn"),
                );
            }
        }

        let normalized = context::normalize(&title);
        let all: Vec<Change> = s.paths.iter().flat_map(|p| p.batches()).flatten().collect();
        let edits = context::edits(&all);
        let accepted: Vec<context::Existing> = self
            .drafts
            .iter()
            .map(|d| context::Existing::new(d.kind, &d.title, &d.all_changes()))
            .collect();
        let twin = self
            .context
            .existing
            .iter()
            .chain(&accepted)
            // The note being rewritten is not its own duplicate.
            .filter(|n| replaces.is_none() || n.id != replaces)
            .find(|n| {
                n.title == normalized || (!edits.is_empty() && n.edits == edits && n.kind == s.kind)
            });
        if let Some(twin) = twin {
            // Say which test matched, so a note refused by mistake can be told
            // apart from a real repeat.
            let why = if twin.title == normalized {
                "has the same title as".to_owned()
            } else {
                format!(
                    "makes the same changes ({}) as",
                    edits.iter().cloned().collect::<Vec<_>>().join(", ")
                )
            };
            // A twin the model may rewrite is the likeliest intent: say how.
            let fix = match twin.id.filter(|id| self.context.editable.contains(id)) {
                Some(id) => format!(
                    "to change that note, submit this one with \"replaces\": {id}; otherwise drop it or make a different point"
                ),
                None => "drop it or make a different point".to_owned(),
            };
            problem(
                "duplicate",
                format!(
                    "repeats an existing note: it {why} \"{}\"; {fix}",
                    twin.title
                ),
            );
        }

        // Every path previewed exactly as submitted, while previews are left;
        // then resolved step by step for its diffs.
        let budget_left = self.usage.previews < self.client.settings.max_previews;
        // One entry per failing path: its first problem's kind, and every
        // problem of the step that failed.
        let mut change_problems: Vec<Problem> = Vec::new();
        let mut paths = Vec::with_capacity(s.paths.len());
        for path in s.paths {
            let batches = path.batches();
            let preview = self.previewed.get(&batch_key(&batches)).cloned();
            let previewed = preview.is_some();
            if !previewed && budget_left {
                problem(
                    "not_previewed",
                    format!(
                        "path \"{}\": preview exactly these steps with preview_changes before submitting",
                        path.key
                    ),
                );
            }
            let diffs = match self.tools.resolve_steps(&batches) {
                Ok(diffs) => diffs,
                Err((step, found)) => {
                    let step_key = path.steps.get(step).map_or("", |s| s.key.as_str());
                    let kind = found.first().map_or("change", change_problem_kind);
                    change_problems.push((
                        kind,
                        json!({
                            "path": path.key,
                            "step": step_key,
                            "problems": found,
                        }),
                    ));
                    continue;
                }
            };
            for (step, diff) in path.steps.iter().zip(&diffs) {
                if diff.is_empty() {
                    problem(
                        "no_effect",
                        format!(
                            "path \"{}\", step \"{}\": the changes leave the plan as it is",
                            path.key, step.key
                        ),
                    );
                }
            }
            if let Some(motive) = motive
                && motive.needs_materiality()
                && s.kind != Kind::Stress
            {
                let what = match motive {
                    Motive::Optimization => "an optimization",
                    _ => "a risk",
                };
                match &preview {
                    Some(preview) => {
                        let deltas = Deltas::of(preview);
                        let floor = &self.client.settings.materiality;
                        if !deltas.clears(floor) {
                            problem(
                                "immaterial",
                                format!(
                                    "path \"{}\": {} — too small for {what} note, which needs {}. Drop the path, or the note; keep it only if it corrects how the plan models the person's reality (motive correctness or realism)",
                                    path.key,
                                    deltas.describe(),
                                    floor.describe(),
                                ),
                            );
                        }
                    }
                    // With previews left, `not_previewed` already asks for one.
                    None if !budget_left => problem(
                        "immaterial",
                        format!(
                            "path \"{}\": {what} note must show a simulated effect, and the preview budget is used up; drop it",
                            path.key
                        ),
                    ),
                    None => {}
                }
            }
            paths.push(AiPath {
                key: path.key,
                label: path.label.trim().to_owned(),
                reasoning: path
                    .reasoning
                    .map(|r| r.trim().to_owned())
                    .filter(|r| !r.is_empty()),
                recommended: path.recommended,
                // An estimate only means something where nothing was simulated.
                estimate: if previewed { None } else { path.estimate },
                previewed,
                preview,
                steps: path
                    .steps
                    .into_iter()
                    .zip(diffs)
                    .map(|(step, diff)| AiStep {
                        key: step.key,
                        title: step.title.trim().to_owned(),
                        reasoning: step
                            .reasoning
                            .map(|r| r.trim().to_owned())
                            .filter(|r| !r.is_empty()),
                        changes: step.changes,
                        diff,
                    })
                    .collect(),
            });
        }

        if !messages.is_empty() || !change_problems.is_empty() {
            return Err(messages
                .into_iter()
                .map(|(kind, text)| (kind, Value::String(text)))
                .chain(change_problems)
                .collect());
        }
        let reasoning = match no_change_reason {
            Some(reason) if s.kind == Kind::Check && paths.is_empty() => {
                let reason = reason.trim_end_matches('.');
                format!("{reasoning} {reason}.")
            }
            _ => reasoning,
        };
        Ok(AiDraft {
            kind: s.kind,
            section: s.section,
            title,
            summary,
            reasoning,
            evidence: s.evidence,
            paths,
            replaces,
        })
    }

    fn check_evidence(&self, e: &Evidence) -> Result<(), String> {
        let account = |id: i64| {
            self.context
                .accounts
                .contains(&id)
                .then_some(())
                .ok_or(format!("no account #{id} in the plan"))
        };
        match e {
            Evidence::Ledger {
                year,
                event_id,
                account_id,
            } => {
                if let Some((first, last)) = self.context.years
                    && !(first..=last).contains(year)
                {
                    return Err(format!("year {year} is outside the run ({first}-{last})"));
                }
                if let Some(id) = event_id
                    && !self.context.events.contains(id)
                {
                    return Err(format!("no event #{id} in the plan"));
                }
                if let Some(id) = account_id {
                    account(*id)?;
                }
                Ok(())
            }
            Evidence::AccountSeries {
                account_id,
                date,
                value,
            } => {
                account(*account_id)?;
                if !self.context.dates.is_empty() && !self.context.dates.contains(date) {
                    return Err(format!("{date} is not a point on the shown path"));
                }
                finite(*value)
            }
            Evidence::Stat { name, value } => {
                if name.trim().is_empty() || name.chars().count() > MAX_STAT_NAME {
                    return Err(format!("stat name must be 1 to {MAX_STAT_NAME} characters"));
                }
                finite(*value)
            }
            Evidence::Diagnostic { field, value } => {
                if !DIAGNOSTIC_FIELDS.contains(&field.as_str()) {
                    return Err(format!("`{field}` is not a funding_diagnostics field"));
                }
                finite(*value)
            }
            Evidence::Document { .. }
            | Evidence::Answer { .. }
            | Evidence::Description { .. }
            | Evidence::Computed { .. } => check_source_evidence(self.context, &self.computed, e),
        }
    }
}

/// The checks for evidence that names a source outside the run: an uploaded
/// document, an answer, the user's description or a tool call of the session.
/// Shared by the review loop and the drafting loop.
fn check_source_evidence(
    context: &ReviewContext,
    computed: &HashMap<String, Computed>,
    e: &Evidence,
) -> Result<(), String> {
    match e {
        Evidence::Document {
            document_id,
            page,
            excerpt,
        } => {
            check_excerpt(excerpt)?;
            let pages = context
                .documents
                .get(document_id)
                .ok_or_else(|| format!("no document #{document_id} in this session"))?;
            let index = usize::try_from(*page)
                .ok()
                .filter(|p| (1..=pages.len().max(1)).contains(p))
                .ok_or_else(|| {
                    format!(
                        "document #{document_id} has {} page(s); page {page} is outside it",
                        pages.len().max(1)
                    )
                })?;
            // A document with no text layer (a scan) has nothing to check against.
            match pages.get(index - 1) {
                Some(text) if !text.trim().is_empty() => {
                    if squash(text).contains(&squash(excerpt)) {
                        Ok(())
                    } else {
                        Err(format!(
                            "the excerpt does not appear on page {page} of document #{document_id}; quote the text exactly"
                        ))
                    }
                }
                _ => Ok(()),
            }
        }
        Evidence::Answer { question_key } => context
            .answers
            .contains(question_key)
            .then_some(())
            .ok_or_else(|| format!("the user has not answered a question `{question_key}`")),
        Evidence::Description { excerpt } => {
            check_excerpt(excerpt)?;
            match &context.description {
                Some(text) if squash(text).contains(&squash(excerpt)) => Ok(()),
                Some(_) => Err(
                    "the excerpt does not appear in the user's description; quote it exactly"
                        .into(),
                ),
                None => Err("there is no user description in this session".into()),
            }
        }
        Evidence::Computed { tool, call_id } => match computed.get(call_id) {
            Some(call) if call.tool == *tool => Ok(()),
            Some(call) => Err(format!(
                "call {call_id} was a call to {}, not {tool}",
                call.tool
            )),
            None => Err(format!(
                "no successful call {call_id} to {tool} in this session; cite the tool_use id of a call that returned a result"
            )),
        },
        _ => Ok(()),
    }
}

/// What a preview moved, from its `base` and `edited` statistics: rates in
/// percentage points, real final net worth in percent. `None` where the
/// preview does not carry the figure.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Deltas {
    success_pts: Option<f64>,
    funding_pts: Option<f64>,
    median_pct: Option<f64>,
    p10_pct: Option<f64>,
}

impl Deltas {
    fn of(preview: &Value) -> Self {
        let stat = |side: &str, pointer: &str| {
            preview
                .get(side)
                .and_then(|s| s.pointer(pointer))
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
        };
        let pts = |pointer: &str| Some((stat("edited", pointer)? - stat("base", pointer)?) * 100.0);
        let pct = |pointer: &str| {
            let (base, edited) = (stat("base", pointer)?, stat("edited", pointer)?);
            if base > 0.0 {
                Some((edited - base) / base * 100.0)
            } else if edited > 0.0 {
                // From nothing (or debt) to a positive balance: as material as it gets.
                Some(f64::INFINITY)
            } else {
                None
            }
        };
        Self {
            success_pts: pts("/success_rate"),
            funding_pts: pts("/funding_success_rate"),
            median_pct: pct("/real_final/p50"),
            p10_pct: pct("/real_final/p10"),
        }
    }

    /// Whether any one improvement reaches its floor.
    fn clears(&self, floor: &Materiality) -> bool {
        let at_least = |delta: Option<f64>, min: f64| delta.is_some_and(|d| d >= min);
        at_least(self.success_pts, floor.rate_pts)
            || at_least(self.funding_pts, floor.rate_pts)
            || at_least(self.median_pct, floor.median_pct)
            || at_least(self.p10_pct, floor.p10_pct)
    }

    /// "success +0.05 pts, funding +0.05 pts, real median +0.4%, real P10 +0.1%"
    fn describe(&self) -> String {
        let signed = |v: f64, digits: usize, unit: &str| {
            if v.is_infinite() {
                "from zero to positive".to_owned()
            } else {
                format!("{v:+.digits$}{unit}")
            }
        };
        let parts: Vec<String> = [
            ("success", self.success_pts.map(|v| signed(v, 2, " pts"))),
            ("funding", self.funding_pts.map(|v| signed(v, 2, " pts"))),
            ("real median", self.median_pct.map(|v| signed(v, 1, "%"))),
            ("real P10", self.p10_pct.map(|v| signed(v, 1, "%"))),
        ]
        .into_iter()
        .filter_map(|(name, v)| v.map(|v| format!("{name} {v}")))
        .collect();
        if parts.is_empty() {
            "the preview reported no comparable figures".to_owned()
        } else {
            format!("the preview moved {}", parts.join(", "))
        }
    }
}

impl Materiality {
    fn describe(&self) -> String {
        format!(
            "at least +{} pts on success or funding success, +{}% on the real median, or +{}% on the real P10",
            trim_float(self.rate_pts),
            trim_float(self.median_pct),
            trim_float(self.p10_pct)
        )
    }
}

/// 1.0 -> "1", 2.5 -> "2.5".
fn trim_float(v: f64) -> String {
    let text = format!("{v:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Text with runs of whitespace collapsed to one space, for comparing an
/// excerpt with where it came from.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn check_excerpt(excerpt: &str) -> Result<(), String> {
    let len = excerpt.trim().chars().count();
    if len == 0 || len > MAX_EXCERPT {
        return Err(format!("an excerpt is 1 to {MAX_EXCERPT} characters"));
    }
    Ok(())
}

fn finite(value: f64) -> Result<(), String> {
    value
        .is_finite()
        .then_some(())
        .ok_or_else(|| "value must be a finite number".into())
}

/// One reply's tokens, and its cost: OpenRouter's own figure when the reply
/// carries one (`usage.cost`), else an estimate from `price`.
fn turn_report(turn: u32, seconds: f64, reply: &Reply, price: Option<ModelPrice>) -> TurnReport {
    let u = reply.usage.as_ref();
    let input = u.and_then(|u| u.input_tokens).unwrap_or(0);
    let output = u.and_then(|u| u.output_tokens).unwrap_or(0);
    let cache_read = u.and_then(|u| u.cache_read_input_tokens).unwrap_or(0);
    let cache_write = u.and_then(|u| u.cache_creation_input_tokens).unwrap_or(0);
    let reported = u
        .and_then(|u| u.extra.get("cost"))
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite() && *c >= 0.0);
    let cost = match (reported, price) {
        (Some(cost), _) => Some((cost, AiCostSource::Reported)),
        (None, Some(price)) => Some((
            price.cost(input, output, cache_read, cache_write),
            AiCostSource::Estimated,
        )),
        (None, None) => None,
    };
    TurnReport {
        turn,
        seconds,
        input_tokens: input,
        output_tokens: output,
        cache_read_input_tokens: cache_read,
        cache_creation_input_tokens: cache_write,
        cost,
    }
}

fn add_usage(usage: &mut Usage, turn: &TurnReport) {
    usage.input_tokens += turn.input_tokens;
    usage.output_tokens += turn.output_tokens;
    usage.cache_creation_input_tokens += turn.cache_creation_input_tokens;
    usage.cache_read_input_tokens += turn.cache_read_input_tokens;
    match turn.cost {
        Some((cost, AiCostSource::Reported)) => usage.cost_reported_usd += cost,
        Some((cost, AiCostSource::Estimated)) => usage.cost_estimated_usd += cost,
        None => usage.unpriced_turns += 1,
    }
    usage.cost_usd = usage.cost_reported_usd + usage.cost_estimated_usd;
}

fn reports_cost(reply: &Reply) -> bool {
    reply
        .usage
        .as_ref()
        .and_then(|u| u.extra.get("cost"))
        .and_then(Value::as_f64)
        .is_some()
}

/// A stop as a fixed tag: no error or model text.
pub fn stop_tag(stop: &Stop) -> &'static str {
    match stop {
        Stop::Finished => "finished",
        Stop::TurnLimit => "turn_limit",
        Stop::SuggestionLimit => "suggestion_limit",
        Stop::MaxTokens => "max_tokens",
        Stop::Refused { .. } => "refused",
        Stop::Interrupted { .. } => "interrupted",
        Stop::Unexpected { .. } => "unexpected",
    }
}

fn log_finished(session: &Session<'_>, run_id: i64, stop: &'static str, started: Instant) {
    let usage = &session.usage;
    tracing::info!(
        event = "review_ai.finished",
        run_id,
        stop,
        notes = session.drafts.len(),
        turns = usage.turns,
        previews = usage.previews,
        rejected = usage.rejected,
        input_tokens = usage.input_tokens,
        output_tokens = usage.output_tokens,
        cache_read_input_tokens = usage.cache_read_input_tokens,
        cache_creation_input_tokens = usage.cache_creation_input_tokens,
        cost_usd = usage.cost_usd,
        cost_source = usage.cost_source(),
        wall_ms = started.elapsed().as_millis() as u64,
    );
}

fn text(text: impl Into<String>) -> AnthropicContentPart {
    AnthropicContentPart::text(text)
}

/// Write review notes for one run. `Err` only when the first request fails
/// or cannot be read; a failure after that keeps what was accepted and says
/// so in [`AiOutcome::stop`].
pub async fn generate(
    client: &AiClient,
    input: &ReviewContext,
    tools: &dyn ToolHost,
) -> Result<AiOutcome, AiError> {
    generate_observed(client, input, tools, &NoObserver).await
}

/// [`generate`], reporting each request, tool call and retry to `observer`
/// as it happens. Logs go to the caller's span: the server runs a pass inside
/// its job span, so every line carries the request that started the review.
pub async fn generate_observed(
    client: &AiClient,
    input: &ReviewContext,
    tools: &dyn ToolHost,
    observer: &dyn Observer,
) -> Result<AiOutcome, AiError> {
    let settings = client.settings();
    let started = Instant::now();
    let thinking = client.thinks().await;
    tracing::info!(
        event = "review_ai.started",
        run_id = input.run_id,
        model = %settings.model,
        thinking,
        max_turns = settings.max_turns,
        max_suggestions = settings.max_suggestions,
        max_previews = settings.max_previews,
        max_tokens = settings.max_tokens,
        rule_notes = input.existing.len(),
        context_bytes = input.text.len(),
    );
    let messages = vec![AnthropicMessage::with_parts(
        AnthropicRole::User,
        vec![
            text(input.text.as_str()),
            text(prompt::task(settings.max_suggestions)),
        ],
    )];
    converse(client, input, tools, observer, messages, started).await
}

/// The tool loop, from an opening conversation that ends on a user turn:
/// a review's single opening message, or a chat thread's history (see
/// [`chat`]). Everything past the opening is the same for both.
async fn converse(
    client: &AiClient,
    input: &ReviewContext,
    tools: &dyn ToolHost,
    observer: &dyn Observer,
    mut messages: Vec<AnthropicMessage>,
    started: Instant,
) -> Result<AiOutcome, AiError> {
    let settings = client.settings();
    let mut session = Session {
        client,
        context: input,
        tools,
        observer,
        drafts: Vec::new(),
        usage: Usage::default(),
        previewed: HashMap::new(),
        computed: HashMap::new(),
    };
    let mut model = None;
    let mut summary = None;

    let stop = loop {
        if session.usage.turns >= settings.max_turns {
            break Stop::TurnLimit;
        }
        observer.thinking();
        let reply = match client.request(&messages).await {
            Ok(request) => client.create(&request, observer).await,
            Err(error) => Err(error),
        };
        let (reply, seconds) = match reply {
            Ok(reply) => reply,
            Err(error) if session.usage.turns == 0 => {
                log_finished(&session, input.run_id, "failed", started);
                return Err(error);
            }
            Err(error) => {
                break Stop::Interrupted {
                    error: error.to_string(),
                };
            }
        };
        session.usage.turns += 1;
        // Prices are looked up only for a reply that reports no cost.
        let price = if reports_cost(&reply) {
            None
        } else {
            client.price().await
        };
        let turn = turn_report(session.usage.turns, seconds, &reply, price);
        add_usage(&mut session.usage, &turn);
        observer.turn(&turn);
        model = reply.model.clone().or(model);
        let calls = |tool: &str| {
            reply
                .content
                .iter()
                .filter(|p| matches!(p, AnthropicContentPart::ToolUse { name, .. } if name == tool))
                .count()
        };
        let tool_calls = reply
            .content
            .iter()
            .filter(|p| matches!(p, AnthropicContentPart::ToolUse { .. }))
            .count();
        tracing::info!(
            event = "review_ai.turn",
            turn = turn.turn,
            latency_ms = (turn.seconds * 1000.0) as u64,
            stop_reason = stop_label(reply.stop_reason.as_deref().unwrap_or_default()),
            input_tokens = turn.input_tokens,
            output_tokens = turn.output_tokens,
            cache_read_input_tokens = turn.cache_read_input_tokens,
            cache_creation_input_tokens = turn.cache_creation_input_tokens,
            cost_usd = turn.cost.map(|(c, _)| c),
            cost_source = turn.cost.map(|(_, s)| s.as_str()).unwrap_or("unknown"),
            preview_calls = calls(prompt::PREVIEW_TOOL),
            submit_calls = calls(prompt::SUBMIT_TOOL),
            other_calls = tool_calls - calls(prompt::PREVIEW_TOOL) - calls(prompt::SUBMIT_TOOL),
        );

        let stop_reason = reply.stop_reason.clone().unwrap_or_default();
        let content = reply.content;
        let said: Vec<&str> = content
            .iter()
            .filter_map(|part| match part {
                AnthropicContentPart::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if !said.is_empty() {
            summary = Some(said.join("\n").trim().to_owned()).filter(|s| !s.is_empty());
        }

        match stop_reason.as_str() {
            "end_turn" | "stop_sequence" => break Stop::Finished,
            // Check before touching content: a refusal's content is not an answer.
            "refusal" => {
                break Stop::Refused {
                    category: reply
                        .extra
                        .get("stop_details")
                        .and_then(|d| d.get("category"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                };
            }
            // A tool call cut off mid-input is not safe to run.
            "max_tokens" => break Stop::MaxTokens,
            "tool_use" => {}
            "" if session.usage.turns == 1 => {
                return Err(AiError::Malformed("reply has no stop reason".into()));
            }
            other => {
                break Stop::Unexpected {
                    stop_reason: other.to_owned(),
                };
            }
        }

        if let Some(said) = summary.as_deref().filter(|_| !said.is_empty()) {
            observer.narration(said);
        }
        let mut results = Vec::new();
        for part in &content {
            let AnthropicContentPart::ToolUse {
                id, name, input, ..
            } = part
            else {
                continue;
            };
            let input = input.clone().unwrap_or(Value::Null);
            let (output, is_error) = session.run_tool(id, name, &input).await;
            results.push(AnthropicContentPart::ToolResult {
                tool_use_id: id.clone(),
                content: Some(AnthropicMessageContent::Text(output)),
                is_error: is_error.then_some(true),
                cache_control: None,
            });
        }
        if results.is_empty() {
            break Stop::Unexpected {
                stop_reason: "tool_use without tool calls".into(),
            };
        }
        // The assistant turn goes back unchanged, thinking blocks included.
        messages.push(AnthropicMessage::with_parts(
            AnthropicRole::Assistant,
            content,
        ));
        // Every result in one user message, in call order.
        messages.push(AnthropicMessage::with_parts(AnthropicRole::User, results));

        if session.drafts.len() >= settings.max_suggestions {
            break Stop::SuggestionLimit;
        }
    };

    log_finished(&session, input.run_id, stop_tag(&stop), started);
    Ok(AiOutcome {
        drafts: session.drafts,
        stop,
        usage: session.usage,
        model,
        summary,
    })
}
