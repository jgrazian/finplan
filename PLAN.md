# FinPlan Development Plan

Use this file to track current and future work plans.
Edit as needed to track implementation state, assumptions, and reasoning.

---

## Current Work: Recursive Transfer Amounts in TUI

**Goal:** Integrate the new `InflationAdjusted` and `Scale` transfer amount variants from finplan_core into the TUI.

**Status:** Planning

### Background

The core engine now supports recursive transfer amounts:

```rust
// finplan_core/src/model/events.rs
pub enum TransferAmount {
    Fixed(f64),
    InflationAdjusted(Box<TransferAmount>),  // NEW - recursive
    Scale(f64, Box<TransferAmount>),          // NEW - recursive
    SourceBalance,
    ZeroTargetBalance,
    TargetToBalance(f64),
    AccountTotalBalance { account_id: AccountId },
    AccountCashBalance { account_id: AccountId },
    Min(Box<TransferAmount>, Box<TransferAmount>),
    Max(Box<TransferAmount>, Box<TransferAmount>),
    // ... more arithmetic
}
```

The TUI currently uses a **flat, non-recursive** `AmountData` that cannot represent these new types.

---

## Current Architecture

### TUI Data Layer (`events_data.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AmountData {
    Fixed(f64),
    Special(SpecialAmount),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SpecialAmount {
    SourceBalance,
    ZeroTargetBalance,
    TargetToBalance { target: f64 },
    AccountBalance { account: AccountTag },
    AccountCashBalance { account: AccountTag },
}
```

**Limitations:**
- No recursion - all values are flat
- Cannot represent `InflationAdjusted` or `Scale`
- Cannot compose amounts (e.g., "4% of account balance, inflation-adjusted")

### TUI Form System (`effect.rs`)

- Amount fields displayed as single `Currency` input
- `AmountData::Special(_)` shows as `0.0` and is **lost on save**
- No mechanism to select or configure special amounts
- No recursion support

### Existing Recursive Pattern (`TriggerBuilderState`)

The trigger system already has a recursive builder pattern:
- Parent stack for nested editing
- `push_child()` / `pop_to_parent()` navigation
- Could adapt for amount building

---

## Implementation Plan

### Phase 1: Extend `AmountData` to Support Recursion

**File:** `crates/finplan/src/data/events_data.rs`

Replace the untagged enum with a fully tagged, recursive structure:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AmountData {
    /// Fixed dollar amount
    Fixed { value: f64 },

    /// Inflation-adjusted wrapper (recursive)
    InflationAdjusted { inner: Box<AmountData> },

    /// Scale by multiplier (recursive) - for percentages
    Scale { multiplier: f64, inner: Box<AmountData> },

    // === Simple special amounts (non-recursive) ===
    SourceBalance,
    ZeroTargetBalance,
    TargetToBalance { target: f64 },
    AccountBalance { account: AccountTag },
    AccountCashBalance { account: AccountTag },
}
```

**YAML Examples:**

```yaml
# Fixed amount
amount:
  type: Fixed
  value: 7000.0

# Inflation-adjusted fixed amount
amount:
  type: InflationAdjusted
  inner:
    type: Fixed
    value: 7000.0

# 4% of account balance
amount:
  type: Scale
  multiplier: 0.04
  inner:
    type: AccountBalance
    account: "Brokerage"

# Inflation-adjusted 4% of account balance
amount:
  type: InflationAdjusted
  inner:
    type: Scale
    multiplier: 0.04
    inner:
      type: AccountBalance
      account: "Brokerage"
```

**Tasks:**
- [ ] **1.1** Refactor `AmountData` enum to tagged recursive structure
- [ ] **1.2** Remove `SpecialAmount` enum (fold into `AmountData`)
- [ ] **1.3** Add helper constructors (`AmountData::fixed()`, `AmountData::inflation_adjusted()`, etc.)
- [ ] **1.4** Update all usages of `AmountData::Fixed(f64)` to `AmountData::Fixed { value }`

---

### Phase 2: Update Conversion Layer

**File:** `crates/finplan/src/data/convert.rs`

Update `convert_amount()` to handle recursive variants:

```rust
fn convert_amount(amount: &AmountData, ctx: &ResolveContext) -> TransferAmount {
    match amount {
        AmountData::Fixed { value } => TransferAmount::Fixed(*value),

        AmountData::InflationAdjusted { inner } => {
            TransferAmount::InflationAdjusted(Box::new(convert_amount(inner, ctx)))
        }

        AmountData::Scale { multiplier, inner } => {
            TransferAmount::Scale(*multiplier, Box::new(convert_amount(inner, ctx)))
        }

        AmountData::SourceBalance => TransferAmount::SourceBalance,
        AmountData::ZeroTargetBalance => TransferAmount::ZeroTargetBalance,
        AmountData::TargetToBalance { target } => TransferAmount::TargetToBalance(*target),

        AmountData::AccountBalance { account } => {
            let account_id = ctx.resolve_account(account);
            TransferAmount::AccountTotalBalance { account_id }
        }

        AmountData::AccountCashBalance { account } => {
            let account_id = ctx.resolve_account(account);
            TransferAmount::AccountCashBalance { account_id }
        }
    }
}
```

**Tasks:**
- [ ] **2.1** Update `convert_amount()` with recursive handling
- [ ] **2.2** Fix account resolution (currently uses placeholder `AccountId(0)`)
- [ ] **2.3** Add reverse conversion `core_to_data_amount()` for loading scenarios

---

### Phase 3: Create Amount Builder State

**New file:** `crates/finplan/src/modals/amount_builder.rs`

Create a builder for recursive amount editing (similar to `TriggerBuilderState`):

```rust
/// State for building recursive AmountData expressions
#[derive(Debug, Clone)]
pub struct AmountBuilderState {
    /// The root amount being built
    pub root: AmountData,
    /// Stack of parent paths for nested editing
    pub path: Vec<AmountPath>,
}

#[derive(Debug, Clone)]
pub enum AmountPath {
    /// Inside InflationAdjusted.inner
    InflationAdjustedInner,
    /// Inside Scale.inner
    ScaleInner,
}

impl AmountBuilderState {
    pub fn new(initial: AmountData) -> Self;

    /// Get the currently focused node
    pub fn current(&self) -> &AmountData;

    /// Get mutable reference to current node
    pub fn current_mut(&mut self) -> &mut AmountData;

    /// Wrap current node in InflationAdjusted and descend
    pub fn wrap_inflation_adjusted(&mut self);

    /// Wrap current node in Scale and descend
    pub fn wrap_scale(&mut self, multiplier: f64);

    /// Unwrap current node (remove InflationAdjusted or Scale wrapper)
    pub fn unwrap(&mut self) -> bool;

    /// Navigate up to parent
    pub fn pop(&mut self) -> bool;

    /// Replace current node with new value
    pub fn set_current(&mut self, value: AmountData);

    /// Check if at root level
    pub fn is_at_root(&self) -> bool;

    /// Get human-readable description of current path
    pub fn path_description(&self) -> String;
}
```

**Tasks:**
- [ ] **3.1** Create `amount_builder.rs` module
- [ ] **3.2** Implement `AmountBuilderState` struct
- [ ] **3.3** Implement navigation methods (`current`, `pop`, path traversal)
- [ ] **3.4** Implement mutation methods (`wrap_*`, `unwrap`, `set_current`)
- [ ] **3.5** Add unit tests for builder operations

---

### Phase 4: Add Amount Field Type to Forms

**File:** `crates/finplan/src/modals/state.rs`

Add new `FieldType` variant:

```rust
pub enum FieldType {
    Text,
    Currency,
    Percentage,
    ReadOnly,
    Select,
    /// Complex amount with recursive structure
    Amount(Box<AmountData>),
}
```

**File:** `crates/finplan/src/modals/form.rs`

Render Amount fields with summary display:

```rust
// Example renders:
// "$7,000.00"
// "$7,000.00 (inflation-adjusted)"
// "4% of Brokerage balance"
// "4% of Brokerage balance (inflation-adjusted)"

fn render_amount_summary(amount: &AmountData) -> String {
    match amount {
        AmountData::Fixed { value } => format!("${:.2}", value),
        AmountData::InflationAdjusted { inner } => {
            format!("{} (inflation-adjusted)", render_amount_summary(inner))
        }
        AmountData::Scale { multiplier, inner } => {
            format!("{}% of {}", multiplier * 100.0, render_amount_base(inner))
        }
        AmountData::SourceBalance => "Source balance".to_string(),
        AmountData::ZeroTargetBalance => "Zero target balance".to_string(),
        AmountData::TargetToBalance { target } => format!("To ${:.2}", target),
        AmountData::AccountBalance { account } => format!("{} balance", account.0),
        AmountData::AccountCashBalance { account } => format!("{} cash", account.0),
    }
}
```

**Tasks:**
- [ ] **4.1** Add `Amount(Box<AmountData>)` variant to `FieldType`
- [ ] **4.2** Add `FormField::amount()` constructor
- [ ] **4.3** Implement `render_amount_summary()` for display
- [ ] **4.4** Implement `get_amount()` extraction method on `FormModal`

---

### Phase 5: Create Amount Editor Modal

**File:** `crates/finplan/src/actions/amount.rs` (new)

Create the modal flow for editing amounts:

#### Stage 1: Amount Type Picker

When user activates an Amount field, show picker:

```
┌─ Select Amount Type ─────────────┐
│ > Fixed Amount ($X)              │
│   Inflation-Adjusted             │
│   Percentage of Account          │
│   ───────────────────────        │
│   Source Balance                 │
│   Zero Target Balance            │
│   Target To Balance              │
│   Account Balance                │
│   Account Cash Balance           │
└──────────────────────────────────┘
```

#### Stage 2: Configure Selected Type

**Fixed Amount:**
```
┌─ Fixed Amount ───────────────────┐
│ Amount: $[7,000.00    ]          │
│                                  │
│ [F10] Save   [Esc] Cancel        │
└──────────────────────────────────┘
```

**Inflation-Adjusted:** (shows nested inner amount)
```
┌─ Inflation-Adjusted Amount ──────┐
│ Adjusts for inflation over time  │
│                                  │
│ Base amount (in today's $):      │
│ ┌────────────────────────────┐   │
│ │ Fixed: $7,000.00     [Edit]│   │
│ └────────────────────────────┘   │
│                                  │
│ [F10] Save   [Esc] Cancel        │
└──────────────────────────────────┘
```

**Percentage (Scale):**
```
┌─ Percentage Amount ──────────────┐
│ Percentage: [4.0    ]%           │
│                                  │
│ Of:                              │
│ ┌────────────────────────────┐   │
│ │ Account Balance: Brokerage │   │
│ │                      [Edit]│   │
│ └────────────────────────────┘   │
│                                  │
│ [F10] Save   [Esc] Cancel        │
└──────────────────────────────────┘
```

**[Edit] action** pushes onto the builder stack and shows nested picker.

**Tasks:**
- [ ] **5.1** Create `actions/amount.rs` module
- [ ] **5.2** Implement `handle_amount_type_picker()` - initial type selection
- [ ] **5.3** Implement `handle_amount_fixed_form()` - configure fixed amount
- [ ] **5.4** Implement `handle_amount_inflation_form()` - configure with nested editing
- [ ] **5.5** Implement `handle_amount_scale_form()` - configure percentage with nested
- [ ] **5.6** Implement `handle_amount_account_picker()` - select account reference
- [ ] **5.7** Add `AmountContext` to `ModalContext` for tracking builder state
- [ ] **5.8** Wire up [Edit] action to push/pop builder stack

---

### Phase 6: Integrate with Effect Forms

**File:** `crates/finplan/src/actions/effect.rs`

Update all effect forms to use new Amount field type:

```rust
// Before:
FormField::currency("Amount", amount_to_f64(&effect.amount))

// After:
FormField::amount("Amount", effect.amount.clone())
```

Update extraction in `handle_add_effect()` and `handle_edit_effect()`:

```rust
// Before:
let amount = form.get_currency(1).unwrap_or(0.0);
EffectData::Expense {
    amount: AmountData::Fixed(amount),
    ...
}

// After:
let amount = form.get_amount(1)
    .unwrap_or(AmountData::Fixed { value: 0.0 });
EffectData::Expense {
    amount,
    ...
}
```

**Effects that use AmountData:**
- Income
- Expense
- AssetPurchase
- AssetSale
- Sweep
- AdjustBalance
- CashTransfer

**Tasks:**
- [ ] **6.1** Update `build_edit_form_for_effect()` to use `FormField::amount()`
- [ ] **6.2** Update `handle_effect_type_for_add()` to initialize with Amount fields
- [ ] **6.3** Update `handle_add_effect()` to extract `AmountData`
- [ ] **6.4** Update `handle_edit_effect()` to extract `AmountData`
- [ ] **6.5** Remove `amount_to_f64()` helper (no longer needed)
- [ ] **6.6** Handle Amount field activation in form key handler

---

### Phase 7: Handle Modal Interactions

**File:** `crates/finplan/src/modals/mod.rs`

Add handling for Amount field editing:

```rust
// When user presses Enter on an Amount field:
// 1. Store current form state
// 2. Create AmountBuilderState from field value
// 3. Show amount type picker
// 4. On completion, update field and restore form

pub fn handle_amount_field_edit(
    state: &mut AppState,
    form_idx: usize,
    field_idx: usize,
) -> ActionResult {
    let form = /* get current form */;
    let current_amount = form.get_amount(field_idx)?;

    // Store form context for restoration
    let context = AmountEditContext {
        form_state: form.clone(),
        field_idx,
        builder: AmountBuilderState::new(current_amount),
    };

    // Show type picker
    ActionResult::modal(ModalState::Picker(
        PickerModal::new("Select Amount Type", amount_type_options())
            .with_context(ModalContext::AmountEdit(context))
    ))
}
```

**Tasks:**
- [ ] **7.1** Add `AmountEditContext` to context system
- [ ] **7.2** Handle Enter key on Amount fields to launch editor
- [ ] **7.3** Handle amount picker completion
- [ ] **7.4** Handle nested amount form completion
- [ ] **7.5** Restore parent form with updated amount value
- [ ] **7.6** Handle Esc to cancel and restore original value

---

## Alternative: Simpler Flat Approach

If full recursion proves too complex for the TUI, a simpler approach:

### Add Flags Instead of Nesting

```rust
pub struct AmountConfig {
    pub base: AmountBase,
    pub inflation_adjusted: bool,
    pub as_percentage: Option<f64>,  // None = absolute, Some(0.04) = 4%
}

pub enum AmountBase {
    Fixed(f64),
    SourceBalance,
    AccountBalance { account: AccountTag },
    // etc.
}
```

### Form Display

```
Amount: $[7,000.00]
[x] Inflation-adjusted
[ ] As percentage: [    ]%
```

**Tradeoffs:**
- Simpler UI (checkboxes instead of nested modals)
- Covers 90% of use cases
- Loses ability to compose arbitrary expressions
- Cannot do "percentage of (inflation-adjusted fixed amount)"

---

## Implementation Order

| Step | Files | Effort | Description |
|------|-------|--------|-------------|
| 1 | `events_data.rs` | Medium | Refactor AmountData to recursive tagged enum |
| 2 | `convert.rs` | Small | Update conversion for new variants |
| 3 | `amount_builder.rs` | Medium | Create builder state for recursive editing |
| 4 | `state.rs`, `form.rs` | Medium | Add Amount field type and rendering |
| 5 | `amount.rs` | Large | Create amount editor modal flow |
| 6 | `effect.rs` | Medium | Update all effect forms |
| 7 | `mod.rs` | Medium | Wire up modal interactions |
| - | Tests | Medium | Unit tests for builder, integration tests |

**Estimated total:** ~800-1200 lines of new/modified code

---

## Open Questions

1. **Nested depth limit?** Should we limit recursion to prevent overly complex amounts?
   - Recommendation: Allow 2-3 levels max (e.g., InflationAdjusted → Scale → AccountBalance)

2. **YAML backwards compatibility?** Old scenarios use `amount: 5000.0` (untagged).
   - Option A: Support both during loading, always save as tagged
   - Option B: Migration script to convert old scenarios
   - Recommendation: Option A for user convenience

3. **Arithmetic operations?** Should TUI support Min/Max/Add/Sub/Mul?
   - Recommendation: Defer to future phase; Scale + InflationAdjusted cover most needs

4. **Form field display width?** Long amount summaries may overflow.
   - Recommendation: Truncate with "..." and show full on focus/hover

---

## Success Criteria

- [ ] Users can create inflation-adjusted expenses in the TUI
- [ ] Users can create percentage-based withdrawals (e.g., 4% rule)
- [ ] Existing scenarios load correctly (backwards compatible)
- [ ] New scenarios save in recursive YAML format
- [ ] Amount editor is intuitive (no more than 3 clicks for common cases)
- [ ] All 7 effect types support the new amount fields
