use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::{
    counter::Counter, family::Family, gauge::Gauge, histogram::Histogram,
};
use prometheus_client::registry::Registry;

use super::extrema::RecentExtrema;
use super::*;

type Labels = Vec<(String, String)>;
type FloatGauge = Gauge<f64, AtomicU64>;
type Histograms = Family<Labels, Histogram, fn() -> Histogram>;
const HTTP_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
];
const JOB_BUCKETS: &[f64] = &[
    0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0,
    3600.0, 7200.0,
];
const SPEED_BUCKETS: &[f64] = &[
    1.0, 10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0, 25000.0, 50000.0,
    100000.0,
];

fn labels<const N: usize>(pairs: [(&str, &str); N]) -> Labels {
    pairs
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}
fn job_labels(kind: JobKind) -> Labels {
    labels([("kind", kind.as_str())])
}
fn outcome_labels(kind: JobKind, outcome: Outcome) -> Labels {
    labels([("kind", kind.as_str()), ("outcome", outcome.as_str())])
}
fn job_histogram() -> Histogram {
    Histogram::new(JOB_BUCKETS.iter().copied())
}
fn http_histogram() -> Histogram {
    Histogram::new(HTTP_BUCKETS.iter().copied())
}

#[derive(Default)]
struct Windows {
    processing: HashMap<(JobKind, Outcome), RecentExtrema>,
    speed: RecentExtrema,
}

struct LogWindow {
    last: Instant,
    suppressed: u64,
}

struct Metrics {
    registry: Registry,
    started: Instant,
    http: Family<Labels, Counter>,
    http_duration: Histograms,
    in_flight: Gauge,
    mutations: Family<Labels, Counter>,
    auth: Family<Labels, Counter>,
    errors: Family<Labels, Counter>,
    submissions: Family<Labels, Counter>,
    rejections: Family<Labels, Counter>,
    admitted: Gauge,
    queued: Family<Labels, Gauge>,
    oldest: Family<Labels, FloatGauge>,
    running: Family<Labels, Gauge>,
    attempts: Family<Labels, Counter>,
    canceled_before_start: Family<Labels, Counter>,
    recovered: Counter,
    queue_wait: Histograms,
    processing: Histograms,
    end_to_end: Histograms,
    phase: Histograms,
    iterations: Counter,
    speed: Histogram,
    snapshot_timestamp: FloatGauge,
    db_connections: Gauge,
    db_idle: Gauge,
    processing_min: Family<Labels, FloatGauge>,
    processing_max: Family<Labels, FloatGauge>,
    processing_count: Family<Labels, Gauge>,
    speed_min: FloatGauge,
    speed_max: FloatGauge,
    speed_count: Gauge,
    windows: Mutex<Windows>,
    log_windows: Mutex<HashMap<(&'static str, &'static str), LogWindow>>,
}

/// Independent registry per application. No process-global recorder is installed.
#[derive(Clone)]
pub struct Telemetry(Arc<Metrics>);

#[derive(Clone, Copy, Debug)]
pub struct QueueSnapshot {
    pub kind: JobKind,
    pub queued: u64,
    pub oldest_age_seconds: f64,
}

/// Releases active work on normal return, cancellation, abort, or unwind.
pub struct RunningGuard(Gauge);
impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.dec();
    }
}

impl Telemetry {
    pub fn new(workers: usize) -> Self {
        let mut registry = Registry::default();
        macro_rules! metric {
            ($name:literal, $help:literal, $value:expr) => {{
                let metric = $value;
                registry.register(concat!("finplan_", $name), $help, metric.clone());
                metric
            }};
        }
        let http = metric!(
            "http_requests",
            "Completed HTTP responses",
            Family::<Labels, Counter>::default()
        );
        let http_duration = metric!(
            "http_request_duration_seconds",
            "Time from arrival to response headers in seconds",
            Histograms::new_with_constructor(http_histogram)
        );
        let in_flight = metric!(
            "http_requests_in_flight",
            "HTTP requests awaiting response headers",
            Gauge::default()
        );
        let mutations = metric!(
            "mutations",
            "Committed user-visible mutations",
            Family::<Labels, Counter>::default()
        );
        let auth = metric!(
            "auth_events",
            "Authentication outcomes",
            Family::<Labels, Counter>::default()
        );
        let errors = metric!(
            "server_errors",
            "Unexpected failures at their handling boundary",
            Family::<Labels, Counter>::default()
        );
        let submissions = metric!(
            "job_submissions",
            "Compute submission decisions",
            Family::<Labels, Counter>::default()
        );
        let rejections = metric!(
            "compute_rejections",
            "Compute admission rejections",
            Family::<Labels, Counter>::default()
        );
        let admitted = metric!(
            "compute_admitted",
            "Process-wide queued and running admission permits held",
            Gauge::default()
        );
        let limit: Gauge = metric!(
            "compute_limit",
            "Process-wide compute admission limit",
            Gauge::default()
        );
        limit.set(crate::billing::COMPUTE_LIMIT as i64);
        let queued = metric!(
            "jobs_queued",
            "Waiting jobs including persisted recovery backlog; sampled every five seconds",
            Family::<Labels, Gauge>::default()
        );
        let oldest = metric!(
            "job_oldest_queued_age_seconds",
            "Age of oldest waiting job in seconds; sampled every five seconds",
            Family::<Labels, FloatGauge>::default()
        );
        let running = metric!(
            "jobs_running",
            "Attempts occupying a worker including preparation and persistence",
            Family::<Labels, Gauge>::default()
        );
        let slots = metric!(
            "worker_slots",
            "Configured worker slots per independent pool",
            Family::<Labels, Gauge>::default()
        );
        for pool in ["run", "analysis"] {
            slots
                .get_or_create(&labels([("pool", pool)]))
                .set(workers as i64);
        }
        for kind in [
            JobKind::Run,
            JobKind::Sweep,
            JobKind::Sensitivity,
            JobKind::Solve,
        ] {
            queued.get_or_create(&job_labels(kind)).set(0);
            oldest.get_or_create(&job_labels(kind)).set(0.0);
            running.get_or_create(&job_labels(kind)).set(0);
        }
        let attempts = metric!(
            "job_attempts",
            "Finished execution attempts",
            Family::<Labels, Counter>::default()
        );
        let canceled_before_start = metric!(
            "jobs_canceled_before_start",
            "Queued cancellations resolved without execution",
            Family::<Labels, Counter>::default()
        );
        let recovered = metric!(
            "jobs_recovered",
            "Persisted runs successfully readmitted after restart",
            Counter::default()
        );
        let queue_wait = metric!(
            "job_queue_wait_seconds",
            "Time from submission to leaving the queue in seconds",
            Histograms::new_with_constructor(job_histogram)
        );
        let processing = metric!(
            "job_processing_duration_seconds",
            "Time from successful worker claim to attempt completion in seconds",
            Histograms::new_with_constructor(job_histogram)
        );
        let end_to_end = metric!(
            "job_end_to_end_duration_seconds",
            "Time from submission to attempt completion including recovery downtime in seconds",
            Histograms::new_with_constructor(job_histogram)
        );
        let phase = metric!(
            "job_phase_duration_seconds",
            "Time in each entered execution phase in seconds",
            Histograms::new_with_constructor(job_histogram)
        );
        let iterations = metric!(
            "run_iterations_completed",
            "Actual Monte Carlo samples in successfully persisted runs",
            Counter::default()
        );
        let speed = metric!(
            "run_iterations_per_second",
            "Per-successful-run sample throughput divided by engine wall time",
            Histogram::new(SPEED_BUCKETS.iter().copied())
        );
        let snapshot_timestamp = metric!(
            "queue_snapshot_timestamp_seconds",
            "Unix time of last successful queue snapshot",
            FloatGauge::default()
        );
        let db_connections = metric!(
            "db_pool_connections",
            "Current database pool connections",
            Gauge::default()
        );
        let db_idle = metric!(
            "db_pool_idle_connections",
            "Current idle database pool connections",
            Gauge::default()
        );
        let start_time = metric!(
            "process_start_time_seconds",
            "Unix process start time in seconds",
            FloatGauge::default()
        );
        start_time.set(unix_seconds());
        let build = metric!(
            "build_info",
            "Application and simulation model versions",
            Family::<Labels, Gauge>::default()
        );
        build
            .get_or_create(&labels([
                ("version", env!("CARGO_PKG_VERSION")),
                ("model_version", crate::runner::inputs::MODEL_VERSION),
            ]))
            .set(1);
        let processing_min = metric!(
            "job_processing_duration_window_min_seconds",
            "Minimum processing seconds in current and previous 299 monotonic seconds; NaN when empty",
            Family::<Labels, FloatGauge>::default()
        );
        let processing_max = metric!(
            "job_processing_duration_window_max_seconds",
            "Maximum processing seconds in current and previous 299 monotonic seconds; NaN when empty",
            Family::<Labels, FloatGauge>::default()
        );
        let processing_count = metric!(
            "job_processing_duration_window_observations",
            "Processing observations in current and previous 299 monotonic seconds",
            Family::<Labels, Gauge>::default()
        );
        let speed_min = metric!(
            "run_iterations_per_second_window_min",
            "Minimum run speed in current and previous 299 monotonic seconds; NaN when empty",
            FloatGauge::default()
        );
        let speed_max = metric!(
            "run_iterations_per_second_window_max",
            "Maximum run speed in current and previous 299 monotonic seconds; NaN when empty",
            FloatGauge::default()
        );
        let speed_count = metric!(
            "run_iterations_per_second_window_observations",
            "Run speed observations in current and previous 299 monotonic seconds",
            Gauge::default()
        );
        let mut windows = Windows::default();
        // Fixed job populations need a zero baseline so the first sparse
        // completion can be included in a Prometheus rate/quantile window.
        for kind in [
            JobKind::Run,
            JobKind::Sweep,
            JobKind::Sensitivity,
            JobKind::Solve,
        ] {
            drop(canceled_before_start.get_or_create(&job_labels(kind)));
            for outcome in [
                Outcome::Succeeded,
                Outcome::Failed,
                Outcome::Canceled,
                Outcome::Interrupted,
            ] {
                let labels = outcome_labels(kind, outcome);
                drop(attempts.get_or_create(&labels));
                drop(processing.get_or_create(&labels));
                drop(end_to_end.get_or_create(&labels));
                windows
                    .processing
                    .insert((kind, outcome), RecentExtrema::default());
            }
            for exit in [QueueExit::Started, QueueExit::Canceled, QueueExit::Deleted] {
                drop(
                    queue_wait
                        .get_or_create(&labels([("kind", kind.as_str()), ("exit", exit.as_str())])),
                );
            }
            for stage in [
                Phase::Prepare,
                Phase::BlockingWait,
                Phase::Engine,
                Phase::Persist,
            ] {
                drop(phase.get_or_create(&labels([
                    ("kind", kind.as_str()),
                    ("phase", stage.as_str()),
                ])));
            }
            for result in [
                SubmissionResult::Accepted,
                SubmissionResult::Invalid,
                SubmissionResult::CapacityRejected,
                SubmissionResult::InternalError,
            ] {
                drop(submissions.get_or_create(&labels([
                    ("kind", kind.as_str()),
                    ("result", result.as_str()),
                ])));
            }
        }
        Self(Arc::new(Metrics {
            registry,
            started: Instant::now(),
            http,
            http_duration,
            in_flight,
            mutations,
            auth,
            errors,
            submissions,
            rejections,
            admitted,
            queued,
            oldest,
            running,
            attempts,
            canceled_before_start,
            recovered,
            queue_wait,
            processing,
            end_to_end,
            phase,
            iterations,
            speed,
            snapshot_timestamp,
            db_connections,
            db_idle,
            processing_min,
            processing_max,
            processing_count,
            speed_min,
            speed_max,
            speed_count,
            windows: Mutex::new(windows),
            log_windows: Mutex::new(HashMap::new()),
        }))
    }

    /// Encoding reads cached gauges only; it never queries the database or job registry.
    pub fn encode(&self) -> Result<String, std::fmt::Error> {
        // Serialize extrema refresh plus encoding, so concurrent scrapes see one window.
        let windows = self.0.windows.lock().unwrap_or_else(|e| e.into_inner());
        let second = self.0.started.elapsed().as_secs();
        for (&(kind, outcome), window) in &windows.processing {
            let (min, max, count) = window.snapshot(second);
            let labels = outcome_labels(kind, outcome);
            self.0.processing_min.get_or_create(&labels).set(min);
            self.0.processing_max.get_or_create(&labels).set(max);
            self.0
                .processing_count
                .get_or_create(&labels)
                .set(count as i64);
        }
        let (min, max, count) = windows.speed.snapshot(second);
        self.0.speed_min.set(min);
        self.0.speed_max.set(max);
        self.0.speed_count.set(count as i64);
        let mut output = String::new();
        encode(&mut output, &self.0.registry)?;
        Ok(output)
    }

    pub(super) fn request_started(&self) -> RunningGuard {
        self.0.in_flight.inc();
        RunningGuard(self.0.in_flight.clone())
    }

    pub(super) fn request_completed(&self, method: &str, route: &str, status: u16, seconds: f64) {
        self.0
            .http
            .get_or_create(&labels([
                ("method", method),
                ("route", route),
                ("status", &status.to_string()),
            ]))
            .inc();
        self.0
            .http_duration
            .get_or_create(&labels([("method", method), ("route", route)]))
            .observe(seconds);
    }

    pub(super) fn mutation_count(&self, resource: Resource, operation: Operation) {
        self.0
            .mutations
            .get_or_create(&labels([
                ("resource", resource.as_str()),
                ("operation", operation.as_str()),
            ]))
            .inc();
    }
    pub(super) fn auth_count(&self, action: AuthAction, outcome: AuthOutcome) {
        self.0
            .auth
            .get_or_create(&labels([
                ("action", action.as_str()),
                ("outcome", outcome.as_str()),
            ]))
            .inc();
    }
    pub fn count_error(&self, component: Component, class: ErrorClass) {
        self.0
            .errors
            .get_or_create(&labels([
                ("component", component.as_str()),
                ("class", class.as_str()),
            ]))
            .inc();
    }
    pub fn submission(&self, kind: JobKind, result: SubmissionResult) {
        self.0
            .submissions
            .get_or_create(&labels([
                ("kind", kind.as_str()),
                ("result", result.as_str()),
            ]))
            .inc();
    }
    pub fn rejection(&self, reason: RejectionReason, origin: Origin) {
        self.0
            .rejections
            .get_or_create(&labels([
                ("reason", reason.as_str()),
                ("origin", origin.as_str()),
            ]))
            .inc();
        if let Some(suppressed) = self.permit_log(("rejection", reason.as_str())) {
            tracing::warn!(
                event = "compute.rejected",
                reason = reason.as_str(),
                origin = origin.as_str(),
                suppressed
            );
        }
    }
    pub fn job_started(&self, kind: JobKind) -> RunningGuard {
        let gauge = self.0.running.get_or_create(&job_labels(kind)).clone();
        gauge.inc();
        RunningGuard(gauge)
    }
    pub fn queue_wait(&self, kind: JobKind, exit: QueueExit, seconds: f64) {
        self.0
            .queue_wait
            .get_or_create(&labels([("kind", kind.as_str()), ("exit", exit.as_str())]))
            .observe(seconds.max(0.0));
    }
    pub fn attempt_completed(&self, kind: JobKind, outcome: Outcome, processing: f64, e2e: f64) {
        let labels = outcome_labels(kind, outcome);
        self.0.attempts.get_or_create(&labels).inc();
        self.0
            .processing
            .get_or_create(&labels)
            .observe(processing.max(0.0));
        self.0
            .end_to_end
            .get_or_create(&labels)
            .observe(e2e.max(0.0));
        self.0
            .windows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .processing
            .entry((kind, outcome))
            .or_default()
            .observe(self.0.started.elapsed().as_secs(), processing);
    }
    pub fn phase(&self, kind: JobKind, phase: Phase, seconds: f64) {
        self.0
            .phase
            .get_or_create(&labels([
                ("kind", kind.as_str()),
                ("phase", phase.as_str()),
            ]))
            .observe(seconds.max(0.0));
    }
    pub fn iterations_completed(&self, iterations: u64, engine_seconds: f64) {
        self.0.iterations.inc_by(iterations);
        if engine_seconds > 0.0 && engine_seconds.is_finite() {
            let speed = iterations as f64 / engine_seconds;
            if speed.is_finite() {
                self.0.speed.observe(speed);
                self.0
                    .windows
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .speed
                    .observe(self.0.started.elapsed().as_secs(), speed);
            }
        }
    }
    pub fn recovered(&self) {
        self.0.recovered.inc();
    }
    pub fn canceled_before_start(&self, kind: JobKind) {
        self.0
            .canceled_before_start
            .get_or_create(&job_labels(kind))
            .inc();
    }

    pub(super) fn set_snapshot(
        &self,
        snapshots: &[QueueSnapshot],
        connections: u32,
        idle: usize,
        admitted: usize,
    ) {
        for snapshot in snapshots {
            self.0
                .queued
                .get_or_create(&job_labels(snapshot.kind))
                .set(snapshot.queued as i64);
            self.0
                .oldest
                .get_or_create(&job_labels(snapshot.kind))
                .set(if snapshot.queued == 0 {
                    0.0
                } else {
                    snapshot.oldest_age_seconds.max(0.0)
                });
        }
        self.0.admitted.set(admitted as i64);
        self.0.db_connections.set(connections as i64);
        self.0.db_idle.set(idle as i64);
        self.0.snapshot_timestamp.set(unix_seconds());
    }

    /// Count every failure; emit the first and a summary at most once a minute.
    pub(super) fn permit_log(&self, key: (&'static str, &'static str)) -> Option<u64> {
        let mut limits = self.0.log_windows.lock().unwrap_or_else(|e| e.into_inner());
        match limits.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(LogWindow {
                    last: Instant::now(),
                    suppressed: 0,
                });
                Some(0)
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let window = entry.get_mut();
                if window.last.elapsed().as_secs() >= 60 {
                    let count = window.suppressed;
                    window.last = Instant::now();
                    window.suppressed = 0;
                    Some(count)
                } else {
                    window.suppressed += 1;
                    None
                }
            }
        }
    }
    pub(super) fn clear_log_limit(&self, key: (&'static str, &'static str)) -> Option<u64> {
        self.0
            .log_windows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&key)
            .map(|w| w.suppressed)
    }
}

pub(super) fn unix_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registries_are_isolated_and_histograms_count_actual_samples() {
        let telemetry = Telemetry::new(2);
        let isolated = Telemetry::new(1);
        let guard = telemetry.job_started(JobKind::Run);
        assert!(
            telemetry
                .encode()
                .unwrap()
                .contains("finplan_jobs_running{kind=\"run\"} 1")
        );
        drop(guard);
        telemetry.attempt_completed(JobKind::Run, Outcome::Succeeded, 5.0, 8.0);
        telemetry.iterations_completed(100, 2.0);
        let output = telemetry.encode().unwrap();
        assert!(output.contains("finplan_jobs_running{kind=\"run\"} 0"));
        assert!(output.contains(
            "finplan_job_processing_duration_seconds_sum{kind=\"run\",outcome=\"succeeded\"} 5.0"
        ));
        assert!(output.contains("finplan_job_processing_duration_window_min_seconds{kind=\"run\",outcome=\"succeeded\"} 5.0"));
        assert!(output.contains("finplan_run_iterations_completed_total 100"));
        assert!(output.contains("finplan_run_iterations_per_second_window_min 50.0"));
        assert!(output.ends_with("# EOF\n"));
        assert!(
            isolated
                .encode()
                .unwrap()
                .contains("finplan_job_attempts_total{kind=\"run\",outcome=\"succeeded\"} 0")
        );
        assert!(
            isolated
                .encode()
                .unwrap()
                .contains("finplan_run_iterations_completed_total 0")
        );
    }

    #[test]
    fn overflowing_samples_are_retained_in_infinity_bucket() {
        let telemetry = Telemetry::new(1);
        telemetry.attempt_completed(JobKind::Solve, Outcome::Failed, 9000.0, 9000.0);
        let output = telemetry.encode().unwrap();
        assert!(output.contains("le=\"+Inf\",kind=\"solve\",outcome=\"failed\"} 1"));
        assert!(output.contains("le=\"7200.0\",kind=\"solve\",outcome=\"failed\"} 0"));
    }
}
