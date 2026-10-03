# finplan_server

HTTP API server for FinPlan. Owns user accounts, scenario configuration, and
Monte Carlo execution, backed by SQLite.

```bash
cargo run --bin finplan-server              # serve on 127.0.0.1:8080
cargo run --bin finplan-server -- migrate   # apply migrations and exit
cargo test -p finplan_server                # integration tests
```

## Rebuilding a pre-v0 database

The v0 schema is a single consolidated `migrations/0001_init.sql` baseline,
including parameters, real estate, what-if stacks, result quartiles, bracket
filling, and scenario slugs. Fresh databases apply only this migration.

A database carrying the development migration chain must be rebuilt before it is opened by a build containing that
baseline. The rebuild requires a source with the same application schema;
apply the full development chain with the previous build first if needed.
Stop every server using the source database, then create a separate
verified database:

```bash
cargo run --bin finplan-server -- rebuild-database \
  --source finplan.db --destination finplan-v0.db
```

The command never alters the source or overwrites an existing destination. It
creates the new schema, copies application tables by column name, retains cash
flows and ledger rows only for each scenario's newest successful run, and runs
row-count, foreign-key, and SQLite integrity checks before publishing the
destination. After inspecting the result, keep the original as a backup and
move `finplan-v0.db` into its place. The rebuilt database records only the
consolidated baseline in `_sqlx_migrations`.

Configuration comes from flags or environment variables:

| Variable | Default | Purpose |
|---|---|---|
| `FINPLAN_BIND` | `127.0.0.1:8080` | Listen address |
| `DATABASE_URL` | `sqlite://finplan.db` | SQLite database |
| `FINPLAN_DB_POOL` | `8` | Pool size |
| `FINPLAN_SIM_WORKERS` | `2` | Concurrent Monte Carlo runs |
| `FINPLAN_MAX_ITERATIONS` | `50000` | Per-run iteration cap (also the ceiling for offloaded runs) |
| `FINPLAN_LOCAL_MODE` | `true` | Plans live in the browser and the engine runs there (spec 19); served as `local_mode` on `/api/health`. Set `false` on a self-hosted server that should keep plans itself |
| `FINPLAN_OFFLOAD_BUDGET_FREE` | `20000000` | Cost units a Free account may offload per calendar month (UTC) |
| `FINPLAN_OFFLOAD_BUDGET_PRO` | `1000000000` | Cost units a Pro account may offload per calendar month (UTC). Self-hosted and beta accounts count as Pro |
| `FINPLAN_SECURE_COOKIES` | `false` | Set `Secure` on session cookies (enable behind TLS) |
| `FINPLAN_CORS_ORIGINS` | `http://localhost:3000` | Comma-separated allowed origins |
| `FINPLAN_LOG_FORMAT` | `auto` | `json`, `text`, or `auto` (JSON when hosted, text locally) |
| `FINPLAN_METRICS_BIND` | unset | Optional private metrics listener, e.g. `127.0.0.1:9090` |
| `RUST_LOG` | application INFO, dependencies WARN | Logging filter |

## Logs and metrics

To enable JSON logs and a local Prometheus scrape endpoint:

```bash
FINPLAN_LOG_FORMAT=json FINPLAN_METRICS_BIND=127.0.0.1:9090 cargo run --bin finplan-server
curl http://127.0.0.1:9090/metrics
```

`/metrics` uses a separate listener and is absent from the public API router.
It does not use browser authentication. Keep the listener on a private network
or behind an authenticated proxy when binding beyond loopback. Leaving the
setting unset disables the listener; application logging remains enabled.

Activity logs record committed account/scenario changes, authentication actions,
and run/analysis lifecycle events using internal IDs. The server generates an
`X-Request-ID` for each response and carries that ID into background job logs.
Logs omit financial values, names, credentials, request bodies, and query
strings. Collect process output with the deployment's log collector. These are
operational logs, not a transactional audit journal.

Metrics cover HTTP requests, errors, admission limits, queued/running jobs,
queue wait, engine and persistence time, processing duration, and run throughput.
Queue depth includes persisted recovery backlog and refreshes every five
seconds; `finplan_queue_snapshot_timestamp_seconds` exposes sampler freshness.
Histograms support averages and estimated p99. Processing-duration and run-speed
minimum/maximum gauges use a five-minute window at one-second resolution and
report NaN when that window has no observations. Iteration throughput counts
actual samples from successfully persisted runs at completion, so long runs
produce completion-sized bursts. Counters reset when the process restarts.

The [observability plan](../../spec/11_server_observability_plan.md) documents
metric names, timing boundaries, and example PromQL queries. Dashboard and alert
deployment are separate from the server instrumentation.

## Layering

```
api/       axum handlers; JSON in, JSON out
domain/    cross-table operations (scenario deep-clone)
runner/    background execution and result persistence
```

## The database is not a serialized config

`finplan_core` identifies entities with dense `u16` indices — `AccountId`,
`AssetId`, `EventId`, `ReturnProfileId` — because `Market` indexes straight into
`Vec`s with them. Those are *simulation-local indices*, valid only for one
`SimulationConfig`, and they are deliberately never persisted.

The database uses stable primary keys instead. `finplan_plan::compile::IdMap` is the seam: it
interns database ids into a gapless `0..n` range at compile time and keeps the
reverse direction so engine output can be attributed back to real rows. This is
what lets an account be renamed, reordered, or deleted without invalidating
anything, and what keeps a scenario's dense ids deterministic (assignment
follows `sort_order, id`) so a seeded run is reproducible.

Three schema decisions follow from modelling the domain rather than the engine's
memory layout:

- **Account flavors are class-table inheritance.** One row in `accounts` plus
  exactly one row in `account_bank` / `account_investment` / `account_property` /
  `account_liability`. Each detail table constrains only the columns its flavor
  actually has, instead of one wide table of mostly-NULL fields.
- **The recursive enums live in self-referential tables.** `EventTrigger`,
  `TransferAmount` and `EventEffect` are recursive in Rust, so `triggers`,
  `transfer_amounts` and `effects` carry parent links, with `CHECK` constraints
  asserting the columns each variant requires. The API still speaks nested JSON;
  `finplan_plan::specs` flattens on write and rebuilds on read.
- **Results are decomposed, not blobbed.** A finished run writes normalized rows
  — `run_stats`, `run_net_worth_points`, `run_account_points`, `run_cash_flows`,
  `run_taxes`, `run_warnings` — so the UI can query one percentile band or one
  account's series without loading a whole `MonteCarloSummary`. Percentile paths
  carry their percentile; the mean path is stored with `percentile IS NULL`.

## Runs are asynchronous

`POST /scenarios/{id}/runs` compiles the scenario synchronously (so a broken
plan fails immediately with a useful message), inserts a `queued` row, and
returns `202` with a run id. A worker pool picks it up, executes on a blocking
thread, and mirrors progress into the row every 400ms.

Because run state lives in SQLite rather than in memory, a crash mid-run is
recoverable: anything left `running` at boot is re-queued by
`runner::requeue_orphans`.

```
POST /api/scenarios/1/runs   -> 202 {"id":7,"status":"queued"}
GET  /api/runs/7             ->     {"status":"running","completed_iterations":340}
GET  /api/runs/7/results     ->     {stats, bands, account_series, cash_flows, warnings}
POST /api/runs/7/cancel      ->     {"status":"canceled"}
```

`GET /runs/{id}/results` defaults to the median path for its per-account series
and cash flows; pass `?series=mean` or `?series=0.95` to select another.

## Server offload

A plan kept in the browser can send a run here when the device is too slow
(spec 19). The plan leaves the device for this, so it is an explicit per-run
action, and the server keeps none of it.

```
POST   /api/compute/runs      {snapshot, model_version, settings}  -> 202 {"id":7}
GET    /api/compute/runs/7    -> {id, status, completed_iterations, iterations, seed,
                                  error, expires_at, results?}
DELETE /api/compute/runs/7    -> 204 (cancels and deletes)
GET    /api/compute/budget    -> {monthly, used, remaining, resets_at, available,
                                  unavailable_reason}
```

`snapshot` is `finplan_plan::snapshot::snapshot` output (a `ScenarioGraph`, as
an object or as its JSON text). `settings` is the run subset of `CreateRun`:
`iterations`, `percentiles`, `seed`, `converge`. The body is capped at 2 MB
(413), `model_version` must equal the server's (409 "Reload FinPlan to
update"), and a guest is refused (403): an account is needed to charge a budget
to. The job id is the `compute_jobs` row id; every route is owner-only (404
otherwise).

The snapshot is compiled in the request and handed to a runner worker in memory
(offload shares the `FINPLAN_SIM_WORKERS` slots with stored runs); it is never
written to the database or a log. No scenario and no `run_*` row is created.
`compute_jobs` holds status, progress, the cost charged and, on success, the
`RunResults` JSON from the same `finplan_plan::results::project` a stored run
persists, so a seeded offload and a stored run of the same graph agree exactly.
Results expire an hour after the job ends and the hourly maintenance loop
purges them. A job still open when the process restarts is failed and refunded.

Cost is `iterations x duration_years x max(accounts + assets + events, 1)` (a
converging run is costed at its ceiling), the formula `create_run` uses, and is
charged to `monthly_offload_spend` at admission. A 403 with code
`offload_budget_spent` says when it resets; `offload_run_exceeds_budget` says
the run is bigger than the whole month. A job canceled before it started, or
failed by the server, is refunded; one that ran and was canceled or whose
results were deleted is not. Logs and metrics carry user, cost, duration and
outcome (`finplan_offload_cost_units_total`, and the `kind="offload"` job
series), never plan contents.

`GET /api/health` answers `{"status":"ok","local_mode":true,"model_version":"..."}`.

## Endpoints

Authentication is an Argon2id password hash plus an opaque session token, stored
as a SHA-256 and delivered in an `HttpOnly` cookie. An `Authorization: Bearer`
header is accepted for non-browser clients.

```
POST   /api/auth/register|login|logout          GET /api/auth/me

GET    /api/scenarios                           POST   /api/scenarios
GET    /api/scenarios/{id}                      PATCH  /api/scenarios/{id}
DELETE /api/scenarios/{id}
POST   /api/scenarios/{id}/duplicate            POST   /api/scenarios/{id}/compile

       /api/scenarios/{id}/assets   [GET POST]  /api/scenarios/{id}/assets/{id}   [GET PATCH DELETE]
       /api/scenarios/{id}/accounts [GET POST]  /api/scenarios/{id}/accounts/{id} [GET PATCH DELETE]
       /api/scenarios/{id}/accounts/{id}/positions [GET POST]  .../{id} [DELETE]
       /api/scenarios/{id}/events   [GET POST]  /api/scenarios/{id}/events/{id}   [GET PUT DELETE]

       /api/return-profiles    [GET POST]  /api/return-profiles/{id}    [GET PATCH DELETE]
       /api/inflation-profiles [GET POST]  /api/inflation-profiles/{id} [DELETE]
       /api/tax-configs        [GET POST]  /api/tax-configs/{id}        [GET PATCH DELETE]
GET    /api/history-presets                     GET /api/health
```

Events use `PUT`, not `PATCH`: a trigger and its effect list form a tree, and
merging a partial tree into an existing one has no sensible semantics, so the
old tree is deleted and rewritten in one transaction.

Registration seeds the new user a starter library — eight return profiles, two
inflation profiles, and the 2024 US federal brackets with their standard
deduction — because a scenario cannot reference a profile that does not exist.
They are ordinary rows and can be edited or deleted.
