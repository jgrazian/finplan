# finplan_server

HTTP API server for FinPlan. Owns user accounts, scenario configuration, and
Monte Carlo execution, backed by SQLite.

```bash
cargo run --bin finplan-server              # serve on 127.0.0.1:8080
cargo run --bin finplan-server -- migrate   # apply migrations and exit
cargo test -p finplan_server                # integration tests
```

## Rebuilding a pre-v0 database

The v0 schema is a consolidated baseline. A database carrying the development
migration chain must be rebuilt before it is opened by a build containing that
baseline. Stop every server using the source database, then create a separate
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
| `FINPLAN_MAX_ITERATIONS` | `50000` | Per-run iteration cap |
| `FINPLAN_SECURE_COOKIES` | `false` | Set `Secure` on session cookies (enable behind TLS) |
| `FINPLAN_CORS_ORIGINS` | `http://localhost:3000` | Comma-separated allowed origins |

## Layering

```
api/       axum handlers; JSON in, JSON out
domain/    cross-table operations (scenario deep-clone)
compile/   stored rows  ->  finplan_core::SimulationConfig
runner/    background execution and result persistence
```

## The database is not a serialized config

`finplan_core` identifies entities with dense `u16` indices — `AccountId`,
`AssetId`, `EventId`, `ReturnProfileId` — because `Market` indexes straight into
`Vec`s with them. Those are *simulation-local indices*, valid only for one
`SimulationConfig`, and they are deliberately never persisted.

The database uses stable primary keys instead. `compile::IdMap` is the seam: it
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
  `api::specs` flattens on write and rebuilds on read.
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
inflation profiles, and the 2024 US federal brackets — because a scenario cannot
reference a profile that does not exist. They are ordinary rows and can be
edited or deleted.
