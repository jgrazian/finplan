# finplan

A Monte Carlo financial planning simulation library written in Rust.

## Overview

`finplan` is a comprehensive financial planning tool that uses Monte Carlo simulations to model portfolio growth, cash flows, and financial events over time. It helps answer questions like "What are the chances my retirement savings will last?" by running thousands of simulations with varying market conditions.

## Features

- **Monte Carlo Simulation**: Run thousands of parallel simulations using Rayon for fast, multi-threaded execution
- **Flexible Return Profiles**: Model asset returns with Fixed, Normal, or LogNormal distributions
  - Includes S&P 500 historical presets
- **Inflation Modeling**: Account for inflation with configurable profiles
  - Includes US historical inflation presets
- **Multiple Account Types**: 
  - `Taxable` - Standard brokerage accounts
  - `TaxDeferred` - 401(k), Traditional IRA
  - `TaxFree` - Roth IRA
  - `Illiquid` - Real estate, vehicles
- **Asset Classes**:
  - `Investable` - Stocks, bonds, mutual funds
  - `RealEstate` - Property values
  - `Depreciating` - Cars, boats, equipment
  - `Liability` - Loans, mortgages
- **Flexible Cash Flows**:
  - Recurring contributions/withdrawals (weekly, bi-weekly, monthly, quarterly, yearly)
  - One-time transactions
  - Inflation-adjusted amounts
  - Annual and lifetime limits (e.g., 401k contribution limits)
  - Source/target account routing
- **Event-Driven Triggers**:
  - Date-based events
  - Balance threshold triggers (start withdrawals when account reaches X)

## Project Structure

```
finplan/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── finplan/            # Core simulation library
│   │   └── src/
│   │       ├── lib.rs      # Public API
│   │       ├── models.rs   # Data structures
│   │       ├── profiles.rs # Return & inflation profiles
│   │       └── simulation.rs # Monte Carlo engine
│   └── finplan_server/     # REST API server (Axum)
│       └── src/
│           └── main.rs     # HTTP endpoints
└── web/                    # Next.js frontend
    └── src/
        ├── app/            # Pages
        ├── components/     # React components
        └── types.ts        # TypeScript types
```

## Usage

### As a Library

```rust
use finplan::models::*;
use finplan::profiles::*;
use finplan::simulation::{simulate, monte_carlo_simulate};

let params = SimulationParameters {
    start_date: None, // Uses current date
    duration_years: 30,
    inflation_profile: InflationProfile::US_HISTORICAL_NORMAL,
    return_profiles: vec![ReturnProfile::SP_500_HISTORICAL_LOG_NORMAL],
    events: vec![],
    accounts: vec![Account {
        account_id: AccountId(1),
        account_type: AccountType::TaxDeferred,
        assets: vec![Asset {
            asset_id: AssetId(1),
            asset_class: AssetClass::Investable,
            initial_value: 100_000.0,
            return_profile_index: 0,
        }],
    }],
    cash_flows: vec![CashFlow {
        cash_flow_id: CashFlowId(1),
        amount: 500.0,
        start: Timepoint::Immediate,
        end: Timepoint::Never,
        repeats: RepeatInterval::Monthly,
        cash_flow_limits: None,
        adjust_for_inflation: true,
        source: CashFlowEndpoint::External,
        target: CashFlowEndpoint::Asset {
            account_id: AccountId(1),
            asset_id: AssetId(1),
        },
    }],
};

// Run a single simulation
let result = simulate(&params, 42);

// Run Monte Carlo simulation (100 iterations)
let mc_result = monte_carlo_simulate(&params, 100);
```

### Running the Server

```bash
cd crates/finplan_server
cargo run
```

The server exposes:
- `GET /` - Health check
- `POST /api/simulate` - Run simulation with JSON parameters

### Running the Web UI

```bash
cd web
pnpm install
pnpm dev
```

Then open http://localhost:3001 (the API server runs on port 3000).

## Key Types

| Type | Description |
|------|-------------|
| `SimulationParameters` | Complete configuration for a simulation run |
| `SimulationResult` | Results from a single simulation |
| `MonteCarloResult` | Aggregated results from multiple simulation runs |
| `Account` | A financial account containing one or more assets |
| `Asset` | An individual investment with its own return profile |
| `CashFlow` | Scheduled money movement (income, expenses, transfers) |
| `Event` | A trigger that can start/stop cash flows |
| `ReturnProfile` | Distribution for modeling investment returns |
| `InflationProfile` | Distribution for modeling inflation |

## Dependencies

- [jiff](https://docs.rs/jiff) - Date/time handling
- [rand](https://docs.rs/rand) / [rand_distr](https://docs.rs/rand_distr) - Random sampling
- [rayon](https://docs.rs/rayon) - Parallel iteration
- [serde](https://docs.rs/serde) - Serialization

## License

MIT
