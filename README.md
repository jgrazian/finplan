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

## Keyboard Shortcuts

The TUI uses vim-style navigation by default:

| Key | Action |
|-----|--------|
| `j/k` | Navigate up/down |
| `Tab` | Switch panels |
| `1-5` | Switch tabs |
| `a/e/d` | Add/Edit/Delete |
| `Ctrl+S` | Save |
| `q` | Quit |

### Custom Keybindings

You can customize keyboard shortcuts by creating `~/.finplan/keybindings.yaml`:

```yaml
# Global keybindings (work everywhere)
global:
  quit: ["q", "ctrl+c"]
  save: ["ctrl+s"]
  cancel: ["esc"]
  tab_1: ["1"]
  tab_2: ["2"]
  tab_3: ["3"]
  tab_4: ["4"]
  tab_5: ["5"]

# Navigation (consistent across all panels)
navigation:
  up: ["k", "up"]
  down: ["j", "down"]
  left: ["h", "left"]
  right: ["l", "right"]
  next_panel: ["tab"]
  prev_panel: ["shift+tab"]
  reorder_up: ["shift+k", "shift+up"]
  reorder_down: ["shift+j", "shift+down"]
  confirm: ["enter"]

# Tab-specific bindings
tabs:
  events:
    add: ["a"]
    edit: ["e"]
    delete: ["d"]
    copy: ["c"]
    toggle: ["t"]
    effects: ["f"]

  scenario:
    run: ["r"]
    monte_carlo: ["m"]
    run_all: ["shift+r"]
    new: ["n"]
    copy: ["c"]
```

Key format: `[modifier+]key` where modifier is `ctrl`, `shift`, or `alt`.

Examples: `"a"`, `"ctrl+s"`, `"shift+j"`, `"enter"`, `"f1"`

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
