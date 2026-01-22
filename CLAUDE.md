# FinPlan - Monte Carlo Retirement Simulation

## Quick Commands

```bash
cargo build             # Build all crates
cargo run --bin finplan # Run the TUI
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
│   └── finplan/        # Terminal UI application (~2600 LOC)
├── spec/               # Detailed specifications
└── web/                # Next.js frontend (not actively developed)
```

## Key Entry Points

| Task | Location |
|------|----------|
| Run simulation | `crates/finplan_core/src/simulation.rs:70` - `simulate()` |
| Monte Carlo | `crates/finplan_core/src/simulation.rs:474` - `monte_carlo_simulate_with_config()` |
| TUI entry | `crates/finplan/src/main.rs` |
| App event loop | `crates/finplan/src/app.rs:116` - `App::run()` |

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
