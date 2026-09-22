# Server logging and metrics implementation plan

Status: packages A, B, and C are implemented by delegated agents, with shared
integration tests and setup documentation. Package D's dashboards, alert rules,
and operations deployment remain deferred.

## Implementation notes

The server now provides structured activity and request logs, correlated worker
logs, an owned Prometheus registry, a separate optional metrics listener, queue
sampling, and job duration/throughput metrics with recent extrema. No API
payload changes or database migrations were needed.

Worker instrumentation includes the correctness fixes required for reliable
counts: channel reservation before commit, cancellation resolution, guarded
terminal persistence, and attempt identity checks when SQLite reuses a deleted
run ID. Shutdown has a bounded HTTP drain and stops observability tasks.

Validation covers committed mutations and rollback, sensitive-data exclusion,
request-to-worker correlation, admission/enqueue failures, cancellation,
duplicate delivery, cascade deletion, ID reuse, engine panic, persistence
failure, recovery backlog, sampler freshness, and private exposition. Workspace
formatting, strict Clippy, and tests pass. A real server smoke test also verifies
JSON output, private metrics, bind failure, and shutdown with an incomplete
HTTP request. Existing ts-rs attribute-parser notices remain; generated API
bindings have no changes after regeneration.

Production workload overhead and histogram bucket calibration remain to be
measured. Prometheus rule validation and Grafana deployment belong to the
deferred operations package; no dashboards, alerts, or external services were
deployed by this implementation.

## Outcome and scope

Make user actions, server failures, and compute bottlenecks observable in
`finplan_server`. Cover both financial accounts and user accounts. Provide
structured operational logs, a Prometheus-compatible scrape endpoint, and
dashboard queries for queue depth, wait time, throughput, and processing-time
average/minimum/maximum/p99.

The Next.js app currently proxies `/api/*` to Rust through `web/next.config.ts`.
Rust owns authentication, mutations, SQLite, and compute; it is the first scope
of this work. Browser analytics and Next.js rendering/proxy telemetry are later
extensions. Rust HTTP timings cannot measure time spent before reaching Rust.
Do not add API payload fields or database migrations solely for telemetry.

Logs go to the process output for collection by the deployment environment.
These are operational activity logs, with a possible gap if the process dies
after a database commit and before logging. A durable transactional audit trail
would require a separate design. Prometheus stores metric history outside the
application; application counters reset on process restart.

## Findings that shape the design

| Existing code | Consequence |
|---|---|
| `src/main.rs` initializes `tracing_subscriber`; `src/lib.rs` installs the default HTTP `TraceLayer` | Extend existing tracing. Default request spans are DEBUG and include raw URIs; replace their fields and set deliberate levels. |
| `src/error.rs` logs database/internal errors and returns sanitized 500 bodies | Keep client responses sanitized and assign one owner for error classification/counting. |
| `src/api/accounts.rs` commits account changes before touching/reloading the scenario | Emit a committed mutation event immediately after the actual commit, even if later response work fails. |
| `src/runner/mod.rs` has a bounded channel of 16, then waits for a worker permit after receiving | Channel length understates waiting work. Include dispatcher-held work and persisted recovery backlog. |
| `src/billing.rs` caps queued plus running compute at 16 globally and 2 per user | Distinguish admission saturation from worker saturation; expose aggregate counts only. |
| `src/analysis/jobs.rs` has its own semaphore and in-memory registry | Instrument analyses as well as runs. Each pool has `sim_workers` slots; there is no shared execution semaphore. |
| Run creation commits before `enqueue_admitted` can fail | Make admission/enqueue outcomes explicit and prevent committed, undispatched submissions caused by an ordinary enqueue failure. |
| A canceled run returns `RunError::Canceled`, then the outer worker logs it as failed | Classify cancellation separately before adding failure counters. |
| Recovery and progress reporting contain ignored errors | Surface recovery, progress, terminal-persistence, session maintenance, and cache failures. |
| Integration tests build multiple applications in one process | Use an explicit per-application metrics registry, not a global recorder. |

Paths in this document are relative to `crates/finplan_server` unless they start
with `web/`, `spec/`, `scripts/`, `ops/`, `.github/`, or `Cargo.lock`.

## Shared implementation contract

Create `src/observability/` with small modules for request context, events,
metrics, job timing, and exposition. Store a cloneable telemetry handle in
`AppState`; pass it explicitly to worker services. Use the existing `tracing`
stack with JSON support and `prometheus-client` with an owned registry. Pin a
compatible dependency in the lockfile during implementation. Do not install a
second global subscriber from library code or tests.

The foundation implementation must publish the following interfaces before
parallel work begins:

- `RequestContext`: server-generated UUID request ID, normalized method and
  matched route, optional authenticated user ID. Return `X-Request-ID` on all
  responses, including middleware and extractor rejections. Generate the ID
  locally rather than trusting arbitrary inbound header values.
- `JobContext`: request ID when available, user/scenario/run or analysis ID,
  bounded job kind, and origin (`request` or `recovery`). Carry context explicitly
  across `tokio::spawn` and into `spawn_blocking`; use tracing instrumentation
  correctly across awaits. Recovery has a new attempt context and the existing
  run ID; it must not pretend to retain the original request span.
- Typed bounded enums for resource, operation, job kind, outcome, rejection
  reason, and error class. Call sites do not invent metric label strings.
- Event helpers that accept allowlisted metadata; job timing records with
  explicit start/finish points; guards that release active gauges on every exit.
- A metrics router built from the same telemetry handle, plus a sampler that
  can be started and stopped independently of scraping.

Suggested configuration:

| Setting | Proposed behavior |
|---|---|
| `RUST_LOG` | Retain filtering; default application INFO and dependencies WARN. |
| `FINPLAN_LOG_FORMAT=auto\|json\|text` | `auto`: JSON in hosted mode, readable text locally. Parse config before subscriber initialization. |
| `FINPLAN_METRICS_BIND` | Optional separate listen address; unset disables the listener. Document `127.0.0.1:9090` for local/private scraping. |

Expose `GET /metrics` only on the dedicated listener. Do not put it under the
Next.js `/api` proxy or require a browser session cookie. A non-loopback bind
must be explicitly configured and protected by the deployment network or an
authenticated reverse proxy. Use the encoder's matching OpenMetrics content
type and valid HELP/TYPE/EOF output. Fail startup clearly if an explicitly
configured listener cannot bind. Supervise listener/sampler failures and stop
them with application shutdown. `build()` and ordinary router tests must not
open network listeners implicitly.

## Logging coverage and semantics

All logs carry timestamp, level, stable `event`, and available request/job
context. Record elapsed durations in seconds and retain IDs as structured
fields. Log only internal IDs, enum values, counts, and allowlisted field names:
no financial balances, account names/descriptions, birth dates, emails,
passwords/hashes, cookies, authorization headers, reset tokens, request/response
bodies, SQL parameters, raw URLs/query strings, or exported plan content.
Do not use `#[instrument]` on whole request/config/user structs without skipping
their values. Error handling must also respect this rule, including panic text
and third-party database/engine errors.

| Event family | Required coverage and level |
|---|---|
| `account.created/updated/deleted/reordered`, `position.*` | INFO after successful persistence; include resource IDs and submitted field names, never values. |
| `scenario.*`, `asset.*`, `event.*`, `return_profile.*`, `inflation_profile.*`, `tax_config.*` | INFO for create/update/delete/reorder/duplicate operations as supported by each route. |
| `onboarding.completed`, `archive.imported/exported`, `run.deleted` | INFO with IDs and affected counts. Idempotent replay is identified and does not claim new mutations. |
| `user.registered/updated/deleted`, `auth.login/logout`, `auth.password_changed`, `auth.reset_requested/completed`, `auth.email_verified`, `auth.session_revoked` | INFO for supported successful actions. Failed login uses a generic outcome and no attempted email or account-existence detail. |
| `request.rejected`, `auth.throttled`, `compute.rejected` | Bounded reason codes. Ordinary validation/not-found/polling responses stay DEBUG; security throttles and operational saturation are WARN with aggregation/rate limiting. |
| `run.submitted/started/succeeded/cancel_requested/canceled/recovered`, equivalent `analysis.*` | INFO; include requested/actual iterations when meaningful and queue/processing durations. Submissions include both manual and automatic UI runs; the existing API cannot distinguish those triggers. |
| `run.failed`, `analysis.failed`, `request.failed` | ERROR with safe error class, phase, IDs, and sanitized diagnostic details. Expected cancellation is not a failure. |
| `billing.reconciled` | INFO when the existing trusted provider reconciliation interface changes state; DEBUG for replay/stale input. No payment payloads. |
| `server.starting/ready/shutdown`, `recovery.started/completed/failed`, `maintenance.failed`, `analysis.cache_failed` | INFO lifecycle, WARN recoverable degradation, ERROR work loss or unavailable service. Include safe configuration such as worker counts/version, not secrets or full config. |

Emit a domain event once the relevant mutation commits, not merely because a
handler was entered or eventually returned 2xx. A committed change followed by
a reload failure should produce both the committed event and a correlated
request failure. Rollbacks and missing-row deletes must not emit success.
For composite operations, log one parent event with affected counts instead of
thousands of child events. Updates report submitted field names rather than
claiming a before/after diff that the code has not computed.

Record one HTTP completion event: INFO for mutations and slow requests, DEBUG
for routine successful reads/polls/health checks. A slow threshold of one second
is an initial default. Count every request regardless of log level. Keep the
original error cause at one ERROR site; the access event may summarize the 500
without duplicating the error stack. Middleware and extractor failures need
request IDs and HTTP metrics too. Configure the layer order and fallback
handling explicitly, including 404, 405, CORS/origin, and entitlement responses.

Record authenticated identity after authentication succeeds, including in
middleware paths where possible; do not repeat authentication just for logging.
Background tasks must log previously ignored failures with bounded diagnostic
codes. Repeated progress/session/cache failures should increment every metric
but log the first failure, periodic summaries, and recovery to avoid flooding.

## Metric catalog

Names below are exported names, including automatic counter suffixes. Use
seconds for durations. Never label by user/account/scenario/run/request ID,
email, error message, input hash, or literal URL. HTTP routes are Axum matched
templates with bounded `unmatched`/`unknown` fallbacks; methods have an `other`
fallback. Version is only a build-info label. Avoid eager Cartesian products of
every possible label combination.

`kind` is one of `run`, `sweep`, `sensitivity`, `solve`. `outcome` is one of
`succeeded`, `failed`, `canceled`, `interrupted`. An interrupted attempt leaves
recoverable work; it does not imply a persisted terminal run status.

| Exported metric | Type / labels | Definition |
|---|---|---|
| `finplan_http_requests_total` | Counter; method, route, status | Every completed HTTP response. Metrics scrapes are excluded. |
| `finplan_http_request_duration_seconds` | Histogram; method, route | Arrival at Rust middleware to response headers; not body-download or browser latency. |
| `finplan_http_requests_in_flight` | Gauge | Requests awaiting response headers; drop-safe. |
| `finplan_mutations_total` | Counter; resource, operation | Committed user-visible mutations; same success boundary as domain logs. |
| `finplan_auth_events_total` | Counter; action, outcome | Bounded authentication action/result pairs, with no identity labels. |
| `finplan_server_errors_total` | Counter; component, class | Unexpected failures, recorded once at their handling boundary; includes non-HTTP background failures. |
| `finplan_job_submissions_total` | Counter; kind, result | Submission decisions reaching the compute submission service: accepted, invalid, capacity_rejected, or internal_error. Pre-handler auth/JSON failures remain HTTP metrics. |
| `finplan_compute_rejections_total` | Counter; reason, origin=request/recovery | Actual admission failures: global_limit, user_limit, queue_full, queue_closed. Recovery retries are counted separately from user submission decisions. |
| `finplan_compute_admitted` | Gauge | Existing process-wide compute permits held, including queued/canceled work awaiting cleanup. |
| `finplan_compute_limit` | Gauge | Process-wide configured admission limit, currently 16. |
| `finplan_jobs_queued` | Gauge; kind | Current waiting work, including recovery backlog and dispatcher-held runs. See sampling rules below. |
| `finplan_job_oldest_queued_age_seconds` | Gauge; kind | Age of oldest waiting job; zero when empty. Detects a queue that never starts work. |
| `finplan_jobs_running` | Gauge; kind | Work actively occupying a worker in this process, including preparation/persistence. |
| `finplan_worker_slots` | Gauge; pool=run/analysis | Configured capacity of each independent pool; do not repeat analysis pool capacity per analysis kind. |
| `finplan_job_attempts_total` | Counter; kind, outcome | Finished execution attempts, counted once; separate from a canceled-before-start event. |
| `finplan_jobs_canceled_before_start_total` | Counter; kind | Cancellation resolved while queued; no processing-duration sample. |
| `finplan_jobs_recovered_total` | Counter | Persisted runs successfully re-admitted after restart. |
| `finplan_job_queue_wait_seconds` | Histogram; kind, exit=started/canceled/deleted | Submission commit/registry insertion to leaving the queue. Measure a recovered run from its original persisted creation time. |
| `finplan_job_processing_duration_seconds` | Histogram; kind, outcome | Successful worker claim to attempt completion, including preparation, blocking-executor wait, engine, and persistence. |
| `finplan_job_end_to_end_duration_seconds` | Histogram; kind, outcome | Submission to completion for completed attempts; recovered runs include downtime. |
| `finplan_job_phase_duration_seconds` | Histogram; kind, phase | prepare, blocking_wait, engine, persist; observe phases actually entered. |
| `finplan_run_iterations_completed_total` | Counter | Actual Monte Carlo samples from successfully persisted runs, added at completion, not requested iteration ceilings. |
| `finplan_run_iterations_per_second` | Histogram | Per-successful-run actual iterations divided by engine wall duration. Includes engine result reconstruction time; not a CPU benchmark. |
| `finplan_queue_snapshot_timestamp_seconds` | Gauge | Unix time of last successful queue snapshot, to make sampler staleness visible. |
| `finplan_db_pool_connections`, `finplan_db_pool_idle_connections` | Gauges | Cheap pool snapshots; do not instrument every SQL query initially. |
| `finplan_build_info`, `finplan_process_start_time_seconds` | Gauges | Version/model version information and process lifetime. Host CPU/RSS monitoring can use the deployment's host/container exporter. |

Sample queue counts and oldest age every five seconds, with an initial sample
before reporting ready. Use SQLite status rows for runs, including recovery
backlog, and the analysis registry for analyses. Cache the result; scraping must
not query SQLite. Leave the last good sample in place after a sampling failure,
increment/log the failure, and expose its age through the timestamp. Document
the five-second lag. Reading the existing process-global compute count is an
explicit exception to per-application metric isolation; do not create a second
permit accounting system for metrics.

Initial finite histogram boundaries (seconds): HTTP
`0.005, .01, .025, .05, .1, .25, .5, 1, 2.5, 5, 10, 30, 60`; job timings
`.01, .05, .1, .25, .5, 1, 2, 5, 10, 30, 60, 120, 300, 600, 1800, 3600, 7200`.
The encoder supplies the infinity bucket. Speed boundaries (iterations/second):
`1, 10, 50, 100, 250, 500, 1000, 2500, 5000, 10000, 25000, 50000, 100000`.
Validate boundaries against a representative small and large workload before
calling p99 useful; chart overflow and sample count alongside quantiles.

## Average, minimum, maximum, p99, and throughput

Use classic histograms for configurable-window averages and estimated
percentiles. Aggregate buckets across instances before calculating quantiles;
do not average instance p99 values. These semantics follow the
[Prometheus histogram guidance](https://prometheus.io/docs/practices/histograms/).

Example average successful processing time over five minutes:

```promql
sum by (kind) (rate(finplan_job_processing_duration_seconds_sum{outcome="succeeded"}[5m]))
/
sum by (kind) (rate(finplan_job_processing_duration_seconds_count{outcome="succeeded"}[5m]))
```

Example p99 for the same population:

```promql
histogram_quantile(0.99,
  sum by (kind, le) (rate(finplan_job_processing_duration_seconds_bucket{outcome="succeeded"}[5m]))
)
```

Completed runs/second comes from the rate of successful `job_attempts_total`.
Effective samples/second comes from the rate of `run_iterations_completed_total`;
it arrives in completion-sized bursts and is not a live engine progress counter.
Average per-run speed is the rate of `run_iterations_per_second_sum` divided by
the rate of its `_count`; p99 speed uses that histogram's buckets. High duration
is bad; high speed is good, so also show p01 speed for slow-run diagnosis.

Histograms do not preserve exact observed extrema. Implement a bounded recent
extrema helper for processing duration (by kind/outcome) and successful run
speed. Use 300 one-second buckets holding observation count, minimum, and
maximum. At collection time include the current second and previous 299, based
on a monotonic clock; expire old slots even when no jobs finish. This is a
five-minute window at one-second resolution, with less than one second of
boundary uncertainty, not an exact arbitrary PromQL range.

Export `finplan_job_processing_duration_window_min_seconds`,
`finplan_job_processing_duration_window_max_seconds`, and
`finplan_job_processing_duration_window_observations` with kind/outcome labels.
Export analogous `finplan_run_iterations_per_second_window_min`, `_window_max`,
and `_window_observations`. Emit NaN extrema and zero observations for an empty
window; dashboards show no data. Across instances use min/min and max/max for
these gauges. Do not infer extrema from a last-duration gauge or use
`histogram_quantile(0/1)` as an exact minimum/maximum. Each completion log also
retains its duration/speed for individual investigation.

## Worker correctness required for trustworthy telemetry

Use monotonic clocks for within-process intervals, including a timestamp inside
the blocking closure to distinguish executor wait from engine time. Preserve
the persisted creation time for recovered queue/end-to-end age, document its
existing second resolution, and clamp/log impossible negative wall-clock ages.
Do not change persisted timestamps or analysis `elapsed_ms` API semantics just
to add telemetry. Keep separate internal submitted/claimed/engine/finished
timestamps.

The worker package must handle these boundaries explicitly:

1. Reserve channel capacity before committing a new run, then send through the
   reserved permit after commit. Validation, capacity, and commit failures must
   release resources and cannot claim a successful submission. Preserve crash
   recovery for a process exit between commit and dispatch.
2. Count start only after the guarded database claim succeeds. Duplicate queue
   deliveries, already canceled/deleted rows, and failed claims are not successful
   attempts. Keep run IDs for correlation, never metric labels.
3. Resolve queued/running cancellation races using the actual transition result.
   Cancellation is logged/counted once at resolution; a request to cancel is a
   separate event. A queued item may still hold admission until it is drained.
4. Emit success only after results persist. Classify preparation, engine panic,
   engine error, and persistence failure separately. If marking failure itself
   fails, report the storage failure without claiming the database is terminal.
5. Ensure active gauges, progress reporter tasks, and timing guards clean up on
   every normal/error/cancel/unwind exit. No promise of a final event on SIGKILL;
   restart recovery and Prometheus process resets explain those gaps.
6. Account for deleting queued runs and scenario/user cascading deletion while
   jobs wait or execute. Do not leave gauges stuck or count missing rows as
   successful work. Follow existing deletion behavior; avoid a scheduler rewrite.
7. Log recovery scan/admission failures that currently end silently. Count a
   recovered item once when admitted; retries do not create new user submissions.
8. For analyses, preserve queued time separately from the `started` field that
   currently resets at `mark_running`. Make terminal recording idempotent and
   distinguish a successful sweep with a failed optional cache write.
9. Avoid per-iteration metrics or a tracing span per simulated event. Core progress
   counters can reset between analysis phases; do not sum sampled values as if
   they were monotonic. Initial normalized speed metrics cover regular runs;
   analyses get queue/phase/processing/throughput-by-job metrics.

## Delegation packages

Packages A, B, and C were authorized for implementation after this plan was
written. Every package starts by reading this plan and repository `AGENTS.md`.
Use an integration owner to freeze interfaces and manage shared files.

| Package | Ownership and deliverables | Dependencies |
|---|---|---|
| A — foundation | `src/observability/*`, `src/main.rs`, `src/lib.rs`, `src/state.rs`, `src/config.rs`, `src/error.rs`, server `Cargo.toml`, `Cargo.lock`. Implement logging format, safe HTTP context/completion, metrics registry/catalog, private endpoint, extrema helper, sampler interfaces, and lifecycle. Update config literals in existing test fixtures. Publish helper signatures and focused tests. | First. |
| B — activity events | `src/api/accounts.rs`, `assets.rs`, `events.rs`, `scenarios.rs`, `profiles.rs`, `taxes.rs`, `archives.rs`, `onboarding.rs`; `src/auth/*`; related domain helpers only when necessary. Instrument committed mutations, authentication/security events and session maintenance, with privacy tests. | After A; parallel with C. |
| C — compute observability | `src/runner/*`, `src/analysis/*`, `src/api/runs.rs`, `src/api/analysis.rs`, `src/billing.rs`, and related worker/history tests. Instrument lifecycle, queue snapshot sources, admission, stages, speeds, recovery, billing transitions, and the narrow correctness fixes listed above. | After A; parallel with B. |
| D — integration and operations | Dedicated `tests/observability.rs` and helper fixtures, server README, `spec/09_operations_runbook.md`, new `ops/observability/` scrape/rule/dashboard examples, and CI wiring if needed. Review implementation, run cross-cutting failure/race tests, validate queries and exposition, and provide a reusable Grafana dashboard JSON. | Draft tests/examples after A; final checks after B/C. |

A owns shared instrumentation interfaces. B/C request changes through the
integration owner instead of concurrently editing foundation files. Where A
needs worker constructor changes, agree minimal wiring with C before parallel
work begins. Prefer dedicated test modules over simultaneous edits to the large
`tests/api.rs`. Each package supplies a completion note with files changed,
checks run, unresolved issues, and any departure from the metric/event contract.

The operations dashboard should show HTTP request/error rate and latency,
admission utilization/rejections, queued/running work per kind, oldest queued
age, queue wait, average/min/max/p99 duration, completed jobs/minute, effective
run sample throughput, per-run speed, phase breakdown, recovery failures, and
queue sampler freshness. Provide configurable alert examples for scrape failure,
stale sampler, sustained oldest-queue age, repeated job/persistence/recovery
failures, and HTTP 5xx spikes. Use minimum-volume gates for ratio/percentile
alerts; thresholds are initial examples to calibrate against workloads.

## Acceptance and verification

- A financial-account create/update/delete produces the right committed event
  with IDs and request correlation. Failed authorization, validation, rollback,
  and missing-row deletes cannot emit mutation success. Verify a post-commit
  response failure still leaves the mutation event.
- Seed distinctive synthetic secrets and financial values in requests, query
  strings, error paths, and auth flows; capture JSON logs and assert they never
  appear. Verify request ID propagation through workers and blocking closures.
  Use scoped test subscribers and explicit async propagation, not global init.
- Exercise success, queued/running cancellation, duplicate delivery, queued
  deletion, cascade deletion, worker panic, failed persistence, enqueue/commit
  failure, and restart recovery. Use deterministic barriers/fake workers where
  needed so queue timing tests do not depend on machine speed.
- Assert no negative/stuck active gauges, no double terminal samples, and exact
  accounting at completed boundaries. Sampler tests cover persisted backlog
  larger than channel capacity, errors, freshness, and zero queue age when idle.
- Use an injected clock for known-duration/speed histogram observations and
  extrema expiration/bucket rollover. Cover empty windows, overflow buckets,
  counters resetting, sparse traffic, and failed/canceled populations.
- Scrape real exposition: verify content type, metric names, units, labels,
  HELP/TYPE output, and parseability. Different concrete IDs must share one
  route label; unknown methods/routes cannot create unlimited series. Independent
  app registries must not share HTTP/job observations; serialize tests for the
  pre-existing process-global admission gate where necessary.
- Verify metrics are absent from the public API router, scrapes need no user
  cookie, disabled mode opens no port, configured bind failure is visible, and
  shutdown stops observability tasks. Scraping cannot require the database.
- Compare a representative seeded small/large workload with telemetry enabled
  and disabled. Report measured overhead and histogram overflow; investigate a
  repeatable processing regression above 5% before accepting it. Do not make a
  noisy microbenchmark a mandatory timing assertion in CI.
- Validate example rules with `promtool check rules` and `promtool test rules`,
  including absent series and idle periods. Import/smoke-check the dashboard
  against a disposable local instance and synthetic data.

Final integration checks: `cargo fmt`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace`. Check `git diff --exit-code -- web/lib/api/generated`;
if API structs change despite the intended scope, regenerate using
`./scripts/gen-bindings.sh` and validate affected frontend types. Stage only
implementation-owned files and suggest a commit message. Do not commit or deploy
automatically as part of this plan.

Suggested eventual implementation commit message:
`feat(server): add structured activity logs and Prometheus metrics`.

The existing untracked `finplan.db.bak` is outside this work.

## Reference choices

The [Prometheus naming guidance](https://prometheus.io/docs/practices/naming/)
supports base units and bounded labels. The
[prometheus-client documentation](https://docs.rs/prometheus-client/latest/prometheus_client/)
provides an owned registry and OpenMetrics text encoding. These are library and
instrumentation choices; deploying a Prometheus/Grafana service or choosing a
central log-storage provider is a separate operational step.
