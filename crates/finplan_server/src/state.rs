//! Shared application state handed to every route.

use std::sync::Arc;

use crate::analysis::AnalysisJobs;
use crate::config::ServerConfig;
use crate::db::Db;
use crate::runner::RunQueue;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: Arc<ServerConfig>,
    pub runs: RunQueue,
    /// Sweeps, sensitivity rankings and goal seeks. Unlike runs these are not
    /// persisted — see `analysis`.
    pub analyses: AnalysisJobs,
}
