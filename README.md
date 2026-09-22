# FinPlan

A Monte Carlo retirement planning simulator with an interactive terminal UI.

Unlike simple retirement calculators, FinPlan models tax-aware withdrawals, RSU vesting, and multiple account types, then runs full Monte Carlo simulations to show you the probability of your plan succeeding. Everything runs locally — no account required, no data leaves your machine.

![FinPlan TUI Demo](finplan.gif)

## What It Does

FinPlan runs thousands of simulations with varying market conditions to answer questions like:

- What's the probability my savings will last through retirement?
- How does retiring at 62 vs 65 affect my outcomes?
- What's the optimal withdrawal strategy given my account mix?

## Installation

Requires [Rust](https://rustup.rs/) (stable).

```bash
# Install from source
cargo install --path crates/finplan

# Or build and run directly
cargo run --bin finplan --release
```

Scenarios are saved to `~/.finplan/scenarios/`.

## Quick Start

An example scenario is included in [`examples/example.yaml`](examples/example.yaml). To load it:

1. Run `finplan` and navigate to the **Scenario** tab (`3`)
2. Press `i` to import a YAML file
3. Enter the path to `examples/example.yaml`

## Events and Effects

An event pairs a **trigger** (when it happens) with **effects** (what it does).
For example, a monthly salary event uses a repeating monthly trigger and an Income
effect that deposits money into your checking account. Other effects include
expenses, account transfers, asset purchases and sales, and RSU vesting. An event
can have multiple effects; an event with no effects performs no actions when it
triggers.

To add effects using the default shortcuts:

1. Open the **Events** tab (`2`) and press `a` to create an event and configure its trigger.
2. Select the event and press `f` to open **Manage Effects**.
3. Choose **Add New Effect**, select an effect type, and configure its accounts and amount.
4. Press `f` again whenever you want to add another effect or select an existing effect to edit or delete it.

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

## Keyboard Shortcuts

The TUI uses vim-style navigation by default:

| Key | Action |
|-----|--------|
| `j/k` | Navigate up/down |
| `Tab` | Switch panels |
| `1-5` | Switch tabs |
| `a/e/d` | Add/Edit/Delete |
| `f` (Events tab) | Manage effects for the selected event (add, edit, or delete) |
| `Ctrl+S` | Save |
| `q` | Quit |

All keybindings are customizable via `~/.finplan/keybindings.yaml`. Key format is `[modifier+]key` where modifier is `ctrl`, `shift`, or `alt`.

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

// result.stats.success_rate          => 0.87 (87% of runs ended solvent)
// result.stats.percentile_values     => [(0.05, 120_000), (0.50, 850_000), (0.95, 2_100_000)]
// result.stats.mean_final_net_worth  => 920_000
```

## Project Structure

```
finplan/
├── crates/
│   ├── finplan_core/   # Simulation engine library
│   └── finplan/        # Terminal UI application
└── spec/               # Detailed specifications
```

## License

MIT
