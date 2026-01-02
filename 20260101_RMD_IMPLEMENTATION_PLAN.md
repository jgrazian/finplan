# Required Minimum Distribution (RMD) Implementation Plan

## Overview

Add automatic Required Minimum Distribution calculations and withdrawals for tax-deferred retirement accounts (Traditional IRA, 401k) based on IRS life expectancy tables, starting at age 73 with annual repetition.

## Implementation Steps

### 1. Add RMD Life Expectancy Table Data Structure

**File:** `crates/finplan/src/models.rs`

- Create IRS Uniform Lifetime Table data structure (ages 73-120+)
- Store divisor values for calculating annual RMD percentages
- Table should be easily updatable as IRS rules change

```rust
pub struct RmdTable {
    pub entries: Vec<RmdTableEntry>,
}

pub struct RmdTableEntry {
    pub age: u8,
    pub divisor: f64,
}
```

**IRS Uniform Lifetime Table (2024):**
- Age 73: 26.5
- Age 74: 25.5
- Age 75: 24.6
- Age 76: 23.7
- ... (continues to 120+)

### 2. Create New EventEffect::CreateRmdWithdrawal Variant

**File:** `crates/finplan/src/models.rs`

Add new event effect variant that allows automatic RMD calculation:

```rust
pub enum EventEffect {
    // ... existing variants
    CreateRmdWithdrawal {
        account_id: AccountId,
        starting_age: u8,  // 73 or 75 based on birth year
    },
}
```

This allows events to trigger calculated RMD withdrawals rather than hardcoded amounts.

### 3. Implement RMD Calculation Logic

**File:** `crates/finplan/src/event_engine.rs`

Within `apply_event_effect()`, add handling for the new effect:

**Logic:**
1. Query prior year-end account balance
2. Get current age from `state.current_age()`
3. Look up IRS divisor for current age
4. Calculate RMD amount: `balance / divisor`
5. Create or modify a `SpendingTarget` with calculated amount
6. Use `WithdrawalStrategy::TaxOptimized` 
7. Filter to only the specific tax-deferred account (via `exclude_accounts`)
8. Set `net_amount_mode: false` (gross amount)
9. Set `adjust_for_inflation: false` (RMD is based on actual balance)
10. Record RMD creation in event history

### 4. Add RMD Tracking to SimulationState

**File:** `crates/finplan/src/simulation_state.rs`

Add new fields:

```rust
pub struct SimulationState {
    // ... existing fields
    
    /// Year-end account balances for RMD calculation (year -> account_id -> balance)
    pub year_end_balances: HashMap<i16, HashMap<AccountId, f64>>,
    
    /// Active RMD accounts (account_id -> starting_age)
    pub active_rmd_accounts: HashMap<AccountId, u8>,
    
    /// RMD-specific withdrawal history
    pub rmd_history: Vec<RmdRecord>,
}

#[derive(Debug, Clone)]
pub struct RmdRecord {
    pub date: jiff::civil::Date,
    pub account_id: AccountId,
    pub age: u8,
    pub prior_year_balance: f64,
    pub irs_divisor: f64,
    pub required_amount: f64,
    pub actual_withdrawn: f64,
    pub spending_target_id: SpendingTargetId,
}
```

### 5. Create Helper Functions

**File:** `crates/finplan/src/simulation_state.rs`

```rust
impl SimulationState {
    /// Get prior year-end balance for an account
    pub fn prior_year_end_balance(&self, account_id: AccountId) -> Option<f64> {
        let prior_year = self.current_date.year() - 1;
        self.year_end_balances
            .get(&prior_year)?
            .get(&account_id)
            .copied()
    }
    
    /// Get IRS divisor for current age
    pub fn current_rmd_divisor(&self, rmd_table: &RmdTable) -> Option<f64> {
        let (years, _months) = self.current_age()?;
        rmd_table.entries
            .iter()
            .find(|e| e.age == years)
            .map(|e| e.divisor)
    }
    
    /// Calculate RMD amount for an account
    pub fn calculate_rmd_amount(
        &self, 
        account_id: AccountId, 
        rmd_table: &RmdTable
    ) -> Option<f64> {
        let balance = self.prior_year_end_balance(account_id)?;
        let divisor = self.current_rmd_divisor(rmd_table)?;
        Some(balance / divisor)
    }
}
```

### 6. Update Year-End Rollover Logic

**File:** `crates/finplan/src/simulation.rs`

In the main simulation loop, capture account balances at December 31:

```rust
// Check if we're at year-end (December 31)
if state.current_date.month() == 12 && state.current_date.day() == 31 {
    let year = state.current_date.year();
    let mut year_balances = HashMap::new();
    
    for (account_id, account) in &state.accounts {
        if matches!(account.account_type, AccountType::TaxDeferred) {
            let balance = state.account_balance(*account_id);
            year_balances.insert(*account_id, balance);
        }
    }
    
    state.year_end_balances.insert(year, year_balances);
}
```

## Usage Example

To enable RMDs for a Traditional IRA:

```rust
Event {
    event_id: EventId(10),
    trigger: EventTrigger::Repeating {
        interval: RepeatInterval::Yearly,
        start_condition: Some(Box::new(EventTrigger::Age { 
            years: 73, 
            months: Some(0)  // First RMD year
        })),
    },
    effects: vec![
        EventEffect::CreateRmdWithdrawal {
            account_id: AccountId(1),  // Traditional IRA
            starting_age: 73,
        }
    ],
    once: false,
}
```

## Key Design Decisions

### Calculation Timing
- RMDs are calculated based on **prior year-end balance** (December 31)
- First RMD is due by April 1 following the year you turn 73
- Subsequent RMDs due by December 31 each year
- **Simplification**: Calculate and withdraw on January 1 (or at trigger date) using prior year-end balance

### Account Identification
- RMDs apply only to `AccountType::TaxDeferred` accounts
- Each account is tracked separately for RMD purposes
- User must explicitly enable RMD via event trigger

### Tax Treatment
- RMD withdrawals are treated as ordinary income
- Full withdrawal amount adds to YTD ordinary income
- Marginal tax rates apply based on other income
- Uses existing `calculate_withdrawal_tax()` infrastructure

### Withdrawal Strategy
- Default to `WithdrawalStrategy::Sequential` with single account
- For multiple tax-deferred accounts, can specify order
- System creates/modifies spending target for actual withdrawal

## Further Considerations

### 1. Multiple Account Handling

**Current IRS Rules:**
- **IRAs**: Can aggregate RMDs across all Traditional IRAs and withdraw total from any combination
- **401(k)s**: Must calculate and withdraw separately from each 401(k)

**Implementation Options:**

**Option A: Per-Account (Simpler, Current Plan)**
- Each account gets separate RMD event
- Withdrawal comes only from that account
- User creates multiple events if multiple accounts

**Option B: Aggregated (More Complex)**
- Add `account_ids: Vec<AccountId>` to effect
- Sum all prior-year balances
- Calculate aggregate RMD
- Use TaxOptimized strategy across all accounts
- Track which accounts were included

**Recommendation:** Start with Option A (per-account), add Option B if needed.

### 2. Starting Age Flexibility

**Current Law (2024):**
- Born 1951-1959: RMDs start at age 73
- Born 1960 or later: RMDs start at age 75
- SECURE 2.0 Act changed from previous age 72

**Implementation Options:**

**Option A: User-Specified (Current Plan)**
- User specifies `starting_age` in event
- Flexible for different scenarios
- Requires user to know correct age

**Option B: Birth-Year Based**
- System calculates based on birth date
- Add `rmd_birth_year_rules: Vec<(i16, u8)>` to config
- Automatic but less flexible

**Recommendation:** Start with Option A (user-specified), can add B as helper.

### 3. Inherited IRA Support

**Different Rules:**
- Non-spouse beneficiaries: 10-year rule (must withdraw all by year 10)
- Spouse beneficiaries: Can treat as own or use life expectancy
- Different RMD calculations

**Recommendation:** Defer to future enhancement. Current implementation handles only owner IRAs.

### 4. RMD Shortfall Penalties

**IRS Penalty:** 25% excise tax on amount not withdrawn (reduced to 10% if corrected within 2 years)

**Implementation:** 
- Track required vs. actual withdrawn in `RmdRecord`
- Could add penalty calculation if shortfall detected
- **Recommendation:** Phase 2 enhancement

### 5. First-Year RMD Special Rule

**IRS Rule:** First RMD can be delayed until April 1 of following year

**Implementation:**
- Use `EventTrigger::Age { years: 73, months: Some(3) }` for April 1 timing
- Or keep January timing as simplification
- **Recommendation:** Document but allow user flexibility via trigger date

### 6. Roth 401(k) Consideration

**Note:** Roth 401(k)s currently require RMDs (unlike Roth IRAs), but SECURE 2.0 eliminates this starting 2024.

**Implementation:** 
- If account is `AccountType::TaxDeferred`, apply RMD
- If account is `AccountType::TaxFree`, skip RMD
- Correctly handles both rules

## Testing Strategy

### Unit Tests

1. **IRS Table Lookup**
   - Test divisor lookup for various ages (73, 80, 90, 100, 120+)
   - Test edge cases (age too young, age too old)

2. **RMD Calculation**
   - Test basic calculation: balance / divisor
   - Test with various balances ($100k, $1M, $10M)
   - Test with missing prior-year balance

3. **Year-End Balance Capture**
   - Verify balances recorded at Dec 31
   - Verify only TaxDeferred accounts included
   - Verify multiple years accumulate correctly

### Integration Tests

1. **Full RMD Lifecycle**
   - Create person born 1951 (age 73 in 2024)
   - Traditional IRA with $1M balance
   - Run simulation through ages 73-83
   - Verify annual RMD withdrawals occur
   - Verify amounts decrease as balance depletes
   - Verify correct tax treatment

2. **Multiple Accounts**
   - Two Traditional IRAs
   - Separate RMD events
   - Verify independent calculation and withdrawal

3. **RMD + Other Income**
   - Social Security income
   - RMD withdrawals
   - Verify tax interactions (marginal rates)

4. **Edge Cases**
   - Account depleted before RMD due
   - Balance grows faster than withdrawals
   - Year-end on weekend/holiday (date handling)

## Implementation Checklist

- [ ] Add `RmdTable` and `RmdTableEntry` structs to models.rs
- [ ] Populate IRS Uniform Lifetime Table (ages 73-120+)
- [ ] Add `EventEffect::CreateRmdWithdrawal` variant
- [ ] Add RMD tracking fields to `SimulationState`
- [ ] Implement `prior_year_end_balance()` helper
- [ ] Implement `current_rmd_divisor()` helper
- [ ] Implement `calculate_rmd_amount()` helper
- [ ] Add RMD effect handling in `apply_event_effect()`
- [ ] Add year-end balance capture in simulation loop
- [ ] Write unit tests for RMD calculations
- [ ] Write integration tests for RMD lifecycle
- [ ] Update documentation with RMD examples
- [ ] Add RMD to example simulations

## Documentation Updates

- [ ] Add RMD section to README
- [ ] Document RMD event pattern
- [ ] Add example retirement plan with RMDs
- [ ] Document IRS table and update process
- [ ] Note simplifications vs. actual IRS rules

## Future Enhancements

1. **Automatic RMD Profile**
   - High-level "enable RMDs" flag on account
   - System auto-generates event based on birth year
   
2. **Inherited IRA Support**
   - 10-year rule implementation
   - Beneficiary designation tracking

3. **RMD Penalty Tracking**
   - Calculate 25% penalty for shortfalls
   - Add to tax summary

4. **Qualified Charitable Distribution (QCD)**
   - Allow RMD to go directly to charity
   - Exclude from taxable income
   - Up to $105k limit (2024)

5. **Still-Working Exception**
   - Delay RMDs if still employed
   - Track employment status

6. **Account Aggregation UI**
   - Helper to aggregate multiple IRA RMDs
   - Optimize withdrawal order
