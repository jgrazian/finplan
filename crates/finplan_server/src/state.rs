//! Shared application state handed to every route.

use std::sync::Arc;

use crate::analysis::AnalysisJobs;
use crate::api::review_ai::AiReviews;
use crate::auth::session::CurrentUser;
use crate::config::ServerConfig;
use crate::db::Db;
use crate::observability::Telemetry;
use crate::runner::RunQueue;

#[derive(Clone)]
pub struct AppState {
    pub telemetry: Telemetry,
    pub db: Db,
    pub config: Arc<ServerConfig>,
    pub runs: RunQueue,
    /// Sweeps, sensitivity rankings and goal seeks. Unlike runs these are held
    /// in memory; only the newest sweep of each scenario is written back, so
    /// the Analysis screen survives a reload. See `analysis`.
    pub analyses: AnalysisJobs,
    /// The review model, when one is configured: model-written review notes
    /// run in the background after the rule notes. See `api::review_ai`.
    pub review_ai: Option<AiReviews>,
}

impl AppState {
    /// A guest on a hosted server, where the guest limits apply (spec 17).
    /// Self-hosted guests are the single local user and are not limited.
    #[must_use]
    pub fn limited_guest(&self, user: &CurrentUser) -> bool {
        user.guest && self.config.hosted
    }

    /// The review model, as this caller may use it: none for a limited guest,
    /// who gets no AI drafts, chat or model-written notes. Rule notes still run.
    #[must_use]
    pub fn review_ai_for(&self, user: &CurrentUser) -> Option<&AiReviews> {
        self.review_ai
            .as_ref()
            .filter(|_| !self.limited_guest(user))
    }
}
