# Local-first FinPlan: plans and engine in the browser

Status: proposed (2026-10-03). Third of three: [17](17_guest_access.md) guest
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
implementations natively and in WASM. So the same seed can produce slightly
different paths on the server and in the browser. Consequences:

- Offloaded and local results agree statistically, not bit for bit. The
  equivalence test runs fixtures both ways and checks success rate and median
  final net worth agree within the Monte Carlo interval.
- Preview pairing (same markets for base and edit) holds only within one
  engine. When a base run's `engine` differs from where the preview runs, the
  preview re-simulates the base alongside the edit, which `api/preview.rs`
  already does when the base can't be reused.
- Making them bit-identical would mean routing every transcendental through
  the `libm` crate, including inside `rand_distr`. Not worth it unless users
  notice.

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
