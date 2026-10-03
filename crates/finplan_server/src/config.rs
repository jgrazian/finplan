//! Server configuration, sourced from CLI flags and environment variables.

use clap::{Args, ValueEnum};

#[derive(Debug, Default, Clone, Copy, ValueEnum)]
pub enum LogFormat {
    #[default]
    Auto,
    Json,
    Text,
}

/// Commercial access policy is independent of hosted security protections.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HostedAccessMode {
    #[default]
    Subscription,
    Beta,
}

/// Server offload of local runs (spec 19, `api::compute`): the monthly budget
/// in cost units, `iterations x duration_years x max(accounts + assets +
/// events, 1)`, the same units `create_run` uses to refuse oversized runs. A
/// typical plan at 1,000 iterations costs a few hundred thousand.
#[derive(Debug, Clone, Args)]
pub struct OffloadConfig {
    /// Cost units a Free account may offload each calendar month (UTC).
    #[arg(
        long = "offload-budget-free",
        env = "FINPLAN_OFFLOAD_BUDGET_FREE",
        default_value_t = 20_000_000
    )]
    pub budget_free: i64,

    /// Cost units a Pro account (and any self-hosted or beta account) may
    /// offload each calendar month (UTC).
    #[arg(
        long = "offload-budget-pro",
        env = "FINPLAN_OFFLOAD_BUDGET_PRO",
        default_value_t = 1_000_000_000
    )]
    pub budget_pro: i64,
}

impl Default for OffloadConfig {
    fn default() -> Self {
        Self {
            budget_free: 20_000_000,
            budget_pro: 1_000_000_000,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct ServerConfig {
    #[command(flatten)]
    pub mail: crate::mail::MailConfig,
    /// Server offload of local runs: monthly budgets.
    #[command(flatten)]
    pub offload: OffloadConfig,
    /// AI review notes (`suggest::ai`).
    #[command(flatten)]
    pub review_ai: crate::suggest::ai::AiConfig,
    /// AI-guided scenario drafts (`api::drafts`).
    #[command(flatten)]
    pub draft: crate::suggest::ai::DraftConfig,
    /// Plan chat on the Review tab (`api::plan_chat`).
    #[command(flatten)]
    pub plan_chat: crate::suggest::ai::PlanChatConfig,
    /// Structured JSON in hosted mode, readable text locally when set to auto.
    #[arg(long, env = "FINPLAN_LOG_FORMAT", value_enum, default_value = "auto")]
    pub log_format: LogFormat,

    /// Optional separate metrics listener, e.g. 127.0.0.1:9090. Unset disables it.
    /// Non-loopback listeners must be protected by the deployment network.
    #[arg(long, env = "FINPLAN_METRICS_BIND")]
    pub metrics_bind: Option<std::net::SocketAddr>,

    /// Enable strict hosted origin and cookie protections.
    #[arg(long, env = "FINPLAN_HOSTED", default_value_t = false)]
    pub hosted: bool,

    /// Hosted access policy. Beta enables all planning features without a subscription.
    /// Ignored for self-hosted deployments; never disables hosted security checks.
    #[arg(
        long,
        env = "FINPLAN_ACCESS_MODE",
        value_enum,
        default_value = "subscription"
    )]
    pub access_mode: HostedAccessMode,

    /// Allow new accounts. Closing enrollment does not disable existing account sign-in.
    #[arg(long, env = "FINPLAN_REGISTRATION_OPEN", default_value_t = true, action = clap::ArgAction::Set)]
    pub registration_open: bool,

    /// Let a signed-out visitor start as a guest (spec 17). Hosted only:
    /// self-hosted deployments always allow guests, uncapped.
    #[arg(long, env = "FINPLAN_GUEST_ACCESS", default_value_t = true, action = clap::ArgAction::Set)]
    pub guest_access: bool,

    /// Local mode (spec 19): plans live in the visitor's browser by default and
    /// the engine runs there too. Served to the web on `/api/health`. A
    /// self-hosted operator who wants plans on their own server sets this to
    /// false.
    #[arg(long, env = "FINPLAN_LOCAL_MODE", default_value_t = true, action = clap::ArgAction::Set)]
    pub local_mode: bool,

    /// Iterations a hosted guest may run on any simulation path.
    #[arg(long, env = "FINPLAN_GUEST_MAX_ITERATIONS", default_value_t = 100)]
    pub guest_max_iterations: usize,

    /// Days without a visit after which a guest and its plans are deleted.
    #[arg(long, env = "FINPLAN_GUEST_RETENTION_DAYS", default_value_t = 30)]
    pub guest_retention_days: i64,

    /// Explicit local development mail sink directory; never available in hosted mode.
    #[arg(long, env = "FINPLAN_LOCAL_MAIL_SINK")]
    pub local_mail_sink: Option<String>,

    /// Address to bind the HTTP listener to.
    #[arg(long, env = "FINPLAN_BIND", default_value = "127.0.0.1:8080")]
    pub bind: String,

    /// SQLite database URL, e.g. `sqlite://finplan.db`.
    #[arg(long, env = "DATABASE_URL", default_value = "sqlite://finplan.db")]
    pub database_url: String,

    /// Maximum SQLite pool connections.
    #[arg(long, env = "FINPLAN_DB_POOL", default_value_t = 8)]
    pub db_pool_size: u32,

    /// How many Monte Carlo runs may execute concurrently. Each run already
    /// parallelizes internally, so the default is deliberately small.
    #[arg(long, env = "FINPLAN_SIM_WORKERS", default_value_t = 2)]
    pub sim_workers: usize,

    /// Upper bound on iterations a single run may request.
    #[arg(long, env = "FINPLAN_MAX_ITERATIONS", default_value_t = 50_000)]
    pub max_iterations: usize,

    /// Set the `Secure` attribute on session cookies. Enable behind TLS.
    #[arg(long, env = "FINPLAN_SECURE_COOKIES", default_value_t = false)]
    pub secure_cookies: bool,

    /// Origins permitted by CORS, comma-separated. The Next.js dev server runs
    /// on a different port, so it needs an explicit entry.
    #[arg(
        long,
        env = "FINPLAN_CORS_ORIGINS",
        value_delimiter = ',',
        default_value = "http://localhost:3000"
    )]
    pub cors_origins: Vec<String>,
}

impl ServerConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.mail
            .validate(self.hosted, self.local_mail_sink.as_deref())?;
        self.review_ai.validate()?;
        self.draft.validate()?;
        self.plan_chat.validate()?;
        if self.hosted
            && (!self.secure_cookies
                || self.local_mail_sink.is_some()
                || self.cors_origins.is_empty()
                || self.cors_origins.iter().any(|o| {
                    let Ok(uri) = o.parse::<axum::http::Uri>() else {
                        return true;
                    };
                    uri.scheme_str() != Some("https")
                        || uri.authority().is_none()
                        || uri
                            .authority()
                            .is_some_and(|a| a.as_str().contains('@') || a.as_str().contains('*'))
                        || uri.path() != "/"
                        || uri.query().is_some()
                        || o.ends_with('/')
                        || o.trim() != o
                }))
        {
            return Err("hosted mode requires secure cookies, explicit HTTPS origins, and no local mail sink".into());
        }
        if self.sim_workers == 0 {
            return Err("simulation workers must be positive".into());
        }
        if self.guest_max_iterations == 0 || self.guest_retention_days < 1 {
            return Err("guest iterations and retention must be positive".into());
        }
        if self.max_iterations == 0 {
            return Err("maximum iterations must be positive".into());
        }
        if self.offload.budget_free < 0 || self.offload.budget_pro < 0 {
            return Err("offload budgets must not be negative".into());
        }
        Ok(())
    }
}
