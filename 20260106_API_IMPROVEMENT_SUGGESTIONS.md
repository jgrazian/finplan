# SimulationConfig API Improvement Suggestions

**Date:** January 6, 2026

After working extensively with the `finplan` codebase and setting up test simulations with the `SimulationConfig` struct, here are observations and suggestions for improving simulation fidelity, flexibility, and usability.

---

## Strengths

### Fidelity
- **Tax modeling is solid**: Federal brackets, state rates, short/long-term capital gains distinction, marginal tax calculations
- **Lot tracking is accurate**: FIFO, LIFO, highest cost, average cost methods all implemented
- **Immutable ledger system**: Every state change is traceable with source event attribution

### Flexibility
- **Event system is powerful**: Date triggers, once/repeating, pause/resume/terminate
- **Multiple account types**: Investment, Bank, Property, Liability with appropriate tax treatment
- **WithdrawalSources**: Allows complex withdrawal strategies (single source, ordered list, proportional)

---

## Pain Points

### 1. Too Much Boilerplate

Every simulation setup requires extensive ceremony:

```rust
let params = SimulationConfig {
    start_date: Some(start_date),
    duration_years: 2,
    birth_date: None,
    inflation_profile: InflationProfile::None,
    return_profiles: HashMap::from([(return_profile, ReturnProfile::Fixed(0.10))]),
    asset_returns: HashMap::from([(asset_id, return_profile)]),
    asset_prices: HashMap::from([(asset_id, 100.0)]),
    accounts: vec![...],
    events: vec![...],
    ..Default::default()
};
```

### 2. ID Indirection is Confusing

- `ReturnProfileId`, `AssetId`, `AccountId`, `EventId` are all `(u32)` wrappers
- Easy to accidentally use wrong ID type (caught at compile time, but error messages aren't helpful)
- Assets need to be registered in 3 places: `asset_returns`, `asset_prices`, and often `positions`

### 3. Missing Convenience Features

- No `InflationAdjusted(amount)` wrapper for expenses that grow over time
- No age-based triggers (retire at 65, RMDs at 73)
- No balance-based triggers ("when account X < $10k, trigger Y")
- No "waterfall" withdrawal strategy ("drain taxable first, then tax-deferred, then Roth")

### 4. Conceptual Clarity Issues

- `AmountMode::Gross` vs `Net` semantics differ between Income and AssetSale - could be confusing
- `TransferAmount::PercentOfSource` - percent of what exactly? Current balance? Annual amount?

---

## Suggestions

### 1. Builder Pattern for SimulationConfig

```rust
SimulationConfig::builder()
    .start(2020, 1, 1)
    .years(5)
    .birth_date(1960, 5, 15)
    .asset("VTSAX", 100.0, ReturnProfile::Fixed(0.10))
    .asset("BND", 50.0, ReturnProfile::Fixed(0.04))
    .account(Account::brokerage("Main").with_cash(10_000.0))
    .account(Account::traditional_401k("Work 401k").with_asset("VTSAX", 50_000.0))
    .event(Event::income("Salary").to("Main").amount(100_000.0).monthly())
    .event(Event::expense("Rent").from("Main").amount(2_000.0).monthly().inflation_adjusted())
    .build()
```

### 2. Named Assets Instead of IDs

Use strings internally mapped to IDs:

```rust
// Instead of:
let asset_id = AssetId(100);
asset_returns.insert(asset_id, return_profile_id);
asset_prices.insert(asset_id, 100.0);

// Use:
config.asset("VTSAX", 100.0, ReturnProfile::Fixed(0.10))
```

### 3. Preset Account Types

```rust
impl Account {
    pub fn traditional_401k(name: &str) -> AccountBuilder { ... }
    pub fn roth_ira(name: &str) -> AccountBuilder { ... }
    pub fn roth_401k(name: &str) -> AccountBuilder { ... }
    pub fn taxable_brokerage(name: &str) -> AccountBuilder { ... }
    pub fn hsa(name: &str) -> AccountBuilder { ... }
    pub fn savings(name: &str) -> AccountBuilder { ... }
}
```

### 4. Event DSL

```rust
Event::expense("Rent")
    .from("Checking")
    .amount(2_000.0)
    .monthly()
    .on_day(1)
    .inflation_adjusted()
    .until(EventTrigger::Age(65))

Event::income("Salary")
    .to("Checking") 
    .amount(150_000.0)
    .gross()
    .taxable()
    .annually()
    .until(EventTrigger::Age(65))

Event::withdrawal("Retirement Income")
    .to("Checking")
    .amount(80_000.0)
    .net()
    .from_waterfall(["Taxable", "Traditional 401k", "Roth IRA"])
    .starting(EventTrigger::Age(65))
```

### 5. Age-Aware Triggers

```rust
pub enum EventTrigger {
    Date(Date),
    Age(u8),                          // When person reaches age
    AgeRange { start: u8, end: u8 },  // Between ages (for RMDs)
    AccountBalance {                   // When account crosses threshold
        account: AccountId,
        condition: BalanceCondition,
    },
    EventFired(EventId),              // After another event fires
}

pub enum BalanceCondition {
    Below(f64),
    Above(f64),
    Depleted,
}
```

### 6. Inflation-Adjusted Amounts

```rust
pub enum TransferAmount {
    Fixed(f64),
    InflationAdjusted { base: f64, start_year: i32 },
    PercentOfBalance { account: AccountId, percent: f64 },
    PercentOfIncome { percent: f64 },  // For savings rate
    Rmd { account: AccountId },         // Calculate RMD automatically
}
```

### 7. Withdrawal Waterfall Strategy

```rust
pub enum WithdrawalSources {
    Single { asset_coord: AssetCoord },
    Ordered(Vec<AssetCoord>),
    Proportional(Vec<(AssetCoord, f64)>),
    // NEW:
    Waterfall {
        order: Vec<AccountId>,  // Drain in order
        preserve_emergency_fund: Option<f64>,
    },
    TaxOptimized {
        accounts: Vec<AccountId>,
        target_tax_bracket: Option<f64>,
    },
}
```

---

## Implementation Priority

1. **High Impact, Lower Effort:**
   - Builder pattern for `SimulationConfig`
   - Named assets (string-based lookup)
   - `EventTrigger::Age(u8)`

2. **High Impact, Medium Effort:**
   - Preset account types
   - `TransferAmount::InflationAdjusted`
   - Event builder DSL

3. **High Impact, Higher Effort:**
   - Withdrawal waterfall strategy
   - Balance-based triggers
   - Tax-optimized withdrawal

---

## Notes

The current test file ([simulation_result.rs](crates/finplan/src/tests/simulation_result.rs)) demonstrates the verbosity issue well - each test requires 30-50 lines just for setup. A builder pattern would reduce this to 5-10 lines while improving readability.
