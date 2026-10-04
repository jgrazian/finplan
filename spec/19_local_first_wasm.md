# Local-first FinPlan: plans and engine in the browser

Status: implemented behind the local-mode flag (2026-10-03, branch
`local-first-wasm`); see "Implementation notes" at the end for what shipped,
where it departs from this text, and what is left. Third of three: [17](17_guest_access.md) guest
access, [18](18_plan_crate.md) the database-free plan crate, then this. It
depends on 18 and retires 17's guest tier.

## Product model

Plans stay on the visitor's device by default, and the simulation engine runs
there too, compiled to WebAssembly. An account is for what can only happen on
a server.

| | Local (default, no account) | Cloud (account) |
|---|---|---|
| Where plans live | This browser (IndexedDB) | FinPlan's server, as today |
| Where runs execute | This device; server offload on request | Server, as today |
| Iterations | Whatever the device can do | Tier limit, as today |
| Multiple devices, backup | Export / import a file | Yes |
| AI review, plan chat, drafts, documents | No (plan must be in the cloud) | Yes, by tier |
| Server offload | Account required (see below) | n/a (already on the server) |
| Works offline | Yes, once loaded (phase 5) | No |

The pitch: "Your plan stays in your browser. Nothing leaves it unless you move
the plan to the cloud, offload a run, or use an AI feature." Every exception
is an explicit action the user takes, and each one says so where it happens.

Iteration caps don't apply to local runs: the CPU is the visitor's, and a cap
enforced in client code is decoration. The UI still keeps people from shooting
themselves in the foot with run-time estimates (below). Pricing therefore has
to move from compute limits to cloud, AI and offload (open question 1).

## Plan homes

Each plan has exactly one home, local or cloud. There is **no two-way sync**
in this spec: syncing one plan across devices with conflict handling is a
project of its own, and copying a plan with one home covers backup and
switching devices.

- **Move to cloud**: export the local plan as a `PlanArchive`, post it to
  `/archives/import`, and on success delete the local copy. Plan-slot limits
  apply as they do for any import.
- **Download to this device**: `GET /scenarios/{id}/archive`, import into the
  local store, and ask whether to delete the cloud copy (default: keep).
- **Export / import a file**: the same `PlanArchive` JSON, for local plans
  with no account at all. This is the backup story for Local.

Runs don't travel with a plan. The archive carries inputs only, and results
are recomputed in the new home.

## Architecture

```
main thread (React)
  └─ PlanApi ── remoteApi ── fetch ──────────────► finplan_server (cloud plans)
            └─ localApi ── postMessage ─► store worker
                                           ├─ finplan_wasm (finplan_plan: graph, edits, compile, results)
                                           ├─ IndexedDB (plans, runs, library)
                                           └─ postMessage ─► compute workers × N
                                                              └─ finplan_wasm (finplan_core batches)
```

### `finplan_wasm` crate

A thin `wasm-bindgen` layer over `finplan_plan` and `finplan_core` with no
logic of its own. JSON strings in and out, typed on the TypeScript side with
the ts-rs bindings the server already generates. At plan size that costs
little, and it avoids a second set of wasm-bindgen types that could drift.

- Store worker: `apply_edit(graph, op) -> graph | error`,
  `compile_report(graph)`, `preflight(graph)`, `snapshot(graph) -> (json,
  hash)`, `import_archive(archive) -> graphs`, `export_archive(graphs)`,
  `project_results(compiled, merged_batches, settings) -> RunResults`.
- Compute worker: `prepare(snapshot_json) -> handle`, `run_batch(handle,
  seed, batch_index, iterations) -> BatchOutput`, `cancel(handle)`.

`MODEL_VERSION` is exported so a stored run knows which engine produced it.

### Parallelism without cross-origin isolation

Running rayon inside WASM needs `SharedArrayBuffer`, which needs COOP/COEP
headers on every page. Those break third-party embeds, payment checkout in
particular. Instead, run N independent compute workers
(`navigator.hardwareConcurrency − 1`, at most 8), each with its own WASM
instance, each running whole batches.

`monte_carlo_simulate_with_config` already splits work into batches with
per-batch seeds and merges per-batch outputs (`OnlineStats::merge`,
`FundingAccumulator::merge`, `SnapshotMeanAccumulator::merge`,
`TaxMeanAccumulator::merge`). Core needs that made public: a serializable
`BatchOutput` (today a local type alias in `simulation.rs`), `run_batch(config,
seed, batch_index, n)`, and `merge_batches(Vec<BatchOutput>) ->
MonteCarloSummary`. Then the result depends on the batch plan, not the worker
count. A core test asserts `merge_batches` of batches run one at a time equals
`monte_carlo_simulate_with_config` for the same seed and batch plan.

Converging runs (`converge`) dispatch rounds of batches and check the metric
between rounds; the coordinator in the store worker does the same.

### Local store

IndexedDB database `finplan`, owned by the store worker:

| Store | Key | Value |
|---|---|---|
| `plans` | local id | `{name, graph (ScenarioGraph JSON), updated_at, last_exported_at}` |
| `runs` | run id | `{plan_id, settings, input_hash, model_version, engine: "wasm" \| "server", RunResults}`; keep the newest 5 per plan |
| `library` | kind + id | return profiles, inflation profiles, tax configs |
| `meta` | key | schema version, device benchmark, persistence state |

- The library is seeded from the same data `seed::seed_user_library` uses,
  exported at build time as JSON and bundled with the WASM. Move that data
  into `finplan_plan` (18) so there's one copy.
- Each edit is one IndexedDB transaction: apply the edit in WASM, then write
  the new graph. A failed edit writes nothing, matching `domain::edit`'s
  atomicity.
- Several tabs: the store worker takes a Web Lock (`navigator.locks`) per
  plan for writes and broadcasts changes on a `BroadcastChannel`, so other
  tabs refresh instead of overwriting.
- Freshness: `snapshot` gives the same `input_hash` as the server, so
  `web/lib/run/freshness.ts` works unchanged.

### Web seam

`web/lib/api/client.ts` exports one `api` object. Split it into a `PlanApi`
interface (the plan-shaped groups: scenarios, accounts, assets, positions,
events, parameters, profiles, tax configs, compile/preflight, runs, results,
ledger, preview, what-if, analysis, archives) implemented by both `remoteApi`
and `localApi`. Auth, account, contact, drafts, documents, suggestions-AI and
plan chat stay remote-only.

- Plan refs: the query string carries `scenario=<id>`. Local plans use
  `scenario=l<id>`, and `web/lib/nav` resolves the ref to the right `PlanApi`.
  Components keep taking numeric ids from the ref.
- Capabilities: `usePlanCapabilities(ref)` says what the home supports
  (AI, offload, goal-seek quota). Components already render locked states for
  entitlements, so local plans reuse them with "Move to cloud to use AI".
- `web/lib/view` stays the only mapping layer. Both homes return the same
  generated types, so the screens don't know which home they're showing.

### Build

- `scripts/build-wasm.sh`: `cargo build -p finplan_wasm --release --target
  wasm32-unknown-unknown`, `wasm-bindgen --target web`, `wasm-opt -Os`, output
  to `web/lib/engine/pkg/`. Not committed (binary output). `pnpm build` and
  `pnpm dev` run it first.
- Pin `wasm-bindgen-cli` to the crate's `wasm-bindgen` version. The selfhost
  image (`ops/selfhost`) needs the wasm32 target and the CLI.
- The `.wasm` loads lazily after first paint; budget ≤ 2 MB compressed. The
  phase 0 spike measures it.

## Performance and run estimates

The spike measures native versus WASM throughput on the benchmark fixtures in
`crates/finplan_core/benches`. A 1.5–3× gap single-threaded is expected.

On first load the compute workers run a short fixed calibration plan and
store a rate in `meta`: cost units per second, where cost is `iterations ×
duration_years × (accounts + assets + events)`, the formula `create_run`
already uses to refuse oversized runs. Before a local run the UI estimates
`cost / (rate × workers)`:

- under 10 s: run;
- 10–60 s: run, with the estimate and a Cancel button on the progress bar;
- over 60 s, or the device reports ≤ 2 cores or a low-memory hint: offer
  "Run on FinPlan servers (about N s)" next to "Run here anyway".

Memory guard: kept paths and ledgers grow with iterations. The projection
keeps percentile paths only, as the server does, but a batch holds its
iterations in memory until merged, so the coordinator sizes batches to keep
each worker under about 256 MB.

## Server offload

For a local plan on a device that is too slow, or for an analysis too big to
run in a browser. The plan does leave the device for this, so it is an
explicit, per-run action, never automatic.

### Who

Requires an account: offload costs real compute, and quota needs an
identity. Proposed: Pro gets a monthly budget in cost units, and a Free
account gets a small one so the feature can be tried. Open question 2.

### API

`POST /compute/runs` takes `{snapshot, model_version, settings}`, where
`snapshot` is `finplan_plan::snapshot` JSON and `settings` is the subset of
`CreateRun` (iterations, seed, percentiles, converge). It returns
`202 {id}`.

- `model_version` must equal the server's `MODEL_VERSION`, else 409 "Reload
  FinPlan to update". The web bundle and the server deploy from the same tag,
  so a mismatch is a stale tab mid-deploy.
- The body is size-capped (2 MB). It is compiled with `finplan_plan::compile`
  and costed with the `create_run` formula against the user's remaining
  budget, then admitted through `admit_compute_observed` like any run.
- The job runs on the existing runner workers but writes no scenario and no
  `run_*` rows. A new `compute_jobs` table holds `{id, user_id, status,
  progress, cost, result_json, expires_at}`. The snapshot is held in memory
  only, never written.
- `GET /compute/runs/{id}` returns progress, then `RunResults` (the same
  projection the browser makes). `DELETE` cancels. Results expire one hour
  after completion; the maintenance loop purges them.
- Logs and metrics record user, cost, duration and outcome, never plan
  contents.

The browser stores the returned `RunResults` as a local run with
`engine: "server"`.

Analysis offload (sweeps, solves, sensitivity) follows the same shape
(`POST /compute/analyses`) in a later phase; it is where offload matters most,
since a sweep is hundreds of runs.

### Same seed, different machine

WASM's basic `f64` arithmetic is IEEE-exact, but transcendental functions
(`exp`, `ln`, `powf`, used by `rand_distr`) come from different libm
implementations natively and in WASM. More decisively, the spike found that
`rand`'s `SmallRng` is Xoshiro128++ on 32-bit targets and Xoshiro256++ on
64-bit ones, so the same seed draws a different stream altogether in WASM. So
the same seed produces different paths on the server and in the browser.
Consequences:

- Offloaded and local results agree statistically, not bit for bit. The
  equivalence test runs fixtures both ways and checks success rate and median
  final net worth agree within the Monte Carlo interval.
- Preview pairing (same markets for base and edit) holds only within one
  engine. When a base run's `engine` differs from where the preview runs, the
  preview re-simulates the base alongside the edit, which `api/preview.rs`
  already does when the base can't be reused.
- Making them bit-identical would mean naming the generator explicitly in
  core (e.g. `ChaCha8Rng`), which changes every seeded result the server has
  produced, and routing every transcendental through the `libm` crate,
  including inside `rand_distr`. Not worth it unless users notice.

## Durability

Browser storage can disappear, and losing a retirement plan is the worst bug
this product can have.

- Call `navigator.storage.persist()` on the first saved edit. Record the
  answer, and if it is denied, say so in the plan header.
- Safari deletes script-written storage for sites not visited in 7 days of
  browser use, unless the app is installed. Detect Safari and nudge
  "Add to Dock / Home Screen" (phase 5, which makes the app an installable
  PWA with a service worker).
- Track `last_exported_at`. After meaningful edits and 14 days since the last
  export, show "Back up this plan" (one click downloads the archive) next to
  "Move to cloud".
- Copy for an empty store on a returning visit (detected through a
  `localStorage` marker): "Your browser cleared this site's data. Import a
  backup file to restore your plan."

## Privacy and telemetry

The promise is only as good as the network panel. In local mode the app makes
no request that carries plan data. `PlanApi` is the only path that touches
plan data, and `localApi` does not use `fetch`. A test drives the local flows
with `fetch` stubbed to throw for anything but static assets and `/health`.

Error reports and any product analytics must not include plan contents, and
local mode sends none of them without opt-in (open question 4).

## Existing users and 17's guests

- Accounts keep their cloud plans exactly as they are. A new plan created
  while signed in asks where it should live; the default is a user preference
  (`default_plan_home`), local for everyone at first.
- Guests from 17: on their first visit after rollout, the web exports their
  plans (`GET /archives`), imports them into the local store, shows "Your plan
  now lives on this device", and logs the guest out. The guest purge from 17
  removes the server copy. Then `FINPLAN_GUEST_ACCESS` is turned off and,
  after one retention window, the guest code is deleted.
- Self-hosted: unchanged, plus local mode available. A self-hosted operator
  can turn off local mode (`FINPLAN_LOCAL_MODE=false`, served to the web via
  `/health` or entitlements) if they want plans on their server.
- TUI: unchanged.

## Phases

0. **Spike (go/no-go).** Build core and plan for wasm32, run fixtures in a
   worker, and record: throughput versus native, bundle size, cold-start
   time, memory at 10k iterations, and the native-versus-WASM result gap.
   Proceed if 1,000 iterations of a typical plan take under 3 s on a
   mid-range laptop and the bundle is ≤ 2 MB compressed.
1. **Local editing.** `finplan_wasm` (store side), the store worker,
   IndexedDB, `PlanApi` with `localApi` for the editing groups,
   compile/preflight, plan refs in nav, archive import and export. Behind a
   flag.
2. **Local runs.** Single compute worker, `RunResults` projection, results,
   ledger, preview and quick what-if on local plans.
3. **Parallel and estimates.** The batch API in core, N workers, calibration,
   the run-time estimate and cancel.
4. **Local analysis and review.** Sweeps, solves, sensitivity and goal seek
   locally; rule-based review notes (from `suggest/rules`, moved in 18
   phase 5).
5. **Plan homes and durability.** Move/download between homes, persistence
   request, backup reminders, PWA and offline.
6. **Server offload.** `/compute/runs`, quota, the "Run on FinPlan servers"
   offer; then `/compute/analyses`.
7. **Default flip.** No-account visitors get local mode; migrate and retire
   17's guests; new pricing page.

## Spike results (2026-10-03)

Machine: Apple Silicon laptop (macOS, Node 24 / V8 for WebAssembly, the same
engine Chrome and Edge use; Chrome 154 headless for the loading checks).
Plan: `crates/finplan_plan/testdata/default_snapshot.json`, the anonymized
default plan (4 investment accounts, 30 years, monthly events). The criterion
benches build their fixtures in Rust, so they cannot be fed to WASM as they
are; the default plan is the typical-plan measure. Native and WASM run the
identical code path: `coordinator_new` + `prepare` + `run_batch` per batch +
`coordinator_finish` (phase 2 and the projection), JSON in and out at every
step, one thread, batches of 100 in rounds of 4. Reproduce with
`cargo run --release -p finplan_wasm --example spike_native -- <snapshot>
<iterations> <seed> [out]` and `node scripts/wasm-spike.mjs <snapshot>
<iterations> <seed> [out]`.

| | Native (release, `opt-level = "s"`) | Native (`opt-level = 3`, fat LTO) | WASM (`wasm-release`) |
|---|---|---|---|
| 1,000 iterations, batches only | 0.87 s (1,150 it/s) | 0.85 s (1,200 it/s) | **1.35 s (745 it/s)** |
| 1,000 iterations, whole run incl. phase 2 and projection | 0.89 s | n/a | **1.38 s** |
| 10,000 iterations, batches only | 7.8 s (1,290 it/s) | n/a | 13.4 s (745 it/s) |

- **Throughput gap: 1.5x** single-threaded (native `opt-level = "s"` as the
  server ships it), inside the expected 1.5-3x. Phase 2 plus projection adds
  about 40 ms (20 ms native). Preparing the plan costs 10 ms.
- **Time for 1,000 iterations: 1.4 s against the 3 s criterion.** With the
  worker pool of phase 3 (batches run in parallel, one WASM instance each) the
  4-batch default plan runs in about a quarter of that on 4+ cores.
- **Bundle:** `finplan_wasm_bg.wasm` is 2.11 MB raw after `wasm-opt -Os`
  (2.29 MB before), **679 KB gzip, 474 KB brotli** against the 2 MB compressed
  budget. The generated JS glue is 34 KB. The size is serde_json plus the
  plan model and engine; `opt-level = "s"`, fat LTO and `panic = "abort"` are
  set in `[profile.wasm-release]`.
- **Cold start**, fetch-free, in Node: import the glue 0.5 ms, instantiate
  1.8-2.1 ms, first call (`library_seed`) 0.6 ms, about 3 ms in all, with 1.25
  MB of linear memory. In Chrome (headless, production build, worker, fetching
  the `.wasm` from the Next server and compiling it) fetch, compile and the
  first call took about 40 ms cold and about 11 ms warm.
- **Memory at 10,000 iterations** through the coordinator: WASM linear memory
  grew to **32.9 MB** (30.8 MB at 1,000). That is with the whole run as four
  batches of 2,500, whose JSON outputs (3.4 MB each, 13.7 MB together at 10k)
  are the bulk of it. The coordinator itself keeps 16 bytes per iteration.
  Well under the 256 MB-per-worker budget, so batch sizing is not yet a
  constraint; a 100-iteration batch is about 150 KB of JSON.
- **Native-versus-WASM result gap, same seed: not bit-identical, statistically
  equal.** At 1,000 iterations seed 42: success rate 0.895 native, 0.903 WASM;
  median final net worth 742.3 M native, 780.1 M WASM. At 10,000: success rate
  0.9056 and 0.9110 (difference 0.5 points, 1.3 combined standard errors),
  median 785.4 M and 789.9 M (0.6%), mean 1,765 M and 1,750 M. The gap is
  larger than libm last-bit differences alone would give, and the reason is
  not libm: `rand`'s `SmallRng` is a different generator on 32-bit targets
  (Xoshiro128++ on wasm32, Xoshiro256++ on 64-bit), and `seed_from_u64` and
  `usize` sampling differ with it. So a seed names a different stream per
  pointer width, which the "Same seed, different machine" section above did not
  account for. The consequences are the ones it lists (agreement within the
  Monte Carlo interval, not bit for bit; preview pairing within one engine).
  Making them identical means naming the generator explicitly in core
  (`rand_chacha::ChaCha8Rng`, already in the dependency tree, or
  `rand_xoshiro`) and sampling in `u64`, plus the `libm` routing the section
  mentions. That would also change every seeded result the server has ever
  produced, so it is not done here.

**Verdict: go.** 1,000 iterations take 1.4 s single-threaded (criterion 3 s),
the module is 474 KB brotli / 679 KB gzip (criterion 2 MB), cold start is tens
of milliseconds, and memory is small. The risks to carry forward: the
1.5x slowdown vs native (parallel workers more than cover it), and the RNG
difference above (an equivalence test must compare statistics, not bits).

Also learned, which phases 1-3 build on:

- **Loading under Next.js 16 / Turbopack works with no special config**, in
  `next dev` and in `next build && next start`, from a Web Worker created with
  `new Worker(new URL("./x.worker.ts", import.meta.url))`. All three of these
  load the `.wasm`, which Turbopack emits as a hashed asset under
  `.next/static/media/`: `await init()` (the generated default,
  `new URL("finplan_wasm_bg.wasm", import.meta.url)` inside the glue),
  `await init({ module_or_path: new URL("./pkg/finplan_wasm_bg.wasm",
  import.meta.url) })`, and `initSync({ module: await (await fetch(new
  URL("./pkg/finplan_wasm_bg.wasm", import.meta.url))).arrayBuffer() })`. The
  first or second is the one to use (they stream-compile); nothing needs to go
  in `public/`. `web/lib/engine/pkg` must exist for `tsc` and the bundler to
  resolve an import of it, which is why `predev`, `prebuild` and CI build it
  first. Under Node (the smoke test) `initSync({ module: readFileSync(...) })`.
- **`BatchOutput` and `BatchSpec` are opaque strings.** Iteration seeds are
  full 64-bit integers, which `JSON.parse` rounds; workers must pass them as
  strings (and join `BatchOutput`s into an array with `"[" + outputs.join(",")
  + "]"`), never parse and re-stringify. The run seed itself is limited to
  0..2^53-1 for the same reason (the browser draws 32 bits).
- A panic in the engine is a trap and the instance is unusable afterwards; the
  export layer throws it as an `EngineError` with code `panic`, and the worker
  should be restarted.
- A `RunResults` for the default plan is 7.8 MB of JSON (the percentile paths'
  ledgers dominate), so the store should keep it as one blob per run and the
  ledger reads (`ledger_page`) re-parse it; if that is too slow, the next
  step is a handle-based `results_open`.
- `finplan_wasm` differs from the sketch in "`finplan_wasm` crate" above in
  these ways: the run seed is a field of the settings (`CreateRun.seed`,
  required locally) rather than a separate argument; `apply_edit` and
  `apply_library` take an optional `now` to stamp `updated_at`, and
  `apply_library` returns the plans the change reached (`changed_plans`) so the
  store can write them; the coordinator exposes `coordinator_info` for progress
  denominators; reads are one `read(graph, library, query)` and
  `read_library(library, plans, query)` with tagged queries.

## Implementation notes (2026-10-03)

Phases 0–7 are built, with local mode behind `NEXT_PUBLIC_FINPLAN_LOCAL_MODE`
or `localStorage['finplan.localMode'] = '1'` (and not refused by the server's
`/health` `local_mode`). Nothing changes for anyone with the flag off; flipping
the default (phase 7's rollout) is a deploy decision, not code.

Where things live:

- Core: the batch API in `finplan_core::simulation` (`BatchSpec`,
  `BatchOutput`, `prepare_run`, `run_batch`, `MonteCarloCoordinator`,
  `merge_batches`); `monte_carlo_core` runs on top of it, bit-identical to
  before. `finplan_core::analysis::McRunner` lets sweeps and solves run on any
  runner.
- Plan crate: `read`, `preflight`, `archive`, `library` (`Library`,
  `LibraryOp`, `seed()`), `edit::EditOp`, `create`, `setup`, `run` (settings
  -> `MonteCarloConfig`, `run_cost`), `analysis`, `what_if`,
  `review::local_review`; `RunResults` is `Deserialize`; `PlanError` carries
  its HTTP status and code.
- `crates/finplan_wasm`: the bindings; `scripts/build-wasm.sh` writes
  `web/lib/engine/pkg/` (not committed; `pnpm dev`/`build` run it).
- Server: `/compute/runs` and `/compute/budget` (`api/compute*`,
  `runner/compute.rs`, migrations 0020–0021), `FINPLAN_LOCAL_MODE` on
  `/health`, `default_plan_home`, offload budgets `FINPLAN_OFFLOAD_BUDGET_FREE`
  (20M cost units/month) and `_PRO` (1B) — placeholders for open question 2.
- Web: `lib/api/{plan,remote,local}.ts` (the `PlanApi` seam), `lib/nav`
  (`l<id>` refs, `usePlanApi`, `usePlanCapabilities`), `lib/engine/` (store
  worker, IndexedDB, nested compute workers, coordinator, calibration, local
  backend), `lib/local/` (flag, runtime contract, durability, estimates,
  offload), `components/local/` (home badge, move/download/export dialogs).

Departures from the text above:

- Offload refuses every guest, and is capped by `FINPLAN_MAX_ITERATIONS`
  rather than the tier's iteration cap: the budget is the limit.
- A constrained device (≤ 2 cores, low memory) is offered the server only when
  the estimate is 10 s or more, not on every run. The "about N s" on the
  server is the local estimate over an assumed 8× speedup.
- `/health` is now JSON (`status`, `local_mode`, `model_version`).
- Local review shows rule-based notes and can apply them; Preview on a local
  note is hidden (no local re-simulation of a note's path yet).

Left to do:

- Local analyses run on one compute worker, and progress only moves between
  simulations: a default sweep (36 points × 250 iterations) took about 122 s
  with the bar at 0%. Farm analysis points across the pool.
- Browser-checked once in headless Chrome (create, edit, run, results, ledger,
  second tab, review, quick what-if). Export/import, cancel and converging
  runs in a browser, Safari, and the IndexedDB store under test are not yet
  covered (Node tests use the in-memory store).
- PWA icons are SVG only; some browsers want 192/512 PNGs before offering
  install. The service worker is untested beyond a production build.
- A guest whose plans were imported locally but whose tab closed before the
  logout is imported again on the next visit (no marker).
- No run-history badge for server-computed runs: the generated `Run` has no
  engine field.
- The cloud offload's `/compute/analyses` (sweeps on the server for local
  plans) is not built.

## Open questions

1. **Pricing.** With local runs unlimited, what does a Free *account* get?
   Proposal: one cloud plan (backup and a second device), a small offload
   budget and a small AI allowance. Pro: unlimited cloud plans, full AI, a
   larger offload budget.
2. **Offload budget sizes,** in cost units per month, for Free and Pro.
3. **AI on local plans.** v1 requires moving the plan to the cloud. A later
   option sends the snapshot per request with no storage, like offload. That
   needs `suggestions` and plan chat to stop assuming a stored scenario and
   run.
4. **Telemetry in local mode:** none, or opt-in error reports with plan data
   stripped?
5. **IndexedDB or OPFS** for the store? IndexedDB is enough for plan-sized
   JSON. OPFS only matters if ledgers or paths get large.
