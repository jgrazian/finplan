//! Server configuration, sourced from CLI flags and environment variables.

use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct ServerConfig {
    /// Enable strict hosted origin and cookie protections.
    #[arg(long, env = "FINPLAN_HOSTED", default_value_t = false)]
    pub hosted: bool,

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
        Ok(())
    }
}
