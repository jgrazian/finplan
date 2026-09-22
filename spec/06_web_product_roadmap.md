# Web correctness and product implementation roadmap

This plan follows the web-UI review on `feature/web-ui`. Items below are proposed
work unless explicitly marked implemented. No pricing or demand assumptions have
been validated. Preserve the existing local/self-hosted option.

## Implemented in the current changes

### Review #1 — signed outcomes

- `web/lib/format.ts`: preserve negative and small nonzero balances; show unavailable
  nonfinite values as an em dash, not as zero.
- `web/components/charts/geometry.ts`: signed linear domains and unclipped positive
  log domains. `ChartCanvas.tsx` draws a distinct zero line.
- `web/components/charts/stack.ts`, `series.tsx`, and
  `web/components/results/NetWorthChart.tsx`: separate positive/negative stacks and
  overlay signed net worth. Scale to the gross stack extents, not just their net.
- `ResultsScreen.tsx` / `ChartToolbar.tsx`: linear fallback and explanation for
  nonpositive paths; account composition is always linear.
- Regression coverage: `web/tests/results-correctness.test.mts` (`cd web && npm test`,
  using Node 22.18+ or Node 24 for native TypeScript stripping; no new test dependency).

### Review #2 — funding is not terminal wealth

- `crates/finplan_core/src/simulation.rs`: record the first cash shortfall after all
  same-date event passes settle and at the terminal checkpoint. Ignore deficits
  smaller than or equal to half a cent. Check each cash account independently;
  property/investments elsewhere do not fund it automatically. Ordinary loan
  principal is not a cash shortfall. Keep the warning after recovery, and run the
  check even when ledger collection is disabled.
- `crates/finplan_core/src/apply.rs`: surface trigger and chained-effect failures
  that were previously swallowed, so skipped processing cannot pass the check.
- `crates/finplan_core/src/model/results.rs`: retain `success_rate` as the legacy
  positive-terminal-net-worth metric; add optional `funding_success_rate` for paths
  with neither a settled cash shortfall nor an event-processing warning. Count it
  over all Monte Carlo iterations, not the displayed percentile paths.
- `crates/finplan_server/migrations/0005_run_funding_success.sql`,
  `src/runner/store.rs`, `src/api/runs.rs`: persist/export the new measurement.
  Historical rows remain NULL; do not infer path-wide funding from yearly snapshots.
- `web/components/results/SuccessRate.tsx`, `WarningList.tsx`,
  `web/components/account/DataPanel.tsx`, `web/lib/view/results.ts`: use accurate
  labels, show both metrics, scope warnings to the selected path, and ask users to
  rerun when the funding check was not measured.
- Tests: `crates/finplan_core/src/tests/funding.rs` and
  `crates/finplan_server/tests/api.rs` cover late recovery, same-date funding,
  liabilities, zero/tiny balances, skipped effects, aggregation, and legacy data.

**Boundary:** this funding check measures modeled cash balances at simulation
checkpoints plus event-processing integrity. It does not detect omitted spending,
prove tax-law completeness, guarantee future outcomes, or enforce reserve/bequest
objectives. Zero terminal wealth and a correctly serviced liability do not, by
themselves, fail this check. TUI/optimization consumers retain their existing
terminal-wealth metric; exposing the new metric there is separate follow-up work.

## Delivery order

1. **Trust:** immutable inputs/history (#4), correct statistical presentation (#3),
   preflight/assumption checks, and portable backups.
2. **Usability:** retained drafts (#5), responsive/accessibility work (#6), and
   guided plan creation.
3. **Decision value:** scenario comparison, diagnostics, and reports.
4. **Paid pilot:** production security/operations and demand validation.

Snapshot work should precede durable comparisons/reports. Draft and responsive
work can proceed independently. Regenerate bindings after every API type change
with `./scripts/gen-bindings.sh`; never hand-edit `web/lib/api/generated/*`.

## A. Review #4 — immutable run inputs and reliable stale-state detection (P1)

### Implementation path

1. Add plan revisions and immutable run-input records in a new migration under
   `crates/finplan_server/migrations/`. Capture the full dependency graph: scenario,
   accounts/lots, assets, events, tax tables, return/inflation distributions.
2. In `src/api/runs.rs::create`, take a consistent database transaction snapshot,
   compile it, materialize the effective RNG seed, and persist the snapshot and
   input hash before enqueueing. Include engine/schema versions and RNG settings.
3. In `src/runner/mod.rs::execute`, compile only those recorded inputs. Recovery
   after restart must not re-read the currently edited scenario.
4. Decouple historical account/event records from live rows in the result schema.
   Store immutable labels and run-local identifiers in `src/runner/store.rs`.
   Remove cascading deletion of historical account points and current-name joins
   in `src/api/runs.rs`; migrate old runs without pretending their missing inputs
   can be recovered.
5. Return `input_revision`/`input_hash` and current revision in run/scenario API
   types. Shared profile/tax edits must invalidate every dependent scenario (or
   use a dependency-aware content hash); list reordering alone should not.
6. Update `web/lib/hooks/useWorkspace.ts`, `useRun.ts`, `web/app/App.tsx`, and
   `web/components/layout/AppHeader.tsx`: compare each scenario with its own run's
   revision. Show a persistent Results banner identifying stale inputs.
7. Tag requests/results with scenario, run, and selected series. Preserve the last
   successful result during a rerun, reconnect to queued/running jobs on refresh,
   and show explicit loading state when percentile detail is being replaced.

### Acceptance

- Edit while queued, rename/delete an account after completion, and restart a
  worker: the old run remains readable and reproducible from its stored inputs.
- Changing a referenced profile marks all affected plans stale; switching scenarios
  never borrows another scenario's data or freshness state.
- A new series label never appears over detail from the previous series.

Tests: `crates/finplan_server/tests/api.rs`, a new runner-recovery test module,
`web/tests/run-state.test.*`, and browser navigation/network-delay tests.

## B. Review #3 — statistical envelopes versus representative paths (P1)

**Implemented.** Real pointwise envelopes and terminal aggregates now come from
all iterations, independently of representative paths ranked by terminal nominal
net worth. Migration `0006_run_real_quantiles.sql` stores the new measurements;
historical runs remain explicitly unmeasured. The web uses response-owned path
IDs for atomic detail changes and identifies dollar base dates. Exact type-7
annual vectors were selected after benchmarking against t-digests.

See [measurement contract, benchmark results and test coverage](results_quantiles_benchmarks.md).
Review #4's immutable inputs and broader request/recovery work remain separate.

### Implementation path (completed)

1. Make the immediate copy accurate in `web/components/screens/ResultsScreen.tsx`,
   `results/RunSummary.tsx`, `ChartReadout.tsx`, and `web/lib/view/results.ts`:
   existing stored paths are ranked by **terminal nominal net worth**, not
   pointwise percentiles or median real wealth. Label their selection basis.
2. In `crates/finplan_core/src/simulation.rs` and `model/results.rs`, accumulate
   real-dollar net worth at a common date grid across all iterations. Compute
   P5/P50/P95 at each date; benchmark exact annual vectors against quantile sketches
   before choosing storage. Define quantile interpolation and failed-path handling.
3. Compute real terminal aggregates from each iteration's own deflated wealth;
   never divide an aggregate nominal mean/percentile by one selected inflation path.
4. Persist a separate quantile-envelope series through `src/runner/store.rs`, a
   migration, and `src/api/runs.rs`. Keep representative path IDs/ledgers separate;
   a pointwise quantile has no single coherent cash-flow ledger.
5. Refactor `web/lib/view/results.ts` and `components/charts/series.tsx` to render
   an actual envelope alongside the explicitly selected representative path.
   The cash-flow table, account breakdown, and ledger must share that path ID.

### Acceptance

- At every date, pointwise P5 <= P50 <= P95, including when sampled paths cross.
- Stochastic inflation fixtures where nominal and real ranks differ yield the
  correct real quantiles/aggregates; all displayed units identify their base date.
- Selecting a path changes detail atomically without changing the envelope.

Tests: `crates/finplan_core/src/tests/results_quantiles.rs` (new), API fixtures,
`web/tests/results-mapping.test.*`, and performance benchmarks.

## C. Review #5 — durable drafts and predictable persistence (P2)

### Implementation path

- Add `web/lib/drafts/` with drafts keyed by scenario ID + entity type + stable
  entity ID, not display names. Record the base revision for conflict detection.
- Lift draft ownership out of `plan/EventEditor.tsx`,
  `portfolio/AccountInspector.tsx`, `portfolio/AssetInspector.tsx`,
  `portfolio/PositionForm.tsx`, and `profiles/ProfileInspector.tsx`.
- Integrate guards into `web/lib/nav/context.tsx`, `web/app/App.tsx`, row selection,
  scenario changes, and dialog dismissal: **Save / Discard / Stay**.
- Add `beforeunload` for truly dirty state; optionally restore drafts from
  session-scoped storage with explicit privacy/retention behavior. Never silently
  replay financial mutations after session expiry or a revision conflict.
- Standardize explicit Apply for structured forms and clearly indicated autosave
  for scalar fields. Serialize/version autosave requests in
  `plan/ScenarioStrip.tsx` so an older response cannot replace a newer edit.
- Preserve failed submissions and surface errors by field. Use server revision
  preconditions for concurrent-tab editing rather than last-write-wins.

### Acceptance

Browser tests cover tab/row/scenario/back navigation, reload, offline rejection,
expired sessions, overlapping saves, and concurrent edits. No silent draft loss;
Cancel/Stay preserves exact values, Save waits for confirmation, Discard is explicit.

## D. Review #6 — responsive and accessible reading/editing (P2)

### Implementation path

- Move structural inline grids to classes in `web/app/globals.css`.
- `components/layout/AppShell.tsx`: stack the rail below/above content at smaller
  widths. `AppHeader.tsx`: compact navigation and move scenario/run controls into a
  second row rather than overflowing the viewport.
- `screens/PlanScreen.tsx`, `plan/EventRail.tsx`, `EventEditor.tsx`: use list ->
  full-width editor navigation on phones, then one-column When/What sections.
  Preserve selected entity and draft when changing viewport or returning to list.
- `screens/AccountsScreen.tsx` / `AssetsScreen.tsx`: full-width detail drawers on
  small screens. Put table overflow inside labeled containers, not the document.
- `results/SuccessRate.tsx`, `RunSummary.tsx`, `AccountBreakdown.tsx`,
  `CashFlowLedger.tsx`: prioritize outcome/assumptions before dense detail.
- `app/design-system.css`: increase supporting/table type, audit contrast (4.5:1
  for ordinary text), focus visibility, and touch targets. Retain the visual
  identity without repeated framing and tiny low-contrast labels.
- `ui/Dialog.tsx`: native dialog or tested modal primitive with focus trap,
  restoration, inert background, and safe dismissal during submissions.
- `charts/ChartCanvas.tsx` / `results/useYearFocus.ts`: keyboard year navigation,
  touch/pointer selection, accessible summary/data table, and announced selection.
- Replace raw engine tags such as `PauseEvent` in `web/lib/view/events.ts`.
  Hide the Analysis destination in `web/app/App.tsx` until a usable screen exists.

### Acceptance

Browser screenshots and interaction tests at 390, 768, 1024, and 1440px; 200% zoom;
keyboard-only and screen-reader spot checks. No document overflow, overlapping
editor columns, unreachable actions, or focus escape from modals.

## E. Guided first-plan flow and configuration preflight (P1/P2)

### Implementation path

- Add `web/components/onboarding/` and a new onboarding route/navigation state.
  Ask about household/dates, balances, income/spending, major changes, and
  assumptions. Start simple; advanced event/amount editors remain available.
- Add versioned plan templates in `crates/finplan_server/src/domain/templates.rs`
  and a transactional creation endpoint. Generate actual event/account records
  rather than maintaining an independent onboarding simulation model.
- Offer a clearly fictional demo plan and a completion checklist from
  `components/scenario/NewScenarioDialog.tsx` and `web/app/App.tsx` empty states.
- Add `src/compile/diagnostics.rs` for preflight diagnostics: no spending, no funding
  path for spending, unmapped assets, dates outside the horizon, missing age data,
  empty enabled events, and unconfirmed tax/return assumptions. Separate definite
  configuration errors from heuristic warnings; link diagnostics to entity/field.
- A funding rule cannot be proven sufficient statically. Use preflight for missing
  intent, then the runtime funding check for realized shortfalls.
- Add tax assumption editing under `web/components/taxes/`, using existing tax API
  routes. `plan/ScenarioStrip.tsx` should link to the actual editable configuration.
- Version seed presets in `src/seed.rs`: explicit tax year, filing status, state
  assumptions, source/as-of metadata, and modeled limitations. Do not silently
  treat a 2024 single-filer flat-rate preset as current household tax advice.

### Acceptance

A new user can build and run a useful baseline without knowing the trigger DSL.
A plan with spending but no modeled funding route receives an actionable warning.
Every tax/market preset names its assumptions and supported scope.

## F. Baseline/variant comparison — first paid-value workflow (P2)

Depends on A; use corrected metrics from B when available.

- Add versioned comparison/override requests to `src/api/` and
  `src/domain/comparisons.rs`. Start with retirement timing and spending; reference
  explicit event/effect IDs or template parameters, never heuristics on names.
- Reuse existing scenario duplication in `src/domain/mod.rs` for Save as variant.
  Temporary overrides must not mutate the baseline or shared profile libraries.
- Use matched seeds/RNG configuration for baseline and variants; keep identical
  account/profile identity maps so reordering does not change random-stream meaning.
- Build `web/components/screens/AnalysisScreen.tsx`,
  `web/lib/view/comparisons.ts`, and `web/lib/hooks/useComparison.ts`.
  Integrate or replace the currently unused `results/WhatIfPanel.tsx`.
- Show changed inputs, cash-funding rate, positive terminal wealth, first-shortfall
  distribution, liquid terminal wealth, taxes, and real wealth quantiles.
- Add separate outcome definitions for reserve/bequest goals and liquid-asset
  depletion; do not overload either existing success statistic.

Acceptance: identical variants reproduce zero deltas; a changed retirement age
alters the intended events only; base inputs remain unchanged; side-by-side units,
seeds, horizon, revisions and uncertainty are explicit.

## G. Explanations, sensitivity and shareable reports (P2/P3)

- Extend runtime diagnostics in `finplan_core/src/model/results.rs` and
  `simulation.rs` with structured first-shortfall date/account/reason, liquid vs
  illiquid wealth, and milestone data. Persist them separately from prose warnings.
- Aggregate diagnostics over *all* iterations in the runner, not three paths.
- Add `web/lib/view/diagnostics.ts` and `components/results/PlanDiagnostics.tsx`
  to answer when and why funding fails, where wealth is inaccessible, and what
  changed versus a prior revision. Link to the relevant year/account/event ledger.
- Add bounded one-at-a-time sensitivity runs through `src/api/` and the worker
  queue. Report tested changes, not causal claims or guaranteed improvements.
- Add a print/report route and `components/reports/`: revision, assumptions,
  definitions, units, comparisons, risks and limitations on every report.
  Private sharing needs scoped expiring authorization; no public plan URLs by default.

Acceptance: explanations reconcile to stored path facts; reports contain everything
needed to interpret numbers without the app. No implied personalized financial
recommendation or invented explanation.

## H. Complete portable export/import and recovery (P1 before paid hosting)

- Replace the incomplete browser archive in `components/account/download.ts` with
  versioned server export/import under `src/api/archives.rs` and
  `src/domain/archives.rs` (new).
- Include scenario, accounts/lots, assets, events, and all referenced return,
  inflation and tax definitions. Include complete input snapshots when exporting
  runs; credentials/session tokens must never be exported.
- Capture exports transactionally; import with validation and complete ID remapping,
  including recursive triggers/effects/amounts and shared profile references.
- Provide preview/dry-run import and schema-version migration. Keep JSON explicit;
  add a separate YAML adapter only if CLI interoperability is implemented/tested.
- Wire Import/Export/Restore in `components/account/DataPanel.tsx`. Keep data
  portability available regardless of subscription status.

Acceptance: export -> fresh database -> import -> seeded simulation preserves
inputs and results. Include customized distributions, nested expressions, tax
brackets, cost bases and shared dependencies. Test malformed/oversized archives
and ownership boundaries. Separately test operational database backup restoration.

## I. Production readiness and monetization experiment (P3)

Before accepting sensitive hosted financial data:

- `finplan_server/src/auth/`, `src/lib.rs`, `src/config.rs`: threat model/review,
  account recovery, email verification, authentication throttling, CSRF/origin
  protections appropriate to deployment, secure cookie enforcement and session
  policy. Audit ownership on every new route. Consider MFA/passkeys.
- `src/runner/mod.rs` and `src/api/runs.rs`: bounded queue, per-user quotas and
  concurrency limits, deduplication/idempotency, timeouts, cleanup/retention, and
  cancellation/recovery tests. Check all compute parameters, not just iterations.
- Add monitoring without logging plan contents, backup/restore drills, tested
  migrations, TLS/deployment documentation and an incident process.
- Update `README.md` and add hosted privacy/model-scope documentation: distinguish
  local operation from hosted storage, collection/retention, deletion, limitations,
  and exports. Get appropriate legal/compliance advice for positioning.
- Establish CI for Rust tests/clippy/fmt, generated-binding drift, frontend
  typecheck/lint/unit/browser tests, dependency audit and release builds. Commit a
  package-manager lockfile as a dedicated dependency-reproducibility change.

Commercial experiment (not a feature commitment): recruit 10–20 FIRE/equity-comp
planners, observe unassisted plan creation/comparison, and test willingness to pay.
Measure activation, time to useful comparison, support burden, repeat usage,
retention and actual paid conversion. Interview people who abandon setup.

Test $99–149/year for hosted comparisons/history/reports or a $29–49 one-off
planning/report package. Keep local core and export free. Introduce billing only
after validated value and trust gates; add server-side entitlements, webhook
idempotency and grace-period behavior. Avoid starting with bank aggregation,
a large adviser CRM, or more exotic distributions before demand warrants them.
