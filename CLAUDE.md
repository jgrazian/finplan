# FinPlan - Monte Carlo Retirement Simulation

## Quick Commands

```bash
cargo build             # Build all crates
cargo run --bin finplan # Run the TUI
cargo run --bin finplan-server # Run the API server
cargo test              # Run all tests
cargo fmt               # Format code (REQUIRED before commits)
```

IMPORTIANT:
- When finished making changes run `cargo fmt`
- Run `cargo clippy` and fix any warnings if they will not cause major refactor work.
- `git add` changed files to track
- Suggest a commit message for the completed work

## Project Structure

```
finplan/
├── crates/
│   ├── finplan_core/   # Simulation engine library (~2500 LOC)
│   ├── finplan_plan/   # Plan model: graph, specs, edits, compile, results (no I/O, builds for wasm32)
│   ├── finplan/        # Terminal UI application (~2600 LOC)
│   └── finplan_server/ # HTTP API server, SQLite-backed; depends on finplan_plan
├── spec/               # Detailed specifications
├── scripts/            # gen-bindings.sh
└── web/                # Next.js frontend
    ├── lib/api/generated/  # ts-rs output — never edit, run gen-bindings.sh
    ├── lib/api/            # typed client over those bindings
    ├── lib/view/           # API shapes -> screen view models
    ├── lib/nav/            # the query string as app state (scenario/tab/row)
    └── components/         # presentational components
```

## TypeScript bindings

Every request/response type in `finplan_server` and `finplan_plan` derives
`ts_rs::TS`, and
`./scripts/gen-bindings.sh` writes one `.ts` file per type into
`web/lib/api/generated/`. The output is committed; after changing an API struct,
regenerate and commit, or the frontend types silently drift from the server.
Export settings (destination, `i64 -> number`) live in `.cargo/config.toml`.

ts-rs exports from a generated test. A plain `cargo test` writes them to the
scratch `target/ts-bindings/` (unformatted), never to `web/`; only
`gen-bindings.sh` writes the committed, formatted files. Running it and then
`git diff --exit-code web/lib/api/generated` is the drift check.

Only `web/lib/view/*` maps the generated shapes onto the screens' view models,
so a server-side change surfaces there as a type error rather than as a wrong
number on screen.

## Key Entry Points

| Task | Location |
|------|----------|
| Run simulation | `crates/finplan_core/src/simulation.rs:70` - `simulate()` |
| Monte Carlo | `crates/finplan_core/src/simulation.rs:474` - `monte_carlo_simulate_with_config()` |
| TUI entry | `crates/finplan/src/main.rs` |
| App event loop | `crates/finplan/src/app.rs:116` - `App::run()` |
| Server entry | `crates/finplan_server/src/main.rs` |
| Plan graph -> engine config | `crates/finplan_plan/src/compile/mod.rs` - `compile()` |
| Load a plan from SQLite | `crates/finplan_server/src/db/graph.rs` - `load()` |
| Summary -> results bodies | `crates/finplan_plan/src/results/mod.rs` - `project()` |
| Schema | `crates/finplan_server/migrations/0001_init.sql` |

## finplan_core Navigation

### Core Simulation
- `simulation.rs` - Main loop, Monte Carlo orchestration
- `simulation_state.rs` - Runtime state (accounts, timeline, taxes)
- `apply.rs` - Execute event effects
- `evaluate.rs` - Evaluate triggers and transfer amounts
- `liquidation.rs` - Asset sale with tax handling
- `taxes.rs` - Progressive tax calculations

### Data Model (`model/`)
- `accounts.rs` - Account, TaxStatus, AssetLot, InvestmentContainer
- `events.rs` - Event, EventTrigger, EventEffect, TransferAmount
- `market.rs` - ReturnProfile, InflationProfile
- `results.rs` - SimulationResult, MonteCarloSummary
- `ids.rs` - AccountId, AssetId, EventId, ReturnProfileId

### Builder DSL (`config/`)
- `builder.rs` - SimulationBuilder fluent API
- `account_builder.rs` - Preset accounts (Checking, 401k, Roth, etc.)
- `asset_builder.rs` - Asset definitions
- `event_builder.rs` - Event construction helpers

## finplan_plan Navigation

What a plan *is*, with no database. Must not depend on sqlx, tokio, axum or
anything doing I/O; CI builds it for `wasm32-unknown-unknown` with
`--no-default-features` (the `sqlx` feature only adds `FromRow` for the server).

- `graph.rs` - row structs and `ScenarioGraph` (the plan as loaded), `next_id`
- `specs/` - API request/response specs (ts-rs exported) and their validation
- `batch.rs` - `RowBatch`: specs lowered to rows; `merge_into` a graph (the server's SQL sink is `db::batch::insert`)
- `edit/` - every write route as an in-memory edit of a `ScenarioGraph` (preview); `edit::apply(graph, &EditOp)` is the single entry point
- `library.rs` - the user's return/inflation profiles and tax configs as a `Library` value: `attach` to a graph, `LibraryOp`/`apply_library`, the starter `seed()`
- `create.rs` - `new_plan` and `duplicate` (the twins of `POST /scenarios` and `/duplicate`)
- `compile/` - `ScenarioGraph` -> `SimulationConfig`
- `read.rs` - every plan-shaped GET body (accounts, assets, events, parameters, profiles, tax configs, compile report, preflight) built from a graph; the server's routes and the local store answer with it
- `preflight.rs`, `archive.rs` - the pre-run review; `PlanArchive` pack/unpack/preview and the import checks
- `snapshot.rs` - canonical input snapshot, its hash, `MODEL_VERSION`
- `results/` - `project()` a `MonteCarloSummary` into the results bodies; ledger, funding views
- `suggest/`, `templates/`, `rules/` - review-note changes, templates and rule-based notes
- `error.rs` - `PlanError` (the server converts it to `ApiError`)

Write routes in the server are "load the graph, call the plan crate, write
rows"; `domain/edit_route_tests.rs` holds each route equal to its edit.

## finplan (TUI) Navigation

### Screens (`screens/`)
- `portfolio_profiles.rs` - Accounts and return profiles
- `scenario.rs` - Simulation parameters, tax config
- `events.rs` - Life event management
- `results.rs` - Monte Carlo results display

### State (`state/`)
- `app_state.rs` - Root application state
- `modal.rs` - Modal dialog state
- `modal_action.rs` - Action dispatch enums

### Actions (`actions/`)
- `scenario.rs` - New, Load, Save, Duplicate, Delete
- `account.rs` - Account CRUD
- `event.rs` - Event configuration
- `effect.rs` - Event effect management

### Data (`data/`)
- `storage.rs` - File persistence (`~/.finplan/scenarios/`)
- `app_data.rs` - In-memory data structures

## Common Tasks

### Adding a new EventEffect
1. Add variant to `crates/finplan_core/src/model/events.rs` - `EventEffect` enum
2. Implement in `crates/finplan_core/src/apply.rs` - `apply_effect()`
3. Add evaluation in `crates/finplan_core/src/evaluate.rs` if needed
4. Add TUI support in `crates/finplan/src/actions/effect.rs`

### Adding a new EventTrigger
1. Add variant to `crates/finplan_core/src/model/events.rs` - `EventTrigger` enum
2. Implement evaluation in `crates/finplan_core/src/evaluate.rs` - `evaluate_trigger()`
3. Add TUI support in `crates/finplan/src/actions/event.rs`

### Adding a new Account type
1. Modify `crates/finplan_core/src/model/accounts.rs` - `AccountFlavor` enum
2. Update `Account::total_value()` and `Account::snapshot()`
3. Add builder preset in `crates/finplan_core/src/config/account_builder.rs`
4. Add TUI support in `crates/finplan/src/actions/account.rs`

## Testing

```bash
cargo test -p finplan_core           # Core library tests
cargo test -p finplan_core -- basic  # Specific test
cargo test -p finplan_server         # API integration tests
```

Key test files:
- `crates/finplan_core/src/tests/basic.rs` - Basic simulation tests
- `crates/finplan_core/src/tests/builder_dsl.rs` - Builder API tests
- `crates/finplan_core/src/tests/accounts.rs` - Account operation tests

## Specifications

Detailed documentation in `spec/`:
- `00_project_overview.md` - Architecture overview
- `01_core_architecture.md` - Engine module organization
- `02_data_model.md` - Core data structures
- `03_simulation_engine.md` - Simulation mechanics
- `04_tui_application.md` - TUI architecture
- `05_future_roadmap.md` - Planned features (optimization, what-ifs, estate planning)

## Code Style

- Run `cargo fmt` before commits (REQUIRED)
- Use type-safe IDs (AccountId, EventId, etc.)
- Record all state changes to the immutable ledger
- Prefer builder pattern for complex configurations
