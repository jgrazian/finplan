//! Server configuration, sourced from CLI flags and environment variables.

use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct ServerConfig {
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
