//! Internal clocks and drop guards shared by the two independent compute pools.
use std::time::Instant;

use crate::error::{ApiError, ApiResult};
use crate::observability::{
    Component, ErrorClass, JobContext, JobKind, Outcome, Phase, SubmissionResult, Telemetry,
};

#[derive(Clone)]
pub(crate) struct Submitted {
    at: Instant,
    initial_age: f64,
}
impl Submitted {
    pub(crate) fn now() -> Self {
        Self {
            at: Instant::now(),
            initial_age: 0.0,
        }
    }
    pub(crate) fn recovered(age: f64, telemetry: &Telemetry) -> Self {
        if age < 0.0 {
            telemetry.count_error(Component::Recovery, ErrorClass::Clock);
            tracing::warn!(event = "recovery.clock_skew", error_class = "clock");
        }
        Self {
            at: Instant::now(),
            initial_age: age.max(0.0),
        }
    }
    pub(crate) fn elapsed(&self) -> f64 {
        self.initial_age + self.at.elapsed().as_secs_f64()
    }
}

/// The API records acceptance at the commit/insertion boundary, even when its
/// subsequent response reload fails. Other decisions are recorded on return.
pub(crate) struct Submission {
    telemetry: Telemetry,
    kind: JobKind,
    accepted: bool,
}
impl Submission {
    pub(crate) fn new(telemetry: &Telemetry, kind: JobKind) -> Self {
        Self {
            telemetry: telemetry.clone(),
            kind,
            accepted: false,
        }
    }
    pub(crate) fn accepted(&mut self) {
        self.accepted = true;
        self.telemetry
            .submission(self.kind, SubmissionResult::Accepted);
    }
    pub(crate) fn result<T>(&self, result: &ApiResult<T>) {
        if self.accepted {
            return;
        }
        let decision = match result {
            Err(ApiError::Conflict(_)) => SubmissionResult::CapacityRejected,
            Err(ApiError::Database(_) | ApiError::Internal(_)) => SubmissionResult::InternalError,
            _ => SubmissionResult::Invalid,
        };
        self.telemetry.submission(self.kind, decision);
    }
}

pub(crate) struct Attempt {
    telemetry: Telemetry,
    context: JobContext,
    submitted: Submitted,
    started: Instant,
    finished: bool,
    failure: Option<ErrorClass>,
}
impl Attempt {
    pub(crate) fn new(telemetry: &Telemetry, context: &JobContext, submitted: Submitted) -> Self {
        Self {
            telemetry: telemetry.clone(),
            context: context.clone(),
            submitted,
            started: Instant::now(),
            finished: false,
            failure: None,
        }
    }
    pub(crate) fn failure(&mut self, class: ErrorClass) {
        self.failure = Some(class);
    }
    pub(crate) fn finish(&mut self, outcome: Outcome) {
        if self.finished {
            return;
        }
        self.finished = true;
        let processing = self.started.elapsed().as_secs_f64();
        let end_to_end = self.submitted.elapsed();
        self.telemetry
            .attempt_completed(self.context.kind, outcome, processing, end_to_end);
        let _entered = self.context.span().entered();
        let event = match (self.context.kind, outcome) {
            (JobKind::Run, Outcome::Succeeded) => "run.succeeded",
            (JobKind::Run, Outcome::Failed) => "run.failed",
            (JobKind::Run, Outcome::Canceled) => "run.canceled",
            (JobKind::Run, Outcome::Interrupted) => "run.interrupted",
            (_, Outcome::Succeeded) => "analysis.succeeded",
            (_, Outcome::Failed) => "analysis.failed",
            (_, Outcome::Canceled) => "analysis.canceled",
            (_, Outcome::Interrupted) => "analysis.interrupted",
        };
        // Failure diagnostics have one separate ERROR owner; this terminal
        // observation carries only the bounded outcome and elapsed times.
        if outcome == Outcome::Failed {
            tracing::error!(
                event,
                outcome = outcome.as_str(),
                error_class = self.failure.unwrap_or(ErrorClass::Internal).as_str(),
                processing_seconds = processing,
                end_to_end_seconds = end_to_end
            );
        } else {
            tracing::info!(
                event,
                outcome = outcome.as_str(),
                processing_seconds = processing,
                end_to_end_seconds = end_to_end
            );
        }
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(Outcome::Interrupted);
        }
    }
}

pub(crate) struct PhaseTimer {
    telemetry: Telemetry,
    kind: JobKind,
    phase: Phase,
    started: Instant,
}
impl PhaseTimer {
    pub(crate) fn new(telemetry: &Telemetry, kind: JobKind, phase: Phase) -> Self {
        Self {
            telemetry: telemetry.clone(),
            kind,
            phase,
            started: Instant::now(),
        }
    }
}
impl Drop for PhaseTimer {
    fn drop(&mut self) {
        self.telemetry
            .phase(self.kind, self.phase, self.started.elapsed().as_secs_f64());
    }
}

/// Aborting the async owner must not leave a progress writer polling forever.
pub(crate) struct AbortTask(pub tokio::task::JoinHandle<()>);
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
