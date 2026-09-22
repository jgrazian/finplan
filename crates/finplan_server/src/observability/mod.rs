//! Per-application, privacy-conscious operational telemetry.
//!
//! Identifiers belong in logs only. Metric labels are bounded enums or Axum
//! route templates; callers cannot supply arbitrary label values.

mod context;
mod events;
mod extrema;
mod http;
mod metrics;
mod runtime;

pub use context::{JobContext, RequestContext};
pub use events::EventFields;
pub(crate) use http::HttpFailure;
pub use http::{metrics_router, request_telemetry};
pub use metrics::{QueueSnapshot, RunningGuard, Telemetry};
pub(crate) use runtime::purge_sessions;
pub use runtime::{ObservabilityRuntime, sample};

macro_rules! bounded_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $value),+ }
            }
        }
    };
}

bounded_enum!(Resource {
    Account => "account", Position => "position", Scenario => "scenario",
    Asset => "asset", Event => "event", ReturnProfile => "return_profile",
    InflationProfile => "inflation_profile", TaxConfig => "tax_config",
    Onboarding => "onboarding", Archive => "archive", Run => "run", User => "user",
    ContactMessage => "contact_message"
});
bounded_enum!(Operation {
    Created => "created", Updated => "updated", Deleted => "deleted",
    Reordered => "reordered", Duplicated => "duplicated", Completed => "completed",
    Imported => "imported", Exported => "exported"
});
bounded_enum!(AuthAction {
    Register => "register", Login => "login", Logout => "logout",
    PasswordChanged => "password_changed", ResetRequested => "reset_requested",
    ResetCompleted => "reset_completed", VerificationRequested => "verification_requested",
    EmailVerified => "email_verified", SessionRevoked => "session_revoked", Throttled => "throttled"
});
bounded_enum!(AuthOutcome {
    Succeeded => "succeeded", Failed => "failed", Rejected => "rejected", Replay => "replay"
});
bounded_enum!(Component {
    Http => "http", Auth => "auth", Session => "session", Run => "run",
    Analysis => "analysis", Recovery => "recovery", QueueSampler => "queue_sampler",
    Billing => "billing", Metrics => "metrics", Server => "server"
});
bounded_enum!(ErrorClass {
    Database => "database", Internal => "internal", Preparation => "preparation",
    Engine => "engine", EnginePanic => "engine_panic", Persistence => "persistence",
    QueueClosed => "queue_closed", Unavailable => "unavailable", Clock => "clock",
    TaskPanic => "task_panic"
});
bounded_enum!(JobKind { Run => "run", Sweep => "sweep", Sensitivity => "sensitivity", Solve => "solve" });
bounded_enum!(Outcome { Succeeded => "succeeded", Failed => "failed", Canceled => "canceled", Interrupted => "interrupted" });
bounded_enum!(Origin { Request => "request", Recovery => "recovery" });
bounded_enum!(SubmissionResult { Accepted => "accepted", Invalid => "invalid", CapacityRejected => "capacity_rejected", InternalError => "internal_error" });
bounded_enum!(RejectionReason { GlobalLimit => "global_limit", UserLimit => "user_limit", QueueFull => "queue_full", QueueClosed => "queue_closed" });
bounded_enum!(Phase { Prepare => "prepare", BlockingWait => "blocking_wait", Engine => "engine", Persist => "persist" });
bounded_enum!(QueueExit { Started => "started", Canceled => "canceled", Deleted => "deleted" });
