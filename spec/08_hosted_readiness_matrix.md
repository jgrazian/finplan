# Hosted paid-pilot readiness matrix

Inventory date: September 13, 2026. Scope: workstream G in
[the agent handoff](07_webui_paid_launch_agent_handoff.md), following roadmap H/I
in [the product roadmap](06_web_product_roadmap.md). Mobile work is deferred.

This is a source inspection of the checkout at the start of implementation.
Other agents are changing metrics and provenance concurrently; their final tests
must supersede the baseline findings below. No production configuration, live
account, payment provider, backup, or external service was inspected or modified.
No runtime behavior is certified by this inventory. Existing test names below
are evidence of coverage to rerun, not claims that those tests were executed here.

Status vocabulary:

- **Verified (source):** a specific implementation exists; deployment and runtime
  acceptance can still be untested.
- **Gap:** the inspected implementation does not meet the named gate, or the
  capability was not found in the inspected routing/configuration and files.
- **Blocked:** verification needs an operator, business decision, or service
  access. Only dependent work is blocked; local implementation/tests can proceed.

Owners below are roles to assign, not people who have accepted responsibility.

## Release gates

| Gate | Evidence and baseline status | Owner | Bounded remediation and acceptance evidence required |
| --- | --- | --- | --- |
| Authentication and sessions | **Verified (source), runtime untested.** `crates/finplan_server/src/auth/mod.rs` uses salted Argon2; `auth/session.rs` generates random 32-byte tokens, stores their SHA-256, checks expiry, and sets HttpOnly/SameSite=Lax cookies. `auth/routes.rs` implements password change, session list/revoke, and password-confirmed account deletion. | Security/backend | Rerun `changing_the_password_ends_every_other_session`, `a_session_can_be_revoked_by_its_opaque_id`, `deleting_an_account_takes_its_scenarios_with_it`, and unauthenticated API tests in `crates/finplan_server/tests/api.rs`. Add expiry, stale-session, failed-password and concurrent-revocation checks. Verify deletion with active workers and result/cache cleanup. |
| Recovery and email verification | **Gap.** `auth/routes.rs::router` has no forgot/reset-password or verification routes; profile email replacement does not verify ownership of the new address. Existing password change requires the old password. | Security/backend; operator for delivery | Implement single-use expiring hashed recovery/verification tokens, generic responses, token consumption and session invalidation tests using a local mail sink. Choose delivery provider and recovery policy separately; test the deployed delivery path before launch. |
| Cross-user access | **Verified (source) guards; exhaustive verification gap.** `api/mod.rs::owned_scenario`, `api/runs.rs::owned_run`, user-filtered profiles/taxes and `analysis/jobs.rs::Registry::owned` scope access. Existing two-user scenario test covers GET scenario/accounts/events; `an_analysis_belongs_to_the_user_who_started_it` covers analysis access. | Security/backend | Enumerate every route in `api/*::router` and `auth/routes.rs::router`. In one temporary database register two isolated users; test read/create/update/delete/reorder/duplicate/compile, nested reference injection, runs/results/ledger/cancel, shared profiles/taxes, caches/layouts and sessions with the other user's IDs. Assert denied requests leave owner rows unchanged. Public health/history preset access should be an explicit exception. |
| Hosted cookie/origin protections | **Verified (source) partial; deployment blocked.** `src/config.rs` defaults secure cookies to false for local use. `src/lib.rs::build_cors` restricts origins and permits credentials. No explicit mutation-origin/CSRF middleware was found in that router. CORS alone is not a complete mutation authorization policy. | Security/backend and operator | Preserve local mode; document/enforce hosted secure-cookie configuration. Define cookie-versus-bearer origin rules; test trusted, hostile, absent and null Origin requests, including auth/session endpoints. Verify TLS termination and real browser cookies on the intended hostname. |
| Authentication throttling and abuse | **Gap.** No login/register rate limiter was found in `auth/routes.rs`, `lib.rs` or `config.rs`; password hashing runs synchronously in handlers. | Security/backend | Bound per-client and per-account attempts, trust proxy addresses only under explicit configuration, avoid account enumeration, and cap concurrent expensive hashes. Test limit/reset behavior and generic failures locally. |
| Complete portable archive | **Gap.** Browser `web/components/account/download.ts::collectScenario` exports scenario/assets/accounts/positions/events but omits referenced return, inflation and tax definitions. `DataPanel.tsx` wraps these in Export all; reads are not a consistent transaction. No archive/import route exists in `api/mod.rs`. | Archives/backend with B contract owner | Build one versioned transactional server archive including recursive distributions, tax brackets, cost bases and all referenced assumptions. Add validated dry-run import with ID remapping, bounded size/depth and atomic rollback. Keep credentials out and portability independent of paid status. Fresh-database seeded round trip must preserve inputs/results. See archive audit below. |
| Immutable inputs and history | **Gap at inventory; assigned to B.** `runner/mod.rs::execute` loads the live ScenarioGraph after queuing; `api/runs.rs::create` deletes earlier runs. `0008_one_run_per_scenario.sql` enforces one run; result account queries join live labels. Seed is nullable and no immutable input version is recorded in the baseline schema. | B; F consumes contract | Require transactionally stored full inputs, effective seed/model identity and historical labels, plus queue/restart/rename/delete tests. Do not attach live input exports to historical results or claim old inputs are reconstructable. F comparisons/reports remain gated on this work. |
| Compute admission and quotas | **Verified (source) partial; gap.** Run workers have a semaphore but `runner/mod.rs` uses an unbounded channel. Analysis has a separate semaphore and an in-memory registry. `api/analysis.rs` caps steps=12, axes=4, varied=3, sweep cells=512 and iterations=2000. `api/runs.rs` caps iterations but only lower-bounds batch/parallel sizes and does not cap percentile count. No per-user admission quota was found. | Runtime/backend | Add shared global and per-user admission limits across runs and analyses. Budget cells × iterations × horizon plus retained paths/entity counts; cap percentile count, batching and parallelism. Return an actionable retry response, test saturation/cancellation/release, and benchmark a bounded worst-case fixture. A worker semaphore does not bound queued memory or guarantee fairness. |
| Cancellation and restart | **Verified (source) partial; runtime untested/gap.** Run cancellation uses atomic flags/status guards; `runner::requeue_orphans` restarts queued/running rows. Analysis jobs are memory-only; only finished sweeps are persisted through `analysis/cache.rs`. | Runtime/backend with B | Test queued/running cancellation, duplicate enqueue, restart and deletion during work using a disposable DB. Require snapshot replay for runs. Persist analysis jobs or clearly expose interruption and safe retry after restart; make lost jobs distinguishable from another user's IDs. Confirm terminal states and permit release. |
| Migration/release safety | **Verified (source) migration mechanism; rollout untested.** `src/db.rs` enables foreign keys/WAL and executes the SQLx migrator. `src/main.rs` exposes migrate. | Operator/backend | Upgrade a disposable prior-version database with representative data; inspect foreign-key integrity and run data after migration. Record a pre-upgrade backup and tested rollback/restore strategy. Never assume destructive historical migrations can recover discarded runs. |
| Operational backup restoration | **Gap in repository; deployed setup blocked.** No backup script or restore drill was found in inspected scripts, server README or roadmap implementation. SQLite uses WAL (`src/db.rs`); portable input archives are not operational backups. | Operator | Use a consistent SQLite backup method, document encrypted storage/access, retention, recovery point/time targets, and ownership. Restore to an isolated service; verify users, assumptions, jobs and results, including interrupted work. Record date, artifact identifier and measured recovery. Do not copy a live main DB file alone and assume WAL data is included. |
| Monitoring and log privacy | **Verified (source) basic tracing; operational gap.** `lib.rs` installs HTTP TraceLayer; `/api/health` returns static `ok`; runner/cache log errors. No alerting, readiness check or incident runbook was found. | Operator/backend | Add DB/worker readiness and bounded queue/failure/latency metrics; define alerts and incident owner. Inspect actual logs from malformed inputs and worker failures to ensure no session credentials or plan contents escape in error strings. Test alert delivery without exposing financial fixtures. |
| Billing and entitlements | **Gap in inspected repository; provider integration blocked on decisions.** No checkout/subscription/webhook/entitlement routes or configuration were found in `api/mod.rs`, `auth/routes.rs`, `config.rs` or frontend API/components searches. | Product and billing/backend | Record provider, paid feature boundary, cancellation/downgrade/grace policy and operational owner. Implement the contract below with local mocks before sandbox provider tests. No prices, live checkout or external communications are authorized by this inventory. |
| Privacy, model scope and support | **Gap in inspected documentation; operator/legal review blocked.** Roadmap I proposes these artifacts; no hosted policy, retention schedule, support contact or incident process was found in inspected docs. | Product/operator; legal reviewer | Draft truthful storage/export/deletion/model-scope documentation matching implementation. Supply actual support channel and response ownership, retention/backup deletion policy and service-provider inventory. Obtain appropriate review for final hosted positioning; source inspection is not certification. |
| CI and reproducibility | **Verified (source) lockfiles; CI gap.** `Cargo.lock` and `web/pnpm-lock.yaml` exist. `.github/workflows/build-cli.yml` builds the CLI; `build-web.yml` targets the older Trunk app/branch; `release.yml` packages CLI binaries. These do not establish Next.js/server release validation. | Integration/CI | Add non-deploying checks for fmt/clippy, core/server tests, generated binding drift, frozen-lockfile frontend install, typecheck/lint/test/build and desktop acceptance. Select the package manager matching the existing lockfile; add dependency audit and release artifact checks. Record combined test output before closing gates. |

## Portable export/import audit

Two incompatible JSON shapes already use `finplan.scenario`:

- Browser export uses `version: 1` and numeric API references; it is missing
  assumption definitions and contains no immutable run inputs. Its top-level
  scenario object may carry summary fields, but those do not constitute complete
  simulation results or a recoverable input snapshot.
- `scripts/export-scenario.py` uses `format_version: 1` and name references. It
  includes the user's return/inflation/tax library, nested distributions and
  events, holdings and cost bases. It intentionally excludes runs/results.
  `scripts/import-scenario.py` rebuilds directly into SQLite, reusing existing
  target-user profiles by name. Matching names with differing definitions must
  not silently change restored assumptions. It is an operator script, not an
  authenticated archive-import API. Round-trip completeness was not executed in
  this inventory, and name ambiguity/concurrent exports require explicit tests.

The new archive owner should document adapters or an explicit rejection for both
legacy shapes; do not treat both as the same version merely because their format
strings match. Never feed a browser archive into the script assuming compatibility.
Use archive-local IDs rather than display names to resolve recursive references.
Test two same-name profiles with different distributions, foreign-user IDs, nested
withdrawal sources, event chains, tax brackets and lot acquisition dates/cost bases.
Import retries must not leave half-created scenarios or duplicate entities.

## Billing contract to settle before provider wiring

Proposed contract for product review, not an approved feature boundary:

1. Store provider customer/subscription IDs against the authenticated user, a
   normalized subscription state, effective access dates and a durable webhook
   receipt keyed by provider event ID. Never accept paid status from the browser.
2. Centralize server capability checks; apply resource/compute quotas at admission
   to runs and every analysis type. Do not gate core correctness, warnings, owned
   data reading, export or account deletion behind a subscription.
3. Define active, cancellation-at-period-end, past-due/grace and expired behavior
   as explicit transitions. Downgrade blocks only agreed premium actions and
   preserves existing data. Decide treatment of queued work at policy transitions.
4. Verify webhook signatures before processing; use transactional idempotency and
   authoritative provider-state reconciliation for duplicate/out-of-order events.
   A checkout redirect alone must never grant entitlement. Reconciliation must
   recover a missed webhook and avoid linking a subscription to another user.
5. With a fake provider, test checkout retry, renewals, cancellation, failed payment,
   grace expiry, duplicate/out-of-order events and reconciliation. Repeat against
   provider sandbox credentials after product decisions; operator verifies support
   and payment lifecycle before accepting money.

## Closure evidence

Each owner should append a dated result, exact test/fixture or runbook reference,
remaining limitations and reviewer. Replace source-only findings with verified
runtime evidence only after execution. Integration owns the combined acceptance
journeys in handoff H and the final `cargo fmt`, `cargo clippy`, Rust/frontend
checks; this documentation-only inventory introduces no runtime/API/schema change.
Paid hosting remains gated on unresolved authentication, portability, isolation,
compute, recovery and operational controls even if desktop usability fixes pass.
