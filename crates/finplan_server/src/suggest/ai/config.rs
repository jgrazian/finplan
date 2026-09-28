//! Operator settings for AI review notes, read from flags and the environment.

use std::fmt;
use std::time::Duration;

use clap::{Args, ValueEnum};

/// The OpenRouter model review notes are written with: an analytical,
/// tool-using job whose output lands in front of users as advice about their
/// plan. Override with `FINPLAN_REVIEW_MODEL` (e.g. an Opus-tier model).
pub const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-5.5";

/// OpenRouter's API root; `/messages` is appended per request.
pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// The `X-Title` OpenRouter shows for this app.
pub const DEFAULT_APP_TITLE: &str = "FinPlan";

/// Materiality defaults: what an optimization or risk note must move to be
/// worth a user's attention (see `AiConfig::min_rate_pts`).
pub const DEFAULT_MIN_RATE_PTS: f64 = 1.0;
pub const DEFAULT_MIN_MEDIAN_PCT: f64 = 5.0;
pub const DEFAULT_MIN_P10_PCT: f64 = 10.0;

/// Whether requests ask for adaptive thinking and an effort level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ThinkingMode {
    /// Only for Anthropic models (`anthropic/...`), which support both.
    #[default]
    Auto,
    On,
    Off,
}

impl ThinkingMode {
    /// Whether a request to `model` carries the thinking settings.
    pub fn applies_to(self, model: &str) -> bool {
        match self {
            ThinkingMode::Auto => model.trim().starts_with("anthropic/"),
            ThinkingMode::On => true,
            ThinkingMode::Off => false,
        }
    }
}

#[derive(Clone, Args)]
pub struct AiConfig {
    /// Write AI review notes alongside the rule-based ones. Needs an
    /// OpenRouter API key; without one the review stays rules-only.
    #[arg(long = "review-ai", env = "FINPLAN_REVIEW_AI", default_value_t = false, action = clap::ArgAction::Set)]
    pub enabled: bool,

    /// OpenRouter API key used for review notes.
    #[arg(long, env = "FINPLAN_OPENROUTER_API_KEY", hide_env_values = true)]
    pub openrouter_api_key: Option<String>,

    /// OpenRouter API root. Only a proxy or a test double needs another.
    #[arg(long, env = "FINPLAN_OPENROUTER_BASE_URL", default_value = DEFAULT_BASE_URL)]
    pub openrouter_base_url: String,

    /// App name OpenRouter attributes requests to (`X-Title`).
    #[arg(long, env = "FINPLAN_OPENROUTER_APP_TITLE", default_value = DEFAULT_APP_TITLE)]
    pub openrouter_app_title: String,

    /// App URL OpenRouter attributes requests to (`HTTP-Referer`).
    #[arg(long, env = "FINPLAN_OPENROUTER_REFERER")]
    pub openrouter_referer: Option<String>,

    /// OpenRouter model id the review notes are written with, e.g.
    /// `anthropic/claude-sonnet-5.5`.
    #[arg(long = "review-model", env = "FINPLAN_REVIEW_MODEL", default_value = DEFAULT_MODEL)]
    pub model: String,

    /// Adaptive thinking and effort: `auto` sends them to Anthropic models
    /// only.
    #[arg(
        long = "review-thinking",
        env = "FINPLAN_REVIEW_THINKING",
        value_enum,
        default_value_t = ThinkingMode::Auto
    )]
    pub thinking: ThinkingMode,

    /// Most model requests one review may make.
    #[arg(
        long = "review-max-turns",
        env = "FINPLAN_REVIEW_MAX_TURNS",
        default_value_t = 12
    )]
    pub max_turns: u32,

    /// Most notes one review may add.
    #[arg(
        long = "review-max-suggestions",
        env = "FINPLAN_REVIEW_MAX_SUGGESTIONS",
        default_value_t = 6
    )]
    pub max_suggestions: usize,

    /// Most simulated previews one review may ask for.
    #[arg(
        long = "review-max-previews",
        env = "FINPLAN_REVIEW_MAX_PREVIEWS",
        default_value_t = 8
    )]
    pub max_previews: u32,

    /// Output token ceiling per model request, thinking included.
    #[arg(
        long = "review-max-tokens",
        env = "FINPLAN_REVIEW_MAX_TOKENS",
        default_value_t = 16_000
    )]
    pub max_tokens: u32,

    /// Seconds one model request may take before it is abandoned.
    #[arg(
        long = "review-timeout-secs",
        env = "FINPLAN_REVIEW_TIMEOUT_SECS",
        default_value_t = 300
    )]
    pub timeout_secs: u64,

    /// Materiality, for notes the model files as optimization or risk: a
    /// path must move success or funding success by at least this many
    /// percentage points ...
    #[arg(
        long = "review-min-rate-pts",
        env = "FINPLAN_REVIEW_MIN_RATE_PTS",
        default_value_t = DEFAULT_MIN_RATE_PTS
    )]
    pub min_rate_pts: f64,

    /// ... or the real median final net worth by at least this percent ...
    #[arg(
        long = "review-min-median-pct",
        env = "FINPLAN_REVIEW_MIN_MEDIAN_PCT",
        default_value_t = DEFAULT_MIN_MEDIAN_PCT
    )]
    pub min_median_pct: f64,

    /// ... or the real P10 final net worth by at least this percent.
    #[arg(
        long = "review-min-p10-pct",
        env = "FINPLAN_REVIEW_MIN_P10_PCT",
        default_value_t = DEFAULT_MIN_P10_PCT
    )]
    pub min_p10_pct: f64,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            openrouter_api_key: None,
            openrouter_base_url: DEFAULT_BASE_URL.into(),
            openrouter_app_title: DEFAULT_APP_TITLE.into(),
            openrouter_referer: None,
            model: DEFAULT_MODEL.into(),
            thinking: ThinkingMode::Auto,
            max_turns: 12,
            max_suggestions: 6,
            max_previews: 8,
            max_tokens: 16_000,
            timeout_secs: 300,
            min_rate_pts: DEFAULT_MIN_RATE_PTS,
            min_median_pct: DEFAULT_MIN_MEDIAN_PCT,
            min_p10_pct: DEFAULT_MIN_P10_PCT,
        }
    }
}

// ServerConfig derives Debug: never let that expose the API key.
impl fmt::Debug for AiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiConfig")
            .field("enabled", &self.enabled)
            .field(
                "openrouter_api_key",
                &self.openrouter_api_key.as_ref().map(|_| "<redacted>"),
            )
            .field("openrouter_base_url", &self.openrouter_base_url)
            .field("openrouter_app_title", &self.openrouter_app_title)
            .field("openrouter_referer", &self.openrouter_referer)
            .field("model", &self.model)
            .field("thinking", &self.thinking)
            .field("max_turns", &self.max_turns)
            .field("max_suggestions", &self.max_suggestions)
            .field("max_previews", &self.max_previews)
            .field("max_tokens", &self.max_tokens)
            .field("timeout_secs", &self.timeout_secs)
            .field("min_rate_pts", &self.min_rate_pts)
            .field("min_median_pct", &self.min_median_pct)
            .field("min_p10_pct", &self.min_p10_pct)
            .finish()
    }
}

impl AiConfig {
    /// The key, when AI notes are switched on and one is configured. Enabled
    /// without a key means rules-only, not a startup failure: the flag is
    /// often set before the secret is provisioned.
    pub fn active_key(&self) -> Option<&str> {
        self.openrouter_api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| self.enabled && !key.is_empty())
    }

    /// Enabled but missing its key — worth a startup warning.
    pub fn missing_key(&self) -> bool {
        self.enabled && self.active_key().is_none()
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.max_turns == 0 || self.max_turns > 50 {
            return Err("review max turns must be between 1 and 50".into());
        }
        if self.max_suggestions == 0 || self.max_suggestions > 20 {
            return Err("review max suggestions must be between 1 and 20".into());
        }
        if self.max_previews > 50 {
            return Err("review max previews must be at most 50".into());
        }
        if !(1_024..=128_000).contains(&self.max_tokens) {
            return Err("review max tokens must be between 1024 and 128000".into());
        }
        if self.timeout_secs == 0 {
            return Err("review timeout must be positive".into());
        }
        if self.model.trim().is_empty() {
            return Err("review model must not be empty".into());
        }
        for (name, value, max) in [
            ("review min rate points", self.min_rate_pts, 100.0),
            ("review min median percent", self.min_median_pct, 1_000.0),
            ("review min P10 percent", self.min_p10_pct, 1_000.0),
        ] {
            if !value.is_finite() || !(0.0..=max).contains(&value) {
                return Err(format!("{name} must be between 0 and {max}"));
            }
        }
        let url = self.openrouter_base_url.trim_end_matches('/');
        if !(url.starts_with("https://")
            || url.starts_with("http://127.0.0.1")
            || url.starts_with("http://localhost"))
        {
            return Err("the OpenRouter base URL must be HTTPS (or a loopback test double)".into());
        }
        if self.openrouter_app_title.trim().is_empty() {
            return Err("the OpenRouter app title must not be empty".into());
        }
        if self
            .openrouter_referer
            .as_deref()
            .is_some_and(|r| !(r.starts_with("https://") || r.starts_with("http://")))
        {
            return Err("the OpenRouter referer must be an http(s) URL".into());
        }
        Ok(())
    }
}
