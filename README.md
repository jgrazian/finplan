# FinPlan

A Monte Carlo retirement planning simulator with an interactive terminal UI.

![FinPlan TUI Demo](finplan.gif)

## What It Does

FinPlan runs thousands of simulations with varying market conditions to answer questions like:

- What's the probability my savings will last through retirement?
- How does retiring at 62 vs 65 affect my outcomes?
- What's the optimal withdrawal strategy given my account mix?

## Building & Running

Requires Rust (stable).

```bash
# Build the project
cargo build --release

# Run the terminal UI
cargo run --bin finplan

# Run tests
cargo test
```

Scenarios are saved to `~/.finplan/scenarios/`.

## Features

**Account Types**
- Taxable (brokerage)
- Tax-Deferred (401k, Traditional IRA)
- Tax-Free (Roth IRA)
- Illiquid (real estate, vehicles)

**Asset Classes**
- Investable (stocks, bonds, funds)
- Real estate
- Depreciating assets
- Liabilities (loans, mortgages)

**Event System**
- Date and age-based triggers
- Recurring income/expenses
- One-time transactions
- Inflation-adjusted amounts
- Account balance thresholds

**Return Modeling**
- Fixed, Normal, or LogNormal distributions
- Historical S&P 500 presets
- Configurable inflation profiles

## Project Structure

```
finplan/
├── crates/
│   ├── finplan_core/   # Simulation engine library
│   └── finplan/        # Terminal UI application
└── spec/               # Detailed specifications
```

## Using as a Library

```rust
use finplan_core::SimulationBuilder;

let result = SimulationBuilder::new()
    .start_date("2025-01-01")
    .duration_years(30)
    .checking("Checking", 10_000.0)
    .traditional_401k("401k", 500_000.0)
    .roth_ira("Roth IRA", 100_000.0)
    .income("Salary", 8_000.0)
        .monthly()
        .to_account("Checking")
        .done()
    .expense("Living Expenses", 5_000.0)
        .monthly()
        .from_account("Checking")
        .done()
    .monte_carlo(1000);
```

## License

MIT
