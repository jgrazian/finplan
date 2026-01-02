# Event System Refactoring Plan

## Overview

This document outlines a comprehensive refactoring of the finplan simulation engine to unify all money movement under a single, powerful event-based system. The goal is to eliminate redundant concepts, solve the balance overshoot problem, and create a more intuitive and maintainable architecture.

## Motivation

### Current Problems

1. **Balance Overshoot Issue**: Balance-based event triggers (e.g., "stop mortgage payments when balance >= 0") don't prevent the triggering cash flow from overshooting the target balance.

2. **Dual Scheduling Systems**: Both `CashFlow` and `Event` with `Repeating` trigger can schedule recurring actions, creating confusion and code duplication.

3. **Redundant Constructs**: Multiple constructs do similar things:
   - `CashFlow`, `TransferAsset`, `SweepToAccount`, and `SpendingTarget` all move money
   - CashFlow control effects (`ActivateCashFlow`, `PauseCashFlow`, etc.) add complexity
   - SpendingTarget duplicates Sweep functionality

4. **No Cost Basis Tracking**: Capital gains calculations use a simplified `taxable_gains_percentage` rather than proper lot tracking.

### Design Goals

1. **Single scheduling mechanism**: All recurring actions use `Event` with `Repeating` trigger
2. **Unified money movement**: `Transfer`, `Liquidate`, and `Sweep` effects with powerful amount calculation
3. **Explicit tax events**: `Liquidate` effect for tax-aware sales with lot selection
4. **Cash-centric flow**: All external money flows through a Cash account
5. **Solve balance overshoot**: `TransferAmount` DSL enables exact-amount transfers
6. **Remove redundancy**: Eliminate CashFlow and SpendingTarget entirely

---

## Phase 1: Add Cash Asset Class

### Changes to `model/accounts.rs`

Add `Cash` to the `AssetClass` enum to explicitly track liquid cash holdings:

```rust
/// Classification of an asset for valuation behavior
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetClass {
    /// Cash, money market - fully liquid, no capital gains
    Cash,
    /// Stocks, bonds, mutual funds - liquid and investable
    Investable,
    /// Property value - typically illiquid
    RealEstate,
    /// Cars, boats, equipment - lose value over time
    Depreciating,
    /// Loans, mortgages - value should be negative
    Liability,
}
```

### Rationale

- `Cash` assets have no capital gains (cost basis always equals value)
- Enables the cash-centric model where all external flows go through cash
- Clear distinction between "money" and "investments"
- Return profile for cash would typically be a low fixed rate (savings interest)

---

## Phase 2: TransferAmount DSL

### New Types in `model/events.rs`

```rust
/// Specifies how much to transfer - supports both simple cases and complex calculations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferAmount {
    // === Simple Cases (90% of use) ===
    
    /// Fixed dollar amount
    Fixed(f64),
    
    /// Transfer entire source balance
    SourceBalance,
    
    /// Transfer enough to zero out target balance (for debt payoff)
    /// Calculates: -1 * target_balance (turns negative debt to zero)
    ZeroTargetBalance,
    
    /// Transfer enough to bring target to specified balance
    /// Calculates: max(0, target_balance - current_target_balance)
    TargetToBalance(f64),
    
    // === Balance References ===
    
    /// Reference a specific asset's balance
    AssetBalance {
        account_id: AccountId,
        asset_id: AssetId,
    },
    
    /// Reference total account balance (sum of all assets)
    AccountBalance {
        account_id: AccountId,
    },
    
    // === Arithmetic Operations (for complex cases) ===
    
    /// Minimum of two amounts
    Min(Box<TransferAmount>, Box<TransferAmount>),
    
    /// Maximum of two amounts
    Max(Box<TransferAmount>, Box<TransferAmount>),
    
    /// Subtract: left - right
    Sub(Box<TransferAmount>, Box<TransferAmount>),
    
    /// Add: left + right
    Add(Box<TransferAmount>, Box<TransferAmount>),
    
    /// Multiply: left * right
    Mul(Box<TransferAmount>, Box<TransferAmount>),
}
```

### Helper Constructors

```rust
impl TransferAmount {
    /// Transfer the lesser of a fixed amount or available balance
    pub fn up_to(amount: f64) -> Self {
        TransferAmount::Min(
            Box::new(TransferAmount::Fixed(amount)),
            Box::new(TransferAmount::SourceBalance),
        )
    }
    
    /// Transfer all balance above a reserve amount
    pub fn excess_above(reserve: f64) -> Self {
        TransferAmount::Max(
            Box::new(TransferAmount::Fixed(0.0)),
            Box::new(TransferAmount::Sub(
                Box::new(TransferAmount::SourceBalance),
                Box::new(TransferAmount::Fixed(reserve)),
            )),
        )
    }
}
```

### Usage Examples

```rust
// Pay off mortgage exactly
amount: TransferAmount::ZeroTargetBalance

// Fixed monthly investment
amount: TransferAmount::Fixed(3000.0)

// Invest all cash above $10k reserve
amount: TransferAmount::excess_above(10_000.0)

// Transfer minimum of $5000 or available balance
amount: TransferAmount::up_to(5000.0)

// Transfer to bring target to $50k
amount: TransferAmount::TargetToBalance(50_000.0)
```

---

## Phase 3: TransferEndpoint

### New Type

```rust
/// Source or destination for a transfer
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransferEndpoint {
    /// External world (income source or expense destination)
    /// No cost basis tracking, no capital gains
    External,
    
    /// Specific asset within an account
    Asset {
        account_id: AccountId,
        asset_id: AssetId,
    },
}
```

### Semantic Meaning

| From | To | Meaning |
|------|-----|---------|
| `External` | `Asset` | Income (salary, dividends, etc.) |
| `Asset` | `External` | Expense (bills, purchases, etc.) |
| `Asset` | `Asset` | Internal transfer (rebalancing, investment, etc.) |
| `External` | `External` | Invalid - rejected at validation |

---

## Phase 4: Flow Limits

### New Type

```rust
/// Period over which limits reset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitPeriod {
    /// Resets every calendar year
    Yearly,
    /// Never resets
    Lifetime,
}

/// Limits on cumulative transfer amounts (e.g., IRS contribution limits)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FlowLimits {
    /// Maximum cumulative amount
    pub limit: f64,
    /// How often the limit resets
    pub period: LimitPeriod,
}
```

---

## Phase 5: Lot Method for Capital Gains

### New Type

```rust
/// Method for selecting which lots to sell (affects capital gains calculation)
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum LotMethod {
    /// First-in, first-out (default, most common)
    #[default]
    Fifo,
    /// Last-in, first-out
    Lifo,
    /// Sell highest cost lots first (minimize realized gains)
    HighestCost,
    /// Sell lowest cost lots first (realize gains in low-income years)
    LowestCost,
    /// Average cost basis (common for mutual funds)
    AverageCost,
}
```

---

## Phase 6: Withdrawal Sources for Sweep

### New Types

```rust
/// Pre-defined withdrawal order strategies
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WithdrawalOrder {
    /// Taxable accounts first, then tax-deferred, then tax-free
    /// Minimizes taxes in early retirement, preserves tax-advantaged growth
    TaxEfficientEarly,
    
    /// Tax-deferred first, then taxable, then tax-free
    /// Good for filling lower tax brackets in early retirement
    TaxDeferredFirst,
    
    /// Tax-free first, then taxable, then tax-deferred
    /// Rarely optimal, but available
    TaxFreeFirst,
    
    /// Pro-rata from all accounts proportionally
    /// Maintains consistent tax treatment over time
    ProRata,
}

/// Source configuration for Sweep withdrawals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WithdrawalSources {
    /// Use a pre-defined withdrawal order strategy
    /// Automatically selects from all non-excluded liquid accounts
    Strategy {
        order: WithdrawalOrder,
        /// Accounts to exclude from automatic selection
        #[serde(default)]
        exclude_accounts: Vec<AccountId>,
    },
    
    /// Explicitly specify accounts/assets in priority order
    Custom(Vec<(AccountId, AssetId)>),
}

impl Default for WithdrawalSources {
    fn default() -> Self {
        WithdrawalSources::Strategy {
            order: WithdrawalOrder::TaxEfficientEarly,
            exclude_accounts: vec![],
        }
    }
}
```

---

## Phase 7: Withdrawal Amount Mode

### New Type

```rust
/// How to interpret the withdrawal amount
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum WithdrawalAmountMode {
    /// Amount is gross (before taxes)
    /// Withdraw exactly this amount, taxes come out of it
    #[default]
    Gross,
    
    /// Amount is net (after taxes)
    /// Gross up withdrawal to cover taxes, so you receive this amount
    Net,
}
```

---

## Phase 8: New EventEffect Variants

### Transfer Effect

Replaces: `TransferAsset`, all CashFlow functionality

```rust
/// Move money between endpoints (external world or assets)
/// Tax implications determined automatically by account types
Transfer {
    /// Source of funds
    from: TransferEndpoint,
    /// Destination for funds
    to: TransferEndpoint,
    /// How much to transfer
    amount: TransferAmount,
    /// Adjust amount for inflation over time
    #[serde(default)]
    adjust_for_inflation: bool,
    /// Optional cumulative limits (e.g., IRS contribution limits)
    #[serde(default)]
    limits: Option<FlowLimits>,
}
```

### Liquidate Effect

Explicit tax-aware sale from taxable accounts:

```rust
/// Sell assets from a taxable account with explicit capital gains handling
/// Use this instead of Transfer when you need control over lot selection
Liquidate {
    /// Account to sell from (should be Taxable type)
    from_account: AccountId,
    /// Asset to sell
    from_asset: AssetId,
    /// Where proceeds go
    to_account: AccountId,
    /// Asset to receive proceeds (typically Cash)
    to_asset: AssetId,
    /// Amount to liquidate (gross, before taxes)
    amount: TransferAmount,
    /// How to select lots for sale
    #[serde(default)]
    lot_method: LotMethod,
}
```

### Enhanced Sweep Effect

Multi-source withdrawal with strategy support (replaces SpendingTarget):

```rust
/// Sweep funds from multiple sources to reach target amount/balance
/// Handles liquidation taxes and prioritized source ordering
/// Replaces SpendingTarget functionality
Sweep {
    /// Destination account
    to_account: AccountId,
    /// Destination asset (typically Cash)
    to_asset: AssetId,
    /// Target amount or balance
    target: TransferAmount,
    /// Where to withdraw from
    #[serde(default)]
    sources: WithdrawalSources,
    /// How to interpret the target amount
    #[serde(default)]
    amount_mode: WithdrawalAmountMode,
    /// Lot selection method for taxable sources
    #[serde(default)]
    lot_method: LotMethod,
}
```

---

## Phase 9: Updated EventEffect Enum

### Complete New Definition

```rust
/// Actions that can occur when an event triggers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventEffect {
    // === Account Management ===
    
    /// Create a new account during simulation
    CreateAccount(Account),
    /// Remove an account
    DeleteAccount(AccountId),
    
    // === Money Movement ===
    
    /// Transfer money between endpoints (external or assets)
    /// Tax implications are automatic based on account types
    Transfer {
        from: TransferEndpoint,
        to: TransferEndpoint,
        amount: TransferAmount,
        #[serde(default)]
        adjust_for_inflation: bool,
        #[serde(default)]
        limits: Option<FlowLimits>,
    },
    
    /// Explicitly liquidate assets with capital gains handling
    Liquidate {
        from_account: AccountId,
        from_asset: AssetId,
        to_account: AccountId,
        to_asset: AssetId,
        amount: TransferAmount,
        #[serde(default)]
        lot_method: LotMethod,
    },
    
    /// Multi-source sweep with withdrawal strategy
    /// Replaces SpendingTarget functionality
    Sweep {
        to_account: AccountId,
        to_asset: AssetId,
        target: TransferAmount,
        #[serde(default)]
        sources: WithdrawalSources,
        #[serde(default)]
        amount_mode: WithdrawalAmountMode,
        #[serde(default)]
        lot_method: LotMethod,
    },
    
    // === Event Control ===
    
    /// Trigger another event immediately
    TriggerEvent(EventId),
    /// Pause a repeating event
    PauseEvent(EventId),
    /// Resume a paused event  
    ResumeEvent(EventId),
    /// Terminate an event permanently
    TerminateEvent(EventId),
    
    // === RMD (Required Minimum Distributions) ===
    
    /// Set up automatic RMD withdrawals from tax-deferred account
    CreateRmdWithdrawal {
        account_id: AccountId,
        starting_age: u8,
    },
}
```

### Removed Effects

The following are removed entirely:

**CashFlow-related (replaced by `Transfer`):**
- `CreateCashFlow`
- `ActivateCashFlow`
- `PauseCashFlow`
- `ResumeCashFlow`
- `TerminateCashFlow`
- `ModifyCashFlow`

**SpendingTarget-related (replaced by `Sweep`):**
- `CreateSpendingTarget`
- `ActivateSpendingTarget`
- `PauseSpendingTarget`
- `ResumeSpendingTarget`
- `TerminateSpendingTarget`
- `ModifySpendingTarget`

**Other (replaced):**
- `TransferAsset` (replaced by `Transfer`)
- `SweepToAccount` (replaced by `Sweep`)

---

## Phase 10: Updated EventTrigger

### Changes

Remove CashFlow and SpendingTarget-related triggers:

```rust
pub enum EventTrigger {
    // === Time-Based Triggers ===
    Date(jiff::civil::Date),
    Age { years: u8, months: Option<u8> },
    RelativeToEvent { event_id: EventId, offset: TriggerOffset },
    
    // === Balance-Based Triggers ===
    AccountBalance { account_id: AccountId, threshold: BalanceThreshold },
    AssetBalance { account_id: AccountId, asset_id: AssetId, threshold: BalanceThreshold },
    NetWorth { threshold: BalanceThreshold },
    AccountDepleted(AccountId),
    
    // === Event-Based Triggers ===
    /// Trigger when another event fires
    EventTriggered(EventId),
    /// Trigger when an event is terminated
    EventEnded(EventId),
    
    // REMOVED: CashFlowEnded (no more CashFlows)
    // REMOVED: TotalIncomeBelow (can be modeled with balance triggers)
    
    // === Compound Triggers ===
    And(Vec<EventTrigger>),
    Or(Vec<EventTrigger>),
    
    // === Scheduled Triggers ===
    Repeating {
        interval: RepeatInterval,
        #[serde(default)]
        start_condition: Option<Box<EventTrigger>>,
        /// Optional: stop repeating when this condition is met
        #[serde(default)]
        end_condition: Option<Box<EventTrigger>>,
    },
    
    // === Manual Control ===
    Manual,
}
```

### New: end_condition for Repeating

Allows events to self-terminate without needing separate termination events:

```rust
// Monthly mortgage payment that stops when paid off
Event {
    trigger: EventTrigger::Repeating {
        interval: RepeatInterval::Monthly,
        start_condition: None,
        end_condition: Some(Box::new(EventTrigger::AssetBalance {
            account_id: MORTGAGE_DEBT,
            asset_id: MORTGAGE,
            threshold: BalanceThreshold::GreaterThanOrEqual(0.0),
        })),
    },
    effects: vec![EventEffect::Transfer {
        from: Asset { account_id: CASH, asset_id: CASH },
        to: Asset { account_id: MORTGAGE_DEBT, asset_id: MORTGAGE },
        amount: TransferAmount::Min(
            Box::new(TransferAmount::Fixed(2500.0)),
            Box::new(TransferAmount::ZeroTargetBalance),
        ),
        adjust_for_inflation: false,
        limits: None,
    }],
    once: false,
}
```

---

## Phase 11: Cost Basis Tracking

### New State Tracking

Add to `SimulationState`:

```rust
/// A single purchase lot for cost basis tracking
#[derive(Debug, Clone)]
pub struct AssetLot {
    /// Date of purchase
    pub purchase_date: jiff::civil::Date,
    /// Number of shares/units (or dollar amount for non-share assets)
    pub units: f64,
    /// Total cost basis for this lot
    pub cost_basis: f64,
}

// In SimulationState:
/// Cost basis lots per asset (only for Taxable accounts)
pub asset_lots: HashMap<(AccountId, AssetId), Vec<AssetLot>>,
```

### Enhanced Asset Definition

```rust
pub struct Asset {
    pub asset_id: AssetId,
    pub asset_class: AssetClass,
    pub initial_value: f64,
    pub return_profile_index: usize,
    /// Initial cost basis (defaults to initial_value if not specified)
    /// Only relevant for Taxable accounts
    #[serde(default)]
    pub initial_cost_basis: Option<f64>,
}
```

### Tax Calculation Logic

```rust
fn liquidate_lots(
    account_id: AccountId,
    asset_id: AssetId,
    amount: f64,
    lot_method: LotMethod,
    state: &mut SimulationState,
) -> LiquidationResult {
    let lots = state.asset_lots.get_mut(&(account_id, asset_id));
    
    // Sort lots based on method
    match lot_method {
        LotMethod::Fifo => lots.sort_by_key(|l| l.purchase_date),
        LotMethod::Lifo => lots.sort_by_key(|l| std::cmp::Reverse(l.purchase_date)),
        LotMethod::HighestCost => lots.sort_by(|a, b| {
            (b.cost_basis / b.units).partial_cmp(&(a.cost_basis / a.units)).unwrap()
        }),
        LotMethod::LowestCost => lots.sort_by(|a, b| {
            (a.cost_basis / a.units).partial_cmp(&(b.cost_basis / b.units)).unwrap()
        }),
        LotMethod::AverageCost => { /* handled differently */ }
    }
    
    // Consume lots until amount is satisfied
    let mut remaining = amount;
    let mut total_cost_basis = 0.0;
    let mut total_proceeds = 0.0;
    let mut short_term_gain = 0.0;
    let mut long_term_gain = 0.0;
    
    while remaining > 0.0 && !lots.is_empty() {
        let lot = &mut lots[0];
        let lot_value = /* current value based on returns */;
        let take_amount = remaining.min(lot_value);
        let take_fraction = take_amount / lot_value;
        
        let basis_used = lot.cost_basis * take_fraction;
        total_cost_basis += basis_used;
        total_proceeds += take_amount;
        
        let gain = take_amount - basis_used;
        let holding_days = (state.current_date - lot.purchase_date).get_days();
        
        if holding_days >= 365 {
            long_term_gain += gain;
        } else {
            short_term_gain += gain;
        }
        
        // Reduce or remove lot
        lot.units *= (1.0 - take_fraction);
        lot.cost_basis -= basis_used;
        if lot.units <= 0.001 {
            lots.remove(0);
        }
        
        remaining -= take_amount;
    }
    
    LiquidationResult {
        proceeds: total_proceeds,
        cost_basis: total_cost_basis,
        short_term_gain,
        long_term_gain,
    }
}
```

---

## Phase 12: Record Types Update

### Enhanced RecordKind

```rust
pub enum RecordKind {
    /// External income received
    Income {
        to_account_id: AccountId,
        to_asset_id: AssetId,
        amount: f64,
        event_id: EventId,
    },
    
    /// External expense paid
    Expense {
        from_account_id: AccountId,
        from_asset_id: AssetId,
        amount: f64,
        event_id: EventId,
    },
    
    /// Internal transfer between assets (no tax implications tracked here)
    Transfer {
        from_account_id: AccountId,
        from_asset_id: AssetId,
        to_account_id: AccountId,
        to_asset_id: AssetId,
        amount: f64,
        event_id: EventId,
    },
    
    /// Liquidation with capital gains
    Liquidation {
        from_account_id: AccountId,
        from_asset_id: AssetId,
        to_account_id: AccountId,
        to_asset_id: AssetId,
        gross_amount: f64,
        cost_basis: f64,
        short_term_gain: f64,
        long_term_gain: f64,
        federal_tax: f64,
        state_tax: f64,
        net_proceeds: f64,
        lot_method: LotMethod,
        event_id: EventId,
    },
    
    /// Sweep withdrawal (may include multiple liquidations)
    Sweep {
        to_account_id: AccountId,
        to_asset_id: AssetId,
        target_amount: f64,
        actual_gross: f64,
        actual_net: f64,
        amount_mode: WithdrawalAmountMode,
        event_id: EventId,
    },
    
    /// Investment return applied
    Return {
        account_id: AccountId,
        asset_id: AssetId,
        balance_before: f64,
        return_rate: f64,
        return_amount: f64,
    },
    
    /// Event triggered
    Event {
        event_id: EventId,
    },
    
    /// RMD withdrawal
    Rmd {
        account_id: AccountId,
        age: u8,
        prior_year_balance: f64,
        irs_divisor: f64,
        required_amount: f64,
        actual_withdrawn: f64,
    },
}
```

---

## Phase 13: SimulationParameters Changes

### Remove CashFlow and SpendingTarget References

```rust
pub struct SimulationParameters {
    pub start_date: Option<jiff::civil::Date>,
    pub duration_years: usize,
    pub birth_date: Option<jiff::civil::Date>,
    
    pub inflation_profile: InflationProfile,
    pub return_profiles: Vec<ReturnProfile>,
    
    pub accounts: Vec<Account>,
    pub events: Vec<Event>,  // Now the ONLY source of scheduled actions
    
    // REMOVED: pub cash_flows: Vec<CashFlow>,
    // REMOVED: pub spending_targets: Vec<SpendingTarget>,
    
    #[serde(default)]
    pub tax_config: TaxConfig,
}
```

---

## Phase 14: Simulation Engine Changes

### Remove apply_cash_flows and apply_spending_targets

The main simulation loop simplifies dramatically:

```rust
pub fn simulate(params: &SimulationParameters, seed: u64) -> SimulationResult {
    let mut state = SimulationState::from_parameters(params, seed);

    while state.current_date < state.end_date {
        let mut something_happened = true;
        while something_happened {
            something_happened = false;

            // Process events (now handles ALL money movement)
            if !process_events(&mut state).is_empty() {
                something_happened = true;
            }
            
            // REMOVED: apply_cash_flows()
            // REMOVED: apply_spending_targets()
        }

        advance_time(&mut state, params);
    }

    state.finalize_year_taxes();
    // ... build result
}
```

### Enhanced process_events

Add handlers for new effect types:

```rust
fn apply_effect(effect: &EventEffect, state: &mut SimulationState, event_id: EventId) {
    match effect {
        EventEffect::Transfer { from, to, amount, adjust_for_inflation, limits } => {
            let calculated_amount = evaluate_transfer_amount(amount, from, to, state);
            let adjusted_amount = if *adjust_for_inflation {
                state.inflation_adjusted_amount(calculated_amount, true, params.duration_years)
            } else {
                calculated_amount
            };
            let final_amount = apply_limits(adjusted_amount, event_id, limits, state);
            
            execute_transfer(from, to, final_amount, event_id, state);
        }
        
        EventEffect::Liquidate { from_account, from_asset, to_account, to_asset, amount, lot_method } => {
            let calculated_amount = evaluate_transfer_amount(amount, /* ... */, state);
            execute_liquidation(
                *from_account, *from_asset,
                *to_account, *to_asset,
                calculated_amount, *lot_method, event_id, state
            );
        }
        
        EventEffect::Sweep { to_account, to_asset, target, sources, amount_mode, lot_method } => {
            let target_amount = evaluate_transfer_amount(target, /* ... */, state);
            
            // Resolve sources based on strategy or custom list
            let source_list = resolve_withdrawal_sources(sources, state);
            
            // Calculate gross needed based on amount mode
            let gross_needed = match amount_mode {
                WithdrawalAmountMode::Gross => target_amount,
                WithdrawalAmountMode::Net => gross_up_for_taxes(target_amount, &source_list, state),
            };
            
            // Execute withdrawals from sources in order
            let mut remaining = gross_needed;
            for (src_account, src_asset) in source_list {
                if remaining <= 0.0 { break; }
                let available = state.asset_balance(src_account, src_asset);
                let take = remaining.min(available);
                
                execute_liquidation(
                    src_account, src_asset,
                    *to_account, *to_asset,
                    take, *lot_method, event_id, state
                );
                remaining -= take;
            }
        }
        
        // ... other effects
    }
}

fn resolve_withdrawal_sources(
    sources: &WithdrawalSources,
    state: &SimulationState,
) -> Vec<(AccountId, AssetId)> {
    match sources {
        WithdrawalSources::Custom(list) => list.clone(),
        WithdrawalSources::Strategy { order, exclude_accounts } => {
            let mut accounts: Vec<_> = state.accounts.iter()
                .filter(|(id, acc)| {
                    !exclude_accounts.contains(id) &&
                    !matches!(acc.account_type, AccountType::Illiquid)
                })
                .collect();
            
            // Sort by strategy
            match order {
                WithdrawalOrder::TaxEfficientEarly => {
                    accounts.sort_by_key(|(_, acc)| match acc.account_type {
                        AccountType::Taxable => 0,
                        AccountType::TaxDeferred => 1,
                        AccountType::TaxFree => 2,
                        AccountType::Illiquid => 3,
                    });
                }
                WithdrawalOrder::TaxDeferredFirst => {
                    accounts.sort_by_key(|(_, acc)| match acc.account_type {
                        AccountType::TaxDeferred => 0,
                        AccountType::Taxable => 1,
                        AccountType::TaxFree => 2,
                        AccountType::Illiquid => 3,
                    });
                }
                WithdrawalOrder::TaxFreeFirst => {
                    accounts.sort_by_key(|(_, acc)| match acc.account_type {
                        AccountType::TaxFree => 0,
                        AccountType::Taxable => 1,
                        AccountType::TaxDeferred => 2,
                        AccountType::Illiquid => 3,
                    });
                }
                WithdrawalOrder::ProRata => {
                    // ProRata handled specially - return all and calculate proportions
                }
            }
            
            // Flatten to (AccountId, AssetId) pairs
            accounts.iter()
                .flat_map(|(acc_id, acc)| {
                    acc.assets.iter().map(|asset| (**acc_id, asset.asset_id))
                })
                .collect()
        }
    }
}
```

---

## Phase 15: Migration Examples

### Before: Monthly Salary (CashFlow)

```rust
// Old
CashFlow {
    cash_flow_id: CashFlowId(1),
    amount: 8000.0,
    repeats: RepeatInterval::Monthly,
    direction: CashFlowDirection::Income {
        target_account_id: CHECKING,
        target_asset_id: CASH,
    },
    state: CashFlowState::Active,
}
```

### After: Monthly Salary (Event)

```rust
// New
Event {
    event_id: EventId(1),
    trigger: EventTrigger::Repeating {
        interval: RepeatInterval::Monthly,
        start_condition: None,
        end_condition: None,
    },
    effects: vec![EventEffect::Transfer {
        from: TransferEndpoint::External,
        to: TransferEndpoint::Asset {
            account_id: CHECKING,
            asset_id: CASH,
        },
        amount: TransferAmount::Fixed(8000.0),
        adjust_for_inflation: true,
        limits: None,
    }],
    once: false,
}
```

### Before: Mortgage Payment (CashFlow + Event to stop)

```rust
// Old - needed TWO constructs plus a HACK threshold
CashFlow {
    cash_flow_id: CashFlowId(4),
    amount: 2500.0,
    repeats: RepeatInterval::Monthly,
    direction: CashFlowDirection::Income {
        target_account_id: MORTGAGE_DEBT,
        target_asset_id: MORTGAGE,
    },
    state: CashFlowState::Active,
}

Event {
    event_id: EventId(101),
    trigger: EventTrigger::AccountBalance {
        account_id: MORTGAGE_DEBT,
        threshold: BalanceThreshold::GreaterThanOrEqual(-1000.0), // HACK: buffer for overshoot
    },
    effects: vec![EventEffect::TerminateCashFlow(CashFlowId(4))],
    once: true,
}
```

### After: Mortgage Payment (Single Event, No Overshoot)

```rust
// New - ONE construct, exact payoff
Event {
    event_id: EventId(1),
    trigger: EventTrigger::Repeating {
        interval: RepeatInterval::Monthly,
        start_condition: None,
        end_condition: Some(Box::new(EventTrigger::AssetBalance {
            account_id: MORTGAGE_DEBT,
            asset_id: MORTGAGE,
            threshold: BalanceThreshold::GreaterThanOrEqual(0.0), // Exact!
        })),
    },
    effects: vec![EventEffect::Transfer {
        from: TransferEndpoint::Asset { account_id: CASH_ACCOUNT, asset_id: CASH },
        to: TransferEndpoint::Asset { account_id: MORTGAGE_DEBT, asset_id: MORTGAGE },
        amount: TransferAmount::Min(
            Box::new(TransferAmount::Fixed(2500.0)),
            Box::new(TransferAmount::ZeroTargetBalance), // Won't overpay!
        ),
        adjust_for_inflation: false,
        limits: None,
    }],
    once: false,
}
```

### Before: 401k Contribution with Limit

```rust
// Old
CashFlow {
    cash_flow_id: CashFlowId(2),
    amount: 2000.0,
    repeats: RepeatInterval::Monthly,
    cash_flow_limits: Some(CashFlowLimits {
        limit: 23000.0,
        limit_period: LimitPeriod::Yearly,
    }),
    direction: CashFlowDirection::Income {
        target_account_id: TRAD_401K,
        target_asset_id: SP500,
    },
    state: CashFlowState::Active,
}
```

### After: 401k Contribution with Limit

```rust
// New
Event {
    event_id: EventId(2),
    trigger: EventTrigger::Repeating {
        interval: RepeatInterval::Monthly,
        start_condition: None,
        end_condition: None,
    },
    effects: vec![EventEffect::Transfer {
        from: TransferEndpoint::Asset { account_id: CASH_ACCOUNT, asset_id: CASH },
        to: TransferEndpoint::Asset { account_id: TRAD_401K, asset_id: SP500 },
        amount: TransferAmount::Fixed(2000.0),
        adjust_for_inflation: false,
        limits: Some(FlowLimits {
            limit: 23000.0,
            period: LimitPeriod::Yearly,
        }),
    }],
    once: false,
}
```

### Before: Retirement Spending (SpendingTarget)

```rust
// Old
SpendingTarget {
    spending_target_id: SpendingTargetId(1),
    amount: 80_000.0,
    net_amount_mode: true,
    repeats: RepeatInterval::Yearly,
    adjust_for_inflation: true,
    withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
    exclude_accounts: vec![REAL_ESTATE],
    state: SpendingTargetState::Pending,
}

// Plus an event to activate it
Event {
    trigger: EventTrigger::Age { years: 65, months: None },
    effects: vec![EventEffect::ActivateSpendingTarget(SpendingTargetId(1))],
    once: true,
}
```

### After: Retirement Spending (Single Event with Sweep)

```rust
// New - ONE construct
Event {
    event_id: EventId(100),
    trigger: EventTrigger::Repeating {
        interval: RepeatInterval::Yearly,
        start_condition: Some(Box::new(EventTrigger::Age { years: 65, months: None })),
        end_condition: None,
    },
    effects: vec![EventEffect::Sweep {
        to_account: CASH_ACCOUNT,
        to_asset: CASH,
        target: TransferAmount::Fixed(80_000.0),
        sources: WithdrawalSources::Strategy {
            order: WithdrawalOrder::TaxEfficientEarly,
            exclude_accounts: vec![REAL_ESTATE],
        },
        amount_mode: WithdrawalAmountMode::Net,
        lot_method: LotMethod::HighestCost,
    }],
    once: false,
}
```

### Tax-Loss Harvesting Example

```rust
// Sell highest-cost lots to minimize gains
Event {
    event_id: EventId(10),
    trigger: EventTrigger::Date(jiff::civil::date(2025, 12, 15)),
    effects: vec![EventEffect::Liquidate {
        from_account: BROKERAGE,
        from_asset: VTSAX,
        to_account: CASH_ACCOUNT,
        to_asset: CASH,
        amount: TransferAmount::Fixed(50_000.0),
        lot_method: LotMethod::HighestCost,
    }],
    once: true,
}
```

### Cash Buffer Maintenance (Replaces SweepToAccount)

```rust
// Keep $20k in cash, funded from investments
Event {
    event_id: EventId(20),
    trigger: EventTrigger::AssetBalance {
        account_id: CASH_ACCOUNT,
        asset_id: CASH,
        threshold: BalanceThreshold::LessThanOrEqual(5_000.0),
    },
    effects: vec![EventEffect::Sweep {
        to_account: CASH_ACCOUNT,
        to_asset: CASH,
        target: TransferAmount::TargetToBalance(20_000.0),
        sources: WithdrawalSources::Custom(vec![
            (BROKERAGE, BONDS),   // Sell bonds first
            (BROKERAGE, STOCKS),  // Then stocks if needed
        ]),
        amount_mode: WithdrawalAmountMode::Gross,
        lot_method: LotMethod::HighestCost,
    }],
    once: false,
}
```

---

## Phase 16: File Changes Summary

| File | Changes |
|------|---------|
| `model/accounts.rs` | Add `Cash` to `AssetClass`, add `initial_cost_basis` to `Asset` |
| `model/events.rs` | New `TransferAmount`, `TransferEndpoint`, `LotMethod`, `FlowLimits`, `WithdrawalSources`, `WithdrawalOrder`, `WithdrawalAmountMode`; rewrite `EventEffect` |
| `model/cash_flows.rs` | Keep `RepeatInterval` and `LimitPeriod` only, remove everything else |
| `model/spending.rs` | **DELETE ENTIRE FILE** |
| `model/records.rs` | Update `RecordKind` variants |
| `model/mod.rs` | Update exports, remove spending module |
| `config/parameters.rs` | Remove `cash_flows` and `spending_targets` fields |
| `config/descriptors.rs` | Remove `CashFlowDescriptor` and `SpendingTargetDescriptor`, add event helper builders |
| `simulation.rs` | Remove `apply_cash_flows()` and `apply_spending_targets()` |
| `simulation_state.rs` | Remove CashFlow and SpendingTarget tracking, add `asset_lots` |
| `event_engine.rs` | Add handlers for `Transfer`, `Liquidate`, `Sweep`; remove CashFlow/SpendingTarget handlers |
| `taxes.rs` | Add lot-based capital gains calculation |
| `tests/*.rs` | Update all tests to use new event system |

---

## Phase 17: Implementation Order

1. **Add new types** (non-breaking)
   - `AssetClass::Cash`
   - `TransferAmount` enum
   - `TransferEndpoint` enum
   - `LotMethod` enum
   - `FlowLimits` struct
   - `WithdrawalSources` enum
   - `WithdrawalOrder` enum
   - `WithdrawalAmountMode` enum

2. **Add new EventEffect variants** (non-breaking)
   - `Transfer`
   - `Liquidate`
   - `Sweep`
   - `PauseEvent`, `ResumeEvent`, `TerminateEvent`

3. **Implement new effect handlers** in `event_engine.rs`

4. **Add cost basis tracking** to `SimulationState`

5. **Add `end_condition`** to `EventTrigger::Repeating`

6. **Update tests** to use new system (keep old tests working)

7. **Deprecate old effects** (mark with `#[deprecated]`)
   - `TransferAsset` → use `Transfer`
   - `SweepToAccount` → use `Sweep`
   - All CashFlow effects
   - All SpendingTarget effects

8. **Deprecate `CashFlow`** struct and `cash_flows` parameter field

9. **Deprecate `SpendingTarget`** struct and `spending_targets` parameter field

10. **Remove deprecated items** in future version

---

## Appendix A: Complete Type Definitions

```rust
// === model/events.rs ===

/// Specifies how much to transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferAmount {
    Fixed(f64),
    SourceBalance,
    ZeroTargetBalance,
    TargetToBalance(f64),
    AssetBalance { account_id: AccountId, asset_id: AssetId },
    AccountBalance { account_id: AccountId },
    Min(Box<TransferAmount>, Box<TransferAmount>),
    Max(Box<TransferAmount>, Box<TransferAmount>),
    Sub(Box<TransferAmount>, Box<TransferAmount>),
    Add(Box<TransferAmount>, Box<TransferAmount>),
    Mul(Box<TransferAmount>, Box<TransferAmount>),
}

/// Source or destination for a transfer
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransferEndpoint {
    External,
    Asset { account_id: AccountId, asset_id: AssetId },
}

/// Lot selection method for capital gains
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum LotMethod {
    #[default]
    Fifo,
    Lifo,
    HighestCost,
    LowestCost,
    AverageCost,
}

/// Period over which limits reset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitPeriod {
    Yearly,
    Lifetime,
}

/// Limits on cumulative transfer amounts
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FlowLimits {
    pub limit: f64,
    pub period: LimitPeriod,
}

/// Pre-defined withdrawal order strategies
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum WithdrawalOrder {
    #[default]
    TaxEfficientEarly,
    TaxDeferredFirst,
    TaxFreeFirst,
    ProRata,
}

/// Source configuration for Sweep withdrawals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WithdrawalSources {
    Strategy {
        order: WithdrawalOrder,
        #[serde(default)]
        exclude_accounts: Vec<AccountId>,
    },
    Custom(Vec<(AccountId, AssetId)>),
}

impl Default for WithdrawalSources {
    fn default() -> Self {
        WithdrawalSources::Strategy {
            order: WithdrawalOrder::TaxEfficientEarly,
            exclude_accounts: vec![],
        }
    }
}

/// How to interpret the withdrawal amount
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum WithdrawalAmountMode {
    #[default]
    Gross,
    Net,
}
```

---

## Appendix B: Conceptual Model

### Before (Complex)

```
┌─────────────┐     ┌─────────────┐     ┌──────────────┐
│  CashFlow   │     │   Event     │     │SpendingTarget│
│  (income/   │     │  (triggers  │     │ (retirement  │
│   expense)  │     │   actions)  │     │  withdrawals)│
└──────┬──────┘     └──────┬──────┘     └──────┬───────┘
       │                   │                   │
       ▼                   ▼                   ▼
┌─────────────────────────────────────────────────────┐
│              Simulation Engine                      │
│  - apply_cash_flows()                               │
│  - process_events()                                 │
│  - apply_spending_targets()                         │
└─────────────────────────────────────────────────────┘
```

### After (Simple)

```
                    ┌─────────────┐
                    │    Event    │
                    │  (unified)  │
                    └──────┬──────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│              Simulation Engine                      │
│  - process_events()                                 │
│    ├── Transfer (income/expense/moves)              │
│    ├── Liquidate (tax-aware sales)                  │
│    └── Sweep (multi-source withdrawals)             │
└─────────────────────────────────────────────────────┘
```

### Money Flow Model

```
                    ┌──────────────┐
                    │   EXTERNAL   │
                    │    WORLD     │
                    └──────┬───────┘
                           │
            Transfer       │       Transfer
            (Income)       │       (Expense)
                           ▼
                    ┌──────────────┐
                    │     CASH     │
                    │   ACCOUNT    │
                    └──────┬───────┘
                           │
         ┌─────────────────┼─────────────────┐
         │                 │                 │
    Transfer          Transfer          Liquidate
    (Invest)         (Pay Debt)          (Sell)
         │                 │                 │
         ▼                 ▼                 ▼
    ┌─────────┐      ┌─────────┐      ┌─────────┐
    │BROKERAGE│      │MORTGAGE │      │   IRA   │
    │ (stocks)│      │ (debt)  │      │ (bonds) │
    └─────────┘      └─────────┘      └─────────┘
```

All money flows through the Cash account, making the simulation easier to understand and debug.
