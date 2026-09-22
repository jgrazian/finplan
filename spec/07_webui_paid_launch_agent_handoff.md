# WebUI paid-launch agent handoff

Status: implementation started; see the first-batch record below. Remaining
workstreams are proposals, not completed release gates.
Source: hands-on new-account review on September 13, 2026, plus focused code inspection.

## First implementation batch — September 13, 2026

Three sub-agents implemented/reviewed metrics, freshness, and diagnostics/readiness;
the integrating agent handled setup improvements, product copy, and acceptance.
All changes are uncommitted. Mobile scaling remains deferred.

| Workstream | Delivered in this batch | Still pending |
| --- | --- | --- |
| A | Funding defaults for new Web solves/sweep graphs; explicit outcome labels and selectors; selected-constraint uncertainty; legacy cache compatibility; readable parameter names | Full release acceptance across future reports/comparisons |
| B | Immediate save invalidation and Results banner; shared-profile/tax invalidation; enqueue-time comparison; scenario/request guards; active-run reconnection; session-captured display context | B2 immutable snapshots/history; durable revision precision across reload; historical ledger retention |
| C | Atomic navigation to the new scenario's Portfolio; end-date/age preview and validation; inline asset return choice; clear cash-versus-position guidance | Resumable wizard, transactional templates, integrated allocation and household/funding setup |
| D | Selected-path funding/processing warnings above charts with Review plan action; corrected fixed-price purchasing-power explanation | Structured account/amount deep links, guided funding rules, preflight and tax-assumption editing/versioning |
| E | Account/tax labels and concise account-settings/export copy; solver precision copy and readable analysis parameter names | Full vocabulary/save-state audit and remaining advanced controls |
| F | No implementation claimed | Depends on B2; comparisons, what-if, history, reports |
| G | Source-backed inventory in `spec/08_hosted_readiness_matrix.md`; export incompleteness disclosed | Remediation and operational/sandbox verification; provider/product decisions |

Tests cover outcome divergence, selected solver constraints and legacy serialization,
shared-dependency invalidation, request/freshness helpers, diagnostic scope, atomic
navigation, and leap-year plan dates. Desktop browser checks cover creation,
direct asset mapping/validation, and result diagnostics/freshness. This batch does
not establish paid-pilot readiness. See the task report for executed check results.

Known limits: timestamp comparison cannot resolve all same-second changes after a
reload; local invalidation can conservatively mark unused-library/reorder edits
stale. The server still replaces prior runs and compiles live inputs at execution.
Captured frontend display context is session-only and is not historical provenance.
Keep comparisons/reports behind B2 rather than assuming these limits are solved.

## Objective and scope

A new user can create a plausible plan, understand why it fails, change an
assumption, and compare the updated result without understanding the engine.
Prepare the hosted WebUI for a paid pilot while preserving local/self-hosted use.

**Mobile scaling is explicitly deferred.** Do not change responsive breakpoints,
mobile navigation, sidebar stacking, or phone/tablet layouts in these tasks.
Desktop keyboard access, field labels, and understandable errors remain in scope.
Do not expand into bank aggregation or a general engine rewrite.

Read `AGENTS.md` and `spec/06_web_product_roadmap.md` first. That roadmap documents
earlier work and proposals; inspect current code before treating an item as absent
or complete. In particular, funding checks and real quantile envelopes already
exist. Extend them rather than rebuilding them. This handoff does not authorize
production deployment, live payments, or external communications.

## Evidence to preserve

The local test account contains `WebUI Review — Sample Retirement`: start
2026-09-13, birth date 1981-01-01, 30-year horizon, $50,000 checking, $500,000 in
a fictional SAMPLE holding mapped to US Total Market, and an annual expense
funded only from checking. Spending was initially $40,000, then saved as $41,000.
Use isolated test fixtures instead of depending on or deleting this account.

- Results reported 0% cash funding. The spending sweep reported 57.6–90.8%
  “Success rate.” These are different measurements, not inconsistent arithmetic:
  `success_rate` measures positive terminal wealth; `funding_success_rate` measures
  checkpoint cash funding and event-processing integrity.
- An applied spending change left the previous run visible without an immediate
  stale indicator. After reload, `results stale` appeared. Reproduce before
  assigning a cause; do not describe freshness detection as entirely absent.
- New scenario creation opened “No results yet” before portfolio setup.
- Investment setup required separate account, holding, asset, and return mapping
  steps. New inline assets were initially unmapped and held at their opening price.
- The unfunded-checking warning correctly explained that other assets do not
  automatically fund spending, but the explanation was in the results sidebar.
- Scenario creation defaulted to the profile labeled US Federal 2024 (single).
- Solver copy claimed an answer “exact to the dollar.” UI copy included raw enum
  names and implementation explanations about server flags and JSON versus YAML.
- What-if was disabled. No billing screen was encountered. Security, recovery,
  billing, and operational readiness were not audited; these are verification
  tasks, not confirmed missing functionality.

## Coordination and delivery sequence

One integrating agent owns shared contracts, migration numbering, generated
bindings, and final acceptance. Each workstream below is a separate handoff task;
it may contain multiple small PRs. Agents must report dependencies and avoid
editing another task's shared files without coordination.

1. Wave 1: A (metrics), B (freshness), and G (read-only readiness inventory).
   A publishes metric/constraint names; B publishes revision/snapshot contracts.
2. Wave 2: C (onboarding) and D (diagnostics/assumptions) agree on their preflight
   and funding-rule interfaces before implementation. Merge A/B first where needed.
3. Wave 3: E (copy consistency) integrates after A/C/D to avoid competing edits.
   F (comparisons/history/reports) starts after immutable input support from B.
4. Paid pilot: integrate the bounded remediation from G and complete H's tests.
   Do not hold core defect fixes for the entire Pro feature roadmap.

With four agent slots, reserve one for integration and run at most three
independent workstreams concurrently. Use isolated branches/worktrees when
available; if sharing a checkout, assign exclusive file ownership.

## A — Consistent outcomes and solver uncertainty (P0)

**Task:** Make the measured outcome explicit and consistent across Results,
Sweep, sensitivity ranking, Solve, exports, and subsequent comparisons/reports.

Starting points:
`web/components/results/SuccessRate.tsx`, `web/lib/view/sweep.ts`,
`web/lib/view/analysis.ts`, `web/components/analysis/SolvePanel.tsx`,
`SensitivityPanel.tsx`, `GraphInspector.tsx`,
`crates/finplan_server/src/analysis/{jobs,results}.rs`,
`crates/finplan_core/src/analysis/solve.rs`, and `model/results.rs`.

- Default new analyses to cash funding. Label terminal wealth as “Positive ending
  net worth,” not unqualified “Success.” Keep both metrics available.
- Carry the selected metric through requests, optimization constraints, ranking,
  cached jobs, graph configuration, CSV headers, and uncertainty calculations.
  Existing saved analyses retain their original metric with an accurate label.
- Preserve `success_rate` semantics for compatibility. Missing historical funding
  measurements display “Not measured — rerun”; never substitute terminal wealth.
- Explain that the funding check includes event-processing warnings and does not
  detect omitted spending or guarantee future outcomes.
- Replace exact-answer claims with search resolution and simulation uncertainty.
  Verify standard error is computed for the selected constraint, not always the
  legacy metric. Report iterations and seed policy; handle infeasible ranges and
  assumptions about monotonicity explicitly.

Acceptance: a fixture with positive final wealth and earlier cash shortfall shows
different, correctly named metrics everywhere. Switching a solve constraint changes
the actual computation. Old caches do not acquire a new meaning. Tests cover null
legacy metrics, exports, selected-constraint uncertainty, and infeasible solves.

## B — Immediate freshness and immutable run provenance (P1)

**Task:** Prevent old results from appearing current after edits and ensure a run
continues to describe the inputs that produced it. Deliver in two PRs if useful.

Starting points: `web/app/App.tsx` (including scenario view mapping),
`web/lib/hooks/{useWorkspace,useRun,useAnalysis}.ts`,
`web/components/layout/AppHeader.tsx`, `web/components/screens/ResultsScreen.tsx`,
`crates/finplan_server/src/api/{runs,scenarios}.rs`, runner modules, and roadmap A.

- B1: reproduce edit -> Apply -> Results without reload. Refresh/invalidate the
  affected scenario's metadata after successful mutations. Show a persistent
  results banner identifying old inputs and offering rerun. Failed saves must not
  falsely claim new data was applied; unsaved drafts are distinct from stale runs.
- Cover accounts, lots, events, scenario settings, assets, and shared return,
  inflation, and tax profiles. A shared dependency edit affects all relevant plans.
- B2: implement/reuse immutable run inputs and revision/hash tracking, including
  effective seeds, model versions, dependency definitions, and historical labels.
  Snapshot transactionally before enqueueing; workers/restarts use that snapshot.
- In-flight completion must not clear staleness when newer edits exist. Guard
  cross-scenario requests and preserve the last successful result during reruns.
- Historical runs lacking reconstructable inputs must be marked as such. Do not
  backfill fabricated provenance. Coordinate schema changes with A/F/G.

Acceptance: $40,000 -> $41,000 immediately shows old results as stale; refresh is
not required. Test shared-profile edits, scenario switching, failed saves, edits
during queued/running work, and reload/restart recovery. Old runs survive entity
renames/deletions without changing their meaning. Snapshot coverage gates F.

## C — Guided first plan and simpler investment setup (P1)

**Task:** Help a new user reach a meaningful first result using the existing data
model and simulation path.

Starting points: `web/components/scenario/NewScenarioDialog.tsx`, `web/app/App.tsx`,
`web/components/portfolio/{NewAccountDialog,PositionForm,NewAssetInline}.tsx`,
`web/components/plan/{EventEditor,EffectFields}.tsx`, and roadmap E.

- Add a resumable setup checklist/flow for household/dates, accounts/allocation,
  income/spending, retirement changes, assumptions, review, and run.
- After scenario creation, open setup rather than an empty Results screen. Offer
  a clearly fictional demo and a route to the advanced editor.
- Provide familiar account presets and salary/spending/retirement event templates.
  Presets create normal engine records; avoid a second modeling implementation.
- Let users enter total account value and allocation without accidentally counting
  the same money as both cash and holdings. Show a reconciled preview before save.
- Choose return assumptions while adding assets. Keep explicit no-growth modeling
  possible, and distinguish it from accidentally unmapped assets.
- Collect funding intent and call D's shared funding/preflight flow. Ask for a
  plan end age/date; do not silently accept a 30-year horizon that ends too early
  for the user's stated objective.
- Preserve completed steps across navigation/reload; prevent duplicate entities on
  retry. Use existing field validation and clear saved/unsaved/error states.

Acceptance: create and run a baseline from an empty account without knowing enum
names, lot mechanics, or trigger DSL. A $500,000 invested account totals $500,000.
The generated plan matches review values and survives reload. Cancel/retry does
not create duplicate records. Existing advanced workflows still work.

## D — Actionable funding diagnostics and assumption preflight (P1)

**Task:** Explain what failed and make model assumptions reviewable before running.

Starting points: `web/components/results/{WarningList,RunSummary}.tsx`,
`web/components/screens/ResultsScreen.tsx`, `web/components/plan/ScenarioStrip.tsx`,
`web/lib/view/results.ts`, `crates/finplan_server/src/compile/`, `src/seed.rs`,
`crates/finplan_core/src/model/results.rs`, and roadmap E/G.

- Promote first funding failures above charts with date, account, amount, and
  applicable path scope. Link to the ledger and affected account/event editor.
- Use structured diagnostic data, extending persistence/API if needed; do not
  recover identifiers or amounts by parsing English warning strings.
- Build a guided funding-rule draft: explicit source accounts/order, timing,
  destination, amount policy, and modeled tax implications. Preview generated
  effects and save only through an explicit user action. Never silently sell
  assets or enable engine-wide automatic funding.
- Separate observations from inferred causes. A selected path's first shortfall
  is not the first-shortfall date of all Monte Carlo runs. Aggregate only if full
  iteration data supports the claim.
- Add preflight diagnostics for unmapped holdings, omitted spending, missing
  funding intent, empty enabled events, age/date gaps, and unconfirmed assumptions.
  Distinguish definite invalid inputs from dismissible warnings. Static analysis
  cannot prove a funding rule will suffice in every simulated market path.
- Show tax year/filing assumptions, inflation, nominal versus real return units,
  source/as-of metadata, and modeled limitations. Preserve old plans' assumptions.
  If refreshing tax presets, verify official sources and effective dates; do not
  simply rename the 2024 preset. Correct the claim that an asset held at a fixed
  opening price is necessarily flat in real purchasing-power terms.

Acceptance: the review fixture explains the checking shortfall despite positive
wealth. Links resolve to the correct records. Adding an explicit funding rule has
a traceable ledger effect; it need not guarantee success. Intentional no-growth
assets can pass review. Assumptions and units remain consistent in all summaries.

## E — Product language and desktop interaction polish (P2)

**Task:** Replace implementation commentary with concise language that helps users
make decisions. Integrate after functional owners settle their interfaces.

Starting points: `web/components/account/`, `portfolio/`, `plan/`, `analysis/`,
`web/lib/view/events.ts`, and existing account/tax term helpers.

- Establish one display vocabulary: Account type, Tax-deferred, Tax-free, Transfer,
  Required minimum distribution, and other familiar terms. Keep API enums intact.
- Replace raw parameter slugs with event names and meaningful units in analysis.
- Rewrite export, preferences, security, empty-state, and help copy. Explain what
  is included/exportable and what users can do; remove YAML/server-flag rationales.
- Standardize action and save labels; visibly label controls, keep keyboard focus
  predictable, and retain inputs on validation failures. Coordinate persistence
  changes with B/C rather than adding a competing draft system.
- Remove the disabled What-if destination until F implements it, or clearly move
  it outside the primary workflow. Preserve the current visual identity.

Acceptance: desktop walkthrough contains no raw engine identifiers as primary
labels, no unsupported statistical claims, and no implementation essays. Form
actions remain discoverable by accessible name. No mobile scaling changes.

## F — Bounded Pro-value workflow (P2, after A/B)

**Task:** Deliver a coherent first premium workflow rather than a broad feature
grab bag. Product proposals are unvalidated; do not invent pricing or demand data.

- F1: saved baseline/variant comparison with changed-input summaries, consistent
  funding metrics, real-dollar outcomes, and run provenance. Start with spending
  and retirement timing attached to stable event/effect IDs.
- F2: guided what-if and a small documented set of stress scenarios using existing
  analysis infrastructure. Temporary changes must not mutate the baseline or
  shared profiles. Matching inputs/seeds must produce zero comparison deltas.
- F3: readable report/print output containing the exact input revision, assumptions,
  warnings, definitions, units, uncertainty, and baseline/variant differences.
  Export historic snapshots, not live inputs beside older results.
- Build on roadmap F/G and existing Analysis components; inspect current APIs
  before adding overlapping comparison or history concepts.

Acceptance: a user saves a baseline, tests one change, compares outcomes, returns
to the unchanged baseline, and exports a self-contained report. History remains
valid after edits. Keep core correctness, warnings, and data portability available
independently of subscription status. Confirm the final paid feature boundary
before entitlement wiring; do not make more iterations the sole Pro proposition.

## G — Hosted paid-pilot readiness (verification, then bounded remediation)

**Task:** Inventory existing capabilities and produce an evidence-backed release
matrix, then implement missing gates in separate focused changes. Read roadmap H/I.

- Inspect authentication/recovery, ownership on all routes, session protections,
  throttling, and account deletion. Use two isolated users for access-boundary tests.
- Verify complete portable export/import, including all referenced assumptions;
  test round-trip restoration independently from operational database backups.
- Verify worker limits/quotas, cancellation/restart handling, migrations, monitoring,
  log privacy, and a documented backup restoration drill.
- Inspect billing/entitlements. If absent, specify server-side entitlement rules
  and provider integration after product/provider decisions are recorded. Implement
  and test checkout, renewal, cancellation, failed payments, grace/downgrade,
  duplicate/out-of-order webhooks, and reconciliation in a sandbox only.
- Preserve access to existing data and exports after downgrade; never silently
  delete plans. Verify limits server-side, including analysis compute dimensions.
- Prepare support/contact, model-scope and privacy/retention documentation, and
  operational ownership. Identify items needing operator or legal input rather
  than claiming certification from code inspection.

Deliverable: each gate has evidence, status (verified/gap/blocked), owner, and
reproduction/test steps. No billing UI observed is not proof billing is absent.
Missing credentials or business decisions block only their dependent integration,
not local mocks, contract tests, inventory, or other implementation work.

## H — Integration and release acceptance

The integrating agent verifies the combined workflow after each wave. Functional
agents own meaningful regression tests for their changes; H does not replace them.

Required journeys:

1. Empty account -> guided baseline -> reviewed assumptions -> first run.
2. Positive terminal wealth with earlier cash failure -> accurate metric labels ->
   diagnostic -> explicit funding-rule edit -> rerun and ledger verification.
3. Save input edit -> immediate stale banner -> rerun -> current result; repeat
   with shared profile edits and editing during an in-flight run.
4. Baseline -> variant -> comparison -> historical report -> unchanged baseline.
5. Legacy run without funding/provenance -> honest missing-data presentation.
6. Failed save, refresh, worker restart, and cross-user access attempts retain the
   expected data and surface actionable errors.
7. Export/import and backup/restore recover a seeded fixture; sandbox billing
   lifecycle enforces the intended access without destroying existing plans.

Validation commands, selected according to changed code:

```bash
cargo fmt
cargo clippy
cargo test -p finplan_core
cargo test -p finplan_server
cd web
npm run typecheck
npm run lint
npm test
npm run build
```

Run appropriate Rust/frontend tests per PR and a combined release pass once the
waves integrate. Run `cargo fmt` and `cargo clippy` per repository instructions;
fix modest warnings and report unrelated major refactors separately. Regenerate
API bindings with `./scripts/gen-bindings.sh` when API structs change; never edit
generated files by hand. Verify no unexpected binding drift after tests. Perform
desktop browser checks using the Browser skill. Do not add mobile acceptance
criteria to this release gate.

Every agent returns: problem and final behavior, files/contracts changed, test
evidence, remaining risks, migration/compatibility notes, and suggested commit
message. Stage only that task's changed files; preserve unrelated work and local
databases. Do not mark a gate complete solely because unit tests pass or a screen
exists. Paid-pilot readiness requires both the core journeys and G's applicable
operational/commercial gates to be verified.
