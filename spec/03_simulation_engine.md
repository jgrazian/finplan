# Simulation Engine

## Overview

The simulation engine runs day-by-day through a financial timeline, triggering events when conditions are met and recording all state changes to an immutable ledger.

## Main Simulation Loop

Located in `simulation.rs:70`:

```rust
pub fn simulate(params: &SimulationConfig, seed: u64) -> Result<SimulationResult, MarketError>
```

### Algorithm

```
1. Initialize state from SimulationConfig
2. Snapshot initial wealth

3. While current_date < end_date:
   a. Inner loop (same-day events):
      - Process all triggered events
      - Apply effects to state
      - Record changes to ledger
      - Repeat until no events trigger (max 1000 iterations for safety)
      - On December 31, once its events have run: capture year-end
        balances (RMD base) and the year's wealth snapshot

   b. Advance time:
      - Find next checkpoint (event date, quarter, year-end)
      - Apply interest/returns for elapsed days
      - Record appreciation to ledger: cash interest, and (with the ledger
        on) each account's holdings repriced
      - Reset contribution limits on boundaries

4. Finalize last year's taxes
5. Return SimulationResult
```

### Time Advancement

The engine advances to the next "checkpoint" rather than day-by-day:

- Next event trigger date
- Quarterly heartbeat (at least every 3 months)
- December 31 (for RMD year-end balance capture)
- Simulation end date

## Event Processing

### Trigger Evaluation (`evaluate.rs`)

Triggers are evaluated recursively:

```rust
fn evaluate_trigger(trigger: &EventTrigger, state: &SimulationState) -> bool
```

| Trigger Type | Evaluation |
|--------------|------------|
| `Date(d)` | `current_date >= d` |
| `Age { years, months }` | Calculate age from birth_date |
| `AccountBalance` | Compare against threshold |
| `Repeating` | Check interval + start/end conditions |
| `And(triggers)` | All must be true |
| `Or(triggers)` | Any must be true |

### Effect Application (`apply.rs`)

Effects are applied in order, each producing ledger entries:

```rust
fn apply_effect(effect: &EventEffect, state: &mut SimulationState) -> Vec<LedgerEntry>
```

### Effect Processing Order

For complex effects like `Sweep`:

1. Calculate required gross amount (if net mode)
2. Liquidate assets from sources in order
3. Calculate capital gains and taxes
4. Record tax withholding
5. Transfer net proceeds to destination
6. Update yearly tax tracking

## Asset Liquidation (`liquidation.rs`)

### Lot Selection

When selling assets, lots are selected based on `LotMethod`:

| Method | Selection Strategy |
|--------|-------------------|
| `Fifo` | Oldest lots first |
| `Lifo` | Newest lots first |
| `HighestCost` | Highest cost basis first (minimize gains) |
| `LowestCost` | Lowest cost basis first (realize gains early) |
| `AverageCost` | Use average cost basis |

### Capital Gains

- **Short-term**: Held < 1 year, taxed as ordinary income. Short-term gains
  count toward the year's ordinary income, so later income that year is taxed
  from where they left it.
- **Long-term**: Held >= 1 year, taxed in `capital_gains_brackets` (0% / 15% /
  20% in law), stacked on top of the year's ordinary income and earlier
  long-term gains. The schedule is over taxable income: the engine moves it by
  the standard deduction and indexes it to inflation with the federal
  brackets, so deduction that ordinary income leaves unused shelters gains.
- **Net investment income tax**: with a `niit_threshold`, 3.8% of the lesser of
  the year's gains and how far ordinary income plus long-term gains is over the
  threshold. Not indexed.
- **Year to date**: every sale reads the year's income as applied so far plus
  the tax events already evaluated in the same effect (other assets, other
  accounts of a sweep), so a multi-asset or multi-account sale is taxed
  dollar after dollar.
- **Net amounts**: a sale asked for a net amount grosses up exactly: the
  ordinary brackets inverted for a tax-deferred account, a solve against the
  lots the sale consumes for a taxable one, the net itself for a tax-free one.
- **Not modeled**: capital losses. A lot sold at a loss realizes no gain, but
  the loss offsets nothing and does not carry forward.

The liquidation module tracks:
- Proceeds from sale
- Cost basis of sold lots
- Capital gain/loss
- Tax liability
- Net amount after taxes

## Tax Calculations (`taxes.rs`)

### Progressive Tax

```rust
pub fn calculate_progressive_tax(income: f64, brackets: &[TaxBracket]) -> f64
```

### Marginal Rate

```rust
pub fn marginal_rate(income: f64, brackets: &[TaxBracket]) -> f64
```

### Gross from Net

For `AmountMode::Net`, calculates gross needed to achieve target net:

```rust
pub fn gross_from_net(net: f64, marginal_rate: f64) -> f64
```

## Monte Carlo Simulation

### Memory-Efficient Implementation

Located in `simulation.rs:474`:

```rust
pub fn monte_carlo_simulate_with_config(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
) -> Result<MonteCarloSummary, MarketError>
```

### Two-Phase Approach

**Phase 1**: Run all iterations, keeping only (seed, final_net_worth)
- O(N) memory for seeds/values instead of O(N * result_size)
- Accumulates mean statistics incrementally

**Phase 2**: Re-run only seeds needed for percentile results
- Typically 5-10 re-runs for common percentiles (10th, 25th, 50th, 75th, 90th)

### Parallelization

Uses Rayon for parallel iteration:

```rust
let seed_results: Vec<(u64, f64)> = (0..num_batches)
    .into_par_iter()
    .flat_map(|batch_idx| { ... })
    .collect();
```

Batch size: 100 iterations per thread to balance overhead.

## Simulation State (`simulation_state.rs`)

### Components

```rust
pub struct SimulationState {
    pub timeline: SimTimeline,      // Current date, start/end dates
    pub portfolio: SimPortfolio,    // Accounts, market, RMD tracking
    pub event_state: SimEventState, // Events, triggered status
    pub taxes: SimTaxState,         // YTD income, contributions
    pub history: SimHistory,        // Ledger, wealth snapshots
    pub warnings: Vec<SimulationWarning>,
}
```

### Timeline

```rust
pub struct SimTimeline {
    pub current_date: Date,
    pub start_date: Date,
    pub end_date: Date,
    pub birth_date: Option<Date>,
}
```

### Year-End Handling

At December 31:
1. Capture balances for RMD calculations (next year)
2. Snapshot wealth for reporting
3. Reset yearly contribution limits

## Safety Limits

### Iteration Limit

Maximum 1000 same-day iterations to prevent infinite loops:

```rust
const MAX_SAME_DATE_ITERATIONS: u64 = 1000;
```

Common cause: Balance-based triggers with `once: false` when sweep cannot fulfill request.

### Warnings

Non-fatal issues are captured as warnings:

```rust
pub struct SimulationWarning {
    pub date: Date,
    pub event_id: Option<EventId>,
    pub message: String,
    pub kind: WarningKind,
}
```

## Instrumented Simulation

For profiling and debugging:

```rust
pub fn simulate_with_metrics(
    params: &SimulationConfig,
    seed: u64,
    config: &InstrumentationConfig,
) -> Result<(SimulationResult, SimulationMetrics), MarketError>
```

Collects:
- Events triggered per day
- Iterations per time step
- Limit hits
- Total time steps
