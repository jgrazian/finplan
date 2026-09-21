use std::sync::{Arc, Mutex};

use super::{JobKind, Origin};

tokio::task_local! {
    pub(crate) static REQUEST_CONTEXT: RequestContext;
}

/// Locally generated correlation context; no inbound correlation headers are trusted.
#[derive(Clone, Debug)]
pub struct RequestContext {
    pub request_id: String,
    pub method: &'static str,
    pub route: String,
    user_id: Arc<Mutex<Option<String>>>,
}

impl RequestContext {
    pub(crate) fn new(method: &'static str, route: String) -> Self {
        Self {
            request_id: uuid::Uuid::new_v4().to_string(),
            method,
            route,
            user_id: Arc::new(Mutex::new(None)),
        }
    }

    pub fn current() -> Option<Self> {
        REQUEST_CONTEXT.try_with(Clone::clone).ok()
    }

    /// Call only after successful authentication; never repeat authentication for logging.
    pub fn authenticate(user_id: &str) {
        if let Some(context) = Self::current() {
            *context.user_id.lock().unwrap_or_else(|e| e.into_inner()) = Some(user_id.to_owned());
            tracing::Span::current().record("user_id", user_id);
        }
    }

    pub fn user_id(&self) -> Option<String> {
        self.user_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// Explicitly carry this context into asynchronous workers and blocking closures.
#[derive(Clone, Debug)]
pub struct JobContext {
    pub request_id: Option<String>,
    pub user_id: String,
    pub scenario_id: i64,
    pub job_id: i64,
    pub kind: JobKind,
    pub origin: Origin,
}

impl JobContext {
    pub fn new(
        kind: JobKind,
        origin: Origin,
        user_id: &str,
        scenario_id: i64,
        job_id: i64,
    ) -> Self {
        Self {
            request_id: (origin == Origin::Request)
                .then(RequestContext::current)
                .flatten()
                .map(|c| c.request_id),
            user_id: user_id.to_owned(),
            scenario_id,
            job_id,
            kind,
            origin,
        }
    }

    pub fn span(&self) -> tracing::Span {
        tracing::info_span!(
            "job", request_id = self.request_id.as_deref().unwrap_or(""),
            user_id = %self.user_id, scenario_id = self.scenario_id,
            job_id = self.job_id, kind = self.kind.as_str(), origin = self.origin.as_str()
        )
    }

    /// Event names must be application-owned constants; never pass engine error text.
    pub fn event(&self, event: &'static str) {
        tracing::info!(event, request_id = self.request_id.as_deref().unwrap_or(""),
            user_id = %self.user_id, scenario_id = self.scenario_id,
            job_id = self.job_id, kind = self.kind.as_str(), origin = self.origin.as_str());
    }
}
