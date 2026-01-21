# FinPlan Project Overview

## Purpose

FinPlan is a Monte Carlo retirement planning simulation system. It helps users model their financial future by simulating thousands of possible market scenarios, tax implications, and life events to answer questions like:

- What's the probability my savings will last through retirement?
- How does retiring at 62 vs 65 affect my outcomes?
- What's the optimal withdrawal strategy given my account mix?
- How do different market conditions affect my plan?

## Architecture

```
finplan/
├── crates/
│   ├── finplan_core/     # Simulation engine (library)
│   └── finplan/          # Terminal UI application
├── spec/                 # Specification documents
└── web/                  # Next.js frontend (not actively developed)
```

### Component Overview

| Component | Purpose | Status |
|-----------|---------|--------|
| `finplan_core` | Monte Carlo simulation engine | Active |
| `finplan` (TUI) | Interactive terminal application | Active |
| `finplan_server` | REST API backend | Disabled |
| `web` | Next.js frontend | Not actively developed |

## Technology Stack

- **Language**: Rust (Edition 2024)
- **Parallelization**: Rayon for Monte Carlo iterations
- **Date/Time**: Jiff library
- **Serialization**: Serde with YAML (serde_saphyr)
- **TUI Framework**: Ratatui + Crossterm
- **Error Handling**: color-eyre

## Key Concepts

### Simulation Flow

1. **Configuration**: Define accounts, assets, events, and parameters
2. **Execution**: Day-by-day simulation with event triggers
3. **Results**: Wealth snapshots, tax records, and statistics

### Account Types

| Tax Status | Examples | Tax Treatment |
|------------|----------|---------------|
| Taxable | Brokerage | Capital gains taxed |
| Tax-Deferred | 401k, Traditional IRA | Withdrawals taxed as income |
| Tax-Free | Roth IRA, Roth 401k | Qualified withdrawals tax-free |

### Event System

Events drive all state changes in the simulation:

- **Triggers**: Date, Age, Balance thresholds, Repeating schedules
- **Effects**: Income, Expenses, Asset sales, Contributions, RMDs

## Data Storage

The TUI stores scenarios in `~/.finplan/`:

```
~/.finplan/
├── config.yaml           # Active scenario, preferences
├── summaries.yaml        # Cached Monte Carlo results
└── scenarios/
    ├── retirement.yaml
    ├── aggressive.yaml
    └── conservative.yaml
```

## Building & Running

```bash
# Build everything
cargo build --release

# Run the TUI
cargo run --bin finplan

# Run tests
cargo test

# Format code (required before commits)
cargo fmt
```

## Related Documentation

- [01_core_architecture.md](01_core_architecture.md) - Engine module organization
- [02_data_model.md](02_data_model.md) - Core data structures
- [03_simulation_engine.md](03_simulation_engine.md) - How simulations work
- [04_tui_application.md](04_tui_application.md) - Terminal UI structure
- [05_future_roadmap.md](05_future_roadmap.md) - Planned improvements
