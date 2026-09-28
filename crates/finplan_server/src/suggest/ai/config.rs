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

/// One tier's limits on AI-guided drafts (`api::drafts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraftLimits {
    /// Drafts a user may start per calendar month (UTC).
    pub drafts_per_month: u32,
    /// Files, bytes and pages (PDF pages plus images) one draft may take in.
    pub max_files: u32,
    pub max_bytes: u64,
    pub max_pages: u32,
}

const MB: u64 = 1024 * 1024;

/// Operator settings for AI-guided scenario drafts: the per-tier limits, how
/// long an abandoned draft lives, and the model budget of one drafting pass
/// (its own values, in the shape of the review settings above).
///
/// Every limit is validated at startup and reported to the web through
/// `Entitlements::ai_drafts`.
#[derive(Debug, Clone, Args)]
pub struct DraftConfig {
    /// AI drafts a Free user may start per calendar month (UTC).
    #[arg(
        long = "draft-free-per-month",
        env = "FINPLAN_DRAFT_FREE_PER_MONTH",
        default_value_t = 2
    )]
    pub free_per_month: u32,
    /// AI drafts a Pro user may start per calendar month (UTC).
    #[arg(
        long = "draft-pro-per-month",
        env = "FINPLAN_DRAFT_PRO_PER_MONTH",
        default_value_t = 20
    )]
    pub pro_per_month: u32,

    /// Files one draft may attach, per tier.
    #[arg(
        long = "draft-free-max-files",
        env = "FINPLAN_DRAFT_FREE_MAX_FILES",
        default_value_t = 10
    )]
    pub free_max_files: u32,
    #[arg(
        long = "draft-pro-max-files",
        env = "FINPLAN_DRAFT_PRO_MAX_FILES",
        default_value_t = 25
    )]
    pub pro_max_files: u32,

    /// Bytes one draft may attach, per tier.
    #[arg(
        long = "draft-free-max-bytes",
        env = "FINPLAN_DRAFT_FREE_MAX_BYTES",
        default_value_t = 25 * MB
    )]
    pub free_max_bytes: u64,
    #[arg(
        long = "draft-pro-max-bytes",
        env = "FINPLAN_DRAFT_PRO_MAX_BYTES",
        default_value_t = 100 * MB
    )]
    pub pro_max_bytes: u64,

    /// Pages (PDF pages plus images) one draft may attach, per tier.
    #[arg(
        long = "draft-free-max-pages",
        env = "FINPLAN_DRAFT_FREE_MAX_PAGES",
        default_value_t = 60
    )]
    pub free_max_pages: u32,
    #[arg(
        long = "draft-pro-max-pages",
        env = "FINPLAN_DRAFT_PRO_MAX_PAGES",
        default_value_t = 200
    )]
    pub pro_max_pages: u32,

    /// Hours an untouched draft lives before the sweeper deletes it. Covers a
    /// closed tab or a crash; cancelling deletes a draft at once.
    #[arg(
        long = "draft-ttl-hours",
        env = "FINPLAN_DRAFT_TTL_HOURS",
        default_value_t = 24
    )]
    pub ttl_hours: u32,

    /// Where a draft's images and scanned PDFs (files with no text layer)
    /// wait for the model, in one folder per draft. They are the only original
    /// files ever written down, are never in the database, and go with the
    /// draft (cancel, create, replace or sweep). Defaults to a `finplan-draft-files`
    /// folder under the OS temp directory.
    #[arg(long = "draft-temp-dir", env = "FINPLAN_DRAFT_TEMP_DIR")]
    pub temp_dir: Option<std::path::PathBuf>,

    /// Route drafting requests only to zero-data-retention providers, on top
    /// of the no-data-collection rule. Drafts carry statements and tax
    /// returns, so this is on by default; turn it off only when no ZDR
    /// endpoint serves the drafting model.
    #[arg(
        long = "draft-require-zdr",
        env = "FINPLAN_DRAFT_REQUIRE_ZDR",
        default_value_t = true,
        action = clap::ArgAction::Set
    )]
    pub require_zdr: bool,

    /// Most model requests one drafting pass may make.
    #[arg(
        id = "draft_max_turns",
        long = "draft-max-turns",
        env = "FINPLAN_DRAFT_MAX_TURNS",
        default_value_t = 20
    )]
    pub max_turns: u32,

    /// Most simulated previews one drafting pass may ask for.
    #[arg(
        id = "draft_max_previews",
        long = "draft-max-previews",
        env = "FINPLAN_DRAFT_MAX_PREVIEWS",
        default_value_t = 12
    )]
    pub max_previews: u32,

    /// Output token ceiling per drafting request, thinking included.
    #[arg(
        id = "draft_max_tokens",
        long = "draft-max-tokens",
        env = "FINPLAN_DRAFT_MAX_TOKENS",
        default_value_t = 32_000
    )]
    pub max_tokens: u32,
}

impl Default for DraftConfig {
    fn default() -> Self {
        Self {
            free_per_month: 2,
            pro_per_month: 20,
            free_max_files: 10,
            pro_max_files: 25,
            free_max_bytes: 25 * MB,
            pro_max_bytes: 100 * MB,
            free_max_pages: 60,
            pro_max_pages: 200,
            ttl_hours: 24,
            temp_dir: None,
            require_zdr: true,
            max_turns: 20,
            max_previews: 12,
            max_tokens: 32_000,
        }
    }
}

impl DraftConfig {
    /// The limits that apply to a Pro or a Free user.
    pub fn limits(&self, pro: bool) -> DraftLimits {
        if pro {
            DraftLimits {
                drafts_per_month: self.pro_per_month,
                max_files: self.pro_max_files,
                max_bytes: self.pro_max_bytes,
                max_pages: self.pro_max_pages,
            }
        } else {
            DraftLimits {
                drafts_per_month: self.free_per_month,
                max_files: self.free_max_files,
                max_bytes: self.free_max_bytes,
                max_pages: self.free_max_pages,
            }
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let limits: [(&str, u64, u64, u64); 4] = [
            (
                "draft drafts per month",
                self.free_per_month.into(),
                self.pro_per_month.into(),
                1_000,
            ),
            (
                "draft max files",
                self.free_max_files.into(),
                self.pro_max_files.into(),
                200,
            ),
            (
                "draft max pages",
                self.free_max_pages.into(),
                self.pro_max_pages.into(),
                5_000,
            ),
            (
                "draft max bytes",
                self.free_max_bytes,
                self.pro_max_bytes,
                1_024 * MB,
            ),
        ];
        for (name, free, pro, max) in limits {
            if free == 0 || pro == 0 || free > max || pro > max {
                return Err(format!("{name} must be between 1 and {max}"));
            }
            if pro < free {
                return Err(format!("{name} for Pro must not be below Free"));
            }
        }
        if self.ttl_hours == 0 || self.ttl_hours > 24 * 30 {
            return Err("draft ttl hours must be between 1 and 720".into());
        }
        if self.max_turns == 0 || self.max_turns > 100 {
            return Err("draft max turns must be between 1 and 100".into());
        }
        if self.max_previews > 100 {
            return Err("draft max previews must be at most 100".into());
        }
        if !(1_024..=128_000).contains(&self.max_tokens) {
            return Err("draft max tokens must be between 1024 and 128000".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod draft_tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        review: AiConfig,
        #[command(flatten)]
        draft: DraftConfig,
    }

    #[test]
    fn the_defaults_are_the_documented_limits_and_valid() {
        let config = Cli::parse_from(["test"]).draft;
        config.validate().unwrap();
        assert_eq!(
            config.limits(false),
            DraftLimits {
                drafts_per_month: 2,
                max_files: 10,
                max_bytes: 25 * 1024 * 1024,
                max_pages: 60,
            }
        );
        assert_eq!(
            config.limits(true),
            DraftLimits {
                drafts_per_month: 20,
                max_files: 25,
                max_bytes: 100 * 1024 * 1024,
                max_pages: 200,
            }
        );
        assert_eq!(config.ttl_hours, 24);
        // Its own model budget, beside the review's rather than shared with it.
        assert_eq!((config.max_turns, config.max_previews), (20, 12));
        assert_eq!(Cli::parse_from(["test"]).review.max_turns, 12);
    }

    #[test]
    fn flags_override_and_bad_limits_are_refused_at_startup() {
        let config = Cli::parse_from([
            "test",
            "--draft-free-per-month",
            "5",
            "--draft-pro-max-files",
            "40",
            "--draft-max-turns",
            "9",
            "--review-max-turns",
            "7",
        ]);
        assert_eq!(config.draft.limits(false).drafts_per_month, 5);
        assert_eq!(config.draft.limits(true).max_files, 40);
        assert_eq!((config.draft.max_turns, config.review.max_turns), (9, 7));
        config.draft.validate().unwrap();

        let broken: [fn(&mut DraftConfig); 8] = [
            |c| c.free_per_month = 0,
            |c| c.pro_per_month = 1,
            |c| c.free_max_files = 0,
            |c| c.free_max_bytes = 2 * 1024 * 1024 * 1024,
            |c| c.free_max_pages = 0,
            |c| c.ttl_hours = 0,
            |c| c.max_turns = 0,
            |c| c.max_tokens = 10,
        ];
        for break_it in broken {
            let mut config = DraftConfig::default();
            break_it(&mut config);
            assert!(config.validate().is_err(), "{config:?}");
        }
    }
}
