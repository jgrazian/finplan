# Data Model

## Type-Safe Identifiers

All entities are referenced by newtype IDs (`model/ids.rs`):

```rust
pub struct AccountId(pub &'static str);
pub struct AssetId(pub &'static str);
pub struct EventId(pub &'static str);
pub struct ReturnProfileId(pub &'static str);

// Composite coordinate for assets within accounts
pub struct AssetCoord {
    pub account_id: AccountId,
    pub asset_id: AssetId,
}
```

## Accounts (`model/accounts.rs`)

### Tax Status

```rust
pub enum TaxStatus {
    Taxable,      // Capital gains taxed (Brokerage)
    TaxDeferred,  // Withdrawals taxed as income (401k, Traditional IRA)
    TaxFree,      // Qualified withdrawals tax-free (Roth)
}
```

### Account Flavors

```rust
pub enum AccountFlavor {
    // Liquid cash (checking, savings, HYSA)
    Bank(Cash),

    // Investment accounts with positions
    Investment(InvestmentContainer),

    // Fixed assets (real estate, vehicles)
    Property(FixedAsset),

    // Debt (mortgages, loans) - stored as positive, treated as negative
    Liability(LoanDetail),
}
```

### Investment Container

```rust
pub struct InvestmentContainer {
    pub tax_status: TaxStatus,
    pub cash: Cash,                              // Settlement fund
    pub positions: Vec<AssetLot>,                // Individual lots
    pub contribution_limit: Option<ContributionLimit>,
}
```

### Asset Lot (Cost Basis Tracking)

```rust
pub struct AssetLot {
    pub asset_id: AssetId,
    pub purchase_date: Date,
    pub units: f64,          // Shares/units
    pub cost_basis: f64,     // Total cost for this lot
}
```

## Events (`model/events.rs`)

### Event Structure

```rust
pub struct Event {
    pub event_id: EventId,
    pub trigger: EventTrigger,
    pub effects: Vec<EventEffect>,
    pub once: bool,  // If true, can only trigger once
}
```

### Event Triggers

```rust
pub enum EventTrigger {
    // Time-based
    Date(Date),
    Age { years: u8, months: Option<u8> },
    RelativeToEvent { event_id: EventId, offset: TriggerOffset },

    // Balance-based
    AccountBalance { account_id: AccountId, threshold: BalanceThreshold },
    AssetBalance { asset_coord: AssetCoord, threshold: BalanceThreshold },
    NetWorth { threshold: BalanceThreshold },

    // Compound
    And(Vec<EventTrigger>),
    Or(Vec<EventTrigger>),

    // Scheduled
    Repeating {
        interval: RepeatInterval,
        start_condition: Option<Box<EventTrigger>>,
        end_condition: Option<Box<EventTrigger>>,
    },

    // Manual (only via TriggerEvent effect)
    Manual,
}
```

### Event Effects

```rust
pub enum EventEffect {
    // Account management
    CreateAccount(Account),
    DeleteAccount(AccountId),

    // Cash flows
    Income { to: AccountId, amount: TransferAmount, amount_mode: AmountMode, income_type: IncomeType },
    Expense { from: AccountId, amount: TransferAmount },

    // Asset operations
    AssetPurchase { from: AccountId, to: AssetCoord, amount: TransferAmount },
    AssetSale { from: AccountId, asset_id: Option<AssetId>, amount: TransferAmount, amount_mode: AmountMode, lot_method: LotMethod },

    // Multi-account operations
    Sweep { sources: WithdrawalSources, to: AccountId, amount: TransferAmount, amount_mode: AmountMode, lot_method: LotMethod, income_type: IncomeType },
    CashTransfer { from: AccountId, to: AccountId, amount: TransferAmount },

    // Balance adjustments
    AdjustBalance { account: AccountId, amount: TransferAmount },

    // Event control
    TriggerEvent(EventId),
    PauseEvent(EventId),
    ResumeEvent(EventId),
    TerminateEvent(EventId),

    // RMD
    ApplyRmd { destination: AccountId, lot_method: LotMethod },
}
```

### Transfer Amounts

Flexible amount specification with arithmetic:

```rust
pub enum TransferAmount {
    // Simple cases
    Fixed(f64),
    SourceBalance,
    ZeroTargetBalance,
    TargetToBalance(f64),

    // Balance references
    AssetBalance { asset_coord: AssetCoord },
    AccountTotalBalance { account_id: AccountId },
    AccountCashBalance { account_id: AccountId },

    // Arithmetic
    Min(Box<TransferAmount>, Box<TransferAmount>),
    Max(Box<TransferAmount>, Box<TransferAmount>),
    Sub(Box<TransferAmount>, Box<TransferAmount>),
    Add(Box<TransferAmount>, Box<TransferAmount>),
    Mul(Box<TransferAmount>, Box<TransferAmount>),
}
```

### Withdrawal Strategies

```rust
pub enum WithdrawalOrder {
    TaxEfficientEarly,   // Taxable → TaxDeferred → TaxFree
    TaxDeferredFirst,    // Good for filling lower brackets
    TaxFreeFirst,        // Rarely optimal
    ProRata,             // Proportional from all
    PenaltyAware,        // Avoids early withdrawal penalties
}
```

### Lot Selection Methods

```rust
pub enum LotMethod {
    Fifo,        // First-in, first-out (default)
    Lifo,        // Last-in, first-out
    HighestCost, // Minimize realized gains
    LowestCost,  // Realize gains in low-income years
    AverageCost, // Common for mutual funds
}
```

## Market Data (`model/market.rs`)

### Return Profiles

```rust
pub enum ReturnProfile {
    Fixed(f64),           // Constant annual return
    Historical(Vec<f64>), // Historical sequence
    MonteCarlo {          // Random with distribution
        mean: f64,
        std_dev: f64,
    },
}
```

### Inflation Profile

```rust
pub enum InflationProfile {
    Fixed(f64),
    Historical(Vec<f64>),
    MonteCarlo { mean: f64, std_dev: f64 },
}
```

## Results (`model/results.rs`)

### Simulation Result

```rust
pub struct SimulationResult {
    pub wealth_snapshots: Vec<WealthSnapshot>,
    pub yearly_taxes: Vec<TaxSummary>,
    pub yearly_cash_flows: Vec<YearlyCashFlowSummary>,
    pub ledger: Vec<LedgerEntry>,
    pub warnings: Vec<SimulationWarning>,
}
```

### Yearly cash flows close

`YearlyCashFlowSummary` is built from the ledger so that a year's columns
explain its change in net worth exactly:

```
net worth at year end = last year end (or the start)
                        + income - expenses - taxes   (net_cash_flow)
                        + appreciation                (growth)
```

- `income` is gross: pay credited net of withholding is grossed up by the tax
  on it (`IncomeTax { on: Withheld }`), and a vest counts at its full value
  (`on: Vest`). Tax on money the plan already held (`on: Holdings`) adds no
  income. A user `AdjustBalance` that raises net worth is income.
- `expenses` are `Expense` debits, interest accrued on loans, and an
  `AdjustBalance` that lowers net worth. Not `Tax` debits (the tax entry
  counts them) and not `DebtPayment` debits (cash into less debt).
- `taxes` are income and capital-gains tax and early-withdrawal penalties:
  the year's `TaxSummary`, each counted once.
- `appreciation` is cash interest (`CashAppreciation`) plus holdings repriced
  (`AssetAppreciation`, including a `MarketShock`).
- `contributions` and `withdrawals` are moves between the plan's own
  accounts, so they are shown but are not in the sum.

The year-end snapshot is taken after December 31's events, so every entry
dated in a year falls before its snapshot. `tests/reconciliation.rs` holds
this on a plan that moves money every way the engine can.

### Monte Carlo Summary

```rust
pub struct MonteCarloSummary {
    pub stats: MonteCarloStats,
    pub percentile_runs: Vec<(f64, SimulationResult)>,
    pub mean_accumulators: Option<MeanAccumulators>,
}

pub struct MonteCarloStats {
    pub num_iterations: usize,
    pub success_rate: f64,
    pub mean_final_net_worth: f64,
    pub std_dev_final_net_worth: f64,
    pub min_final_net_worth: f64,
    pub max_final_net_worth: f64,
    pub percentile_values: Vec<(f64, f64)>,
}
```

## Tax Configuration (`model/tax_config.rs`)

```rust
pub struct TaxConfig {
    pub federal_brackets: Vec<TaxBracket>,   // ascending, first at 0
    pub state_rate: f64,                     // flat, on all taxed income
    pub capital_gains_rate: f64,             // flat, long-term gains
    pub early_withdrawal_penalty_rate: f64,  // TaxDeferred before 59.5
    pub standard_deduction: f64,             // in the brackets' dollars
    pub age_65_extra_deduction: f64,         // from the year the person turns 65
}

pub struct TaxBracket {
    pub threshold: f64,  // income where this bracket begins
    pub rate: f64,
}
```

Each tax year the simulation rebuilds the brackets it charges with
(`SimulationState::index_federal_brackets`): the year's deduction (plus the
65+ extra once the person counts as 65, which needs a birth date) becomes a 0%
band and lifts every threshold by the same amount, then every threshold is
scaled by the path's cumulative inflation through the prior year. Tax on
`income` is then tax on `income - deduction` under the configured brackets,
and both hold steady in real terms.

## Ledger Entries (`model/state_event.rs`)

```rust
pub struct LedgerEntry {
    pub date: Date,
    pub event: StateEvent,
}

pub enum StateEvent {
    // Cash movements
    CashCredit { account_id, amount, kind, source },
    CashDebit { account_id, amount, kind, destination },
    CashAppreciation { account_id, previous_value, new_value, return_rate, days },
    // Holdings repriced: positions at the new prices, or a property's value
    AssetAppreciation { account_id, previous_value, new_value, days },

    // Asset movements
    AssetPurchase { account_id, lot },
    AssetSale { account_id, lots_sold, proceeds, cost_basis, gain, tax_info },

    // Time tracking
    TimeAdvance { from_date, to_date, days_elapsed },

    // Other
    LiabilityInterestAccrual { ... },
    TaxWithholding { ... },
}
```
