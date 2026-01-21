# finplan_core Architecture

## Module Organization

```
finplan_core/src/
├── lib.rs                 # Public API and re-exports
├── simulation.rs          # Main simulation loop
├── simulation_state.rs    # Runtime state management
├── apply.rs               # Event effect application
├── evaluate.rs            # Trigger/effect evaluation
├── liquidation.rs         # Asset sale with tax handling
├── taxes.rs               # Tax calculations
├── metrics.rs             # Performance instrumentation
├── error.rs               # Error types
├── config/                # Builder DSL
│   ├── mod.rs             # SimulationConfig
│   ├── builder.rs         # SimulationBuilder
│   ├── account_builder.rs # Account presets
│   ├── asset_builder.rs   # Asset definitions
│   ├── event_builder.rs   # Event construction
│   ├── metadata.rs        # SimulationMetadata
│   └── descriptors.rs     # Display helpers
└── model/                 # Core types
    ├── mod.rs             # Public exports
    ├── accounts.rs        # Account/Asset types
    ├── events.rs          # Event/Trigger/Effect
    ├── market.rs          # Returns/Inflation
    ├── ids.rs             # Type-safe IDs
    ├── records.rs         # Transaction records
    ├── results.rs         # Simulation output
    ├── state_event.rs     # Ledger entries
    ├── rmd.rs             # RMD tables
    └── tax_config.rs      # Tax configuration
```

## Module Responsibilities

### Core Simulation Loop

| Module | Responsibility |
|--------|----------------|
| `simulation.rs` | Day-by-day simulation, Monte Carlo orchestration |
| `simulation_state.rs` | Mutable runtime state (accounts, timeline, taxes) |
| `apply.rs` | Execute `EventEffect` against state |
| `evaluate.rs` | Evaluate triggers and compute transfer amounts |

### Financial Operations

| Module | Responsibility |
|--------|----------------|
| `liquidation.rs` | Lot selection, cost basis, capital gains |
| `taxes.rs` | Progressive tax brackets, marginal rates |

### Configuration

| Module | Responsibility |
|--------|----------------|
| `config/mod.rs` | `SimulationConfig` combining all parameters |
| `config/builder.rs` | Fluent builder pattern for setup |
| `config/*_builder.rs` | Domain-specific builders with presets |

### Type Definitions

| Module | Responsibility |
|--------|----------------|
| `model/accounts.rs` | Account flavors, tax status, lots |
| `model/events.rs` | Triggers, effects, withdrawal strategies |
| `model/market.rs` | Return profiles, inflation modeling |
| `model/results.rs` | Simulation output, Monte Carlo stats |

## Public API

The library exposes a minimal public API through `lib.rs`:

```rust
// Builder DSL
pub use config::{
    AccountBuilder, AssetBuilder, EventBuilder,
    SimulationBuilder, SimulationMetadata,
};

// Core modules available as `finplan_core::module`
pub mod apply;
pub mod config;
pub mod error;
pub mod evaluate;
pub mod liquidation;
pub mod metrics;
pub mod model;
pub mod simulation;
pub mod simulation_state;
pub mod taxes;
```

## Key Entry Points

### Single Simulation

```rust
// simulation.rs:70
pub fn simulate(
    params: &SimulationConfig,
    seed: u64
) -> Result<SimulationResult, MarketError>
```

### Monte Carlo (Memory Efficient)

```rust
// simulation.rs:474
pub fn monte_carlo_simulate_with_config(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
) -> Result<MonteCarloSummary, MarketError>
```

### With Instrumentation

```rust
// simulation.rs:132
pub fn simulate_with_metrics(
    params: &SimulationConfig,
    seed: u64,
    config: &InstrumentationConfig,
) -> Result<(SimulationResult, SimulationMetrics), MarketError>
```

## Design Patterns

### Type-Safe IDs

All domain entities use newtype IDs (`model/ids.rs`):

```rust
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct AccountId(pub &'static str);
pub struct AssetId(pub &'static str);
pub struct EventId(pub &'static str);
pub struct ReturnProfileId(pub &'static str);
```

### Immutable Ledger

All state changes are recorded to an append-only ledger:

```rust
// state_event.rs
pub struct LedgerEntry {
    pub date: Date,
    pub event: StateEvent,
}
```

### Builder DSL

Ergonomic configuration through fluent builders:

```rust
let (config, metadata) = SimulationBuilder::new()
    .start(2025, 1, 1)
    .years(30)
    .birth_date(1980, 6, 15)
    .account(AccountBuilder::taxable_brokerage("Brokerage").cash(50_000.0))
    .build();
```

## Error Handling

Errors are defined in `error.rs` with specific variants:

- `LookupError` - Entity not found
- `AccountTypeError` - Invalid operation for account type
- `MarketError` - Market data issues
- `TransferEvaluationError` - Failed to compute transfer amount
- `TriggerEventError` - Event trigger evaluation failed

## Dependencies

### Required
- `jiff` - Date/time handling
- `serde` - Serialization
- `rand`, `rand_distr` - Randomness for Monte Carlo
- `rayon` - Parallel iteration
- `rustc-hash` - Fast HashMap implementation

### Optional
- `ts-rs` (feature: `ts`) - TypeScript type generation
- `criterion` - Benchmarking
