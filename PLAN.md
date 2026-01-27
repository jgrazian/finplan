Use this file as a store of current and future plans for the repo.
Edit the file as needed to track the implementation state, assumptions and reasoning about feature implementation.
When work is complete make sure to update the state of PLAN.md.

--

# Performance Optimization Plan for finplan_core

## Performance Profile Summary

Profiling data from `perf.data` (52K samples, ~242B cycles) reveals the following hotspots:

| Function | % Time | Root Cause |
|----------|--------|------------|
| `evaluate_effect` | 17.76% | Large match statement, recursive calls |
| `EventTrigger::clone` | 13.52% | Excessive cloning in process_events |
| `process_events` | 10.52% | Main loop overhead, cloning |
| `Vec::clone` | 9.93% | Cloning collections |
| `Vec::from_iter` | 9.84% | Collecting into new Vecs |
| `evaluate_trigger` | 8.41% | Trigger evaluation + Vec allocations |
| `simulate` | 4.94% | Main loop |
| `malloc + free` | ~5% | Memory allocation overhead |
| `drop_in_place<EventTrigger>` | 1.91% | Dropping cloned triggers |

**Key Insight:** ~35% of runtime is spent on cloning and memory allocation. The hottest paths are:
- `process_events` clones `EventTrigger` for every event check (line 490)
- `evaluate_effect` allocates new `Vec<EvalEvent>` for every effect
- And/Or triggers collect into temporary Vecs before checking

---

## Optimization Plan

### Phase 1: Eliminate Unnecessary Cloning (Target: 15-20% improvement)

#### 1.1 Refactor process_events to avoid trigger cloning

**Current code (apply.rs:487-492):**
```rust
let trigger = match state.event_state.events.get(&event_id) {
    Some(event) => event.trigger.clone(),  // CLONE HERE
    None => continue,
};
let trigger_result = evaluate_trigger(&event_id, &trigger, state);
```

**Proposed fix:** Keep borrow alive instead of cloning:
```rust
// Get trigger reference without cloning
let trigger_result = {
    let trigger = match state.event_state.events.get(&event_id) {
        Some(event) => &event.trigger,
        None => continue,
    };
    evaluate_trigger(&event_id, trigger, state)
};
```

**Files to modify:**
- `crates/finplan_core/src/apply.rs`

#### 1.2 Avoid cloning effects vector when event triggers

**Current code (apply.rs:547-550):**
```rust
let effects = match state.event_state.events.get(&event_id) {
    Some(event) => event.effects.clone(),  // CLONE HERE
    None => continue,
};
```

**Proposed fix:** Process effects inline with temporary borrow:
```rust
// Get effects length first, then iterate by index
let effects_len = state.event_state.events.get(&event_id)
    .map(|e| e.effects.len())
    .unwrap_or(0);

for i in 0..effects_len {
    let effect = match state.event_state.events.get(&event_id) {
        Some(event) => event.effects.get(i).cloned(),
        None => break,
    };
    // ... evaluate effect
}
```

Or better - refactor to use indices:
```rust
// Collect effect indices, then process
for effect_idx in 0..effects_len {
    // Re-borrow events to get each effect
    let Some(effect) = state.event_state.events.get(&event_id)
        .and_then(|e| e.effects.get(effect_idx)) else { break };

    match evaluate_effect(effect, state) {
        // ...
    }
}
```

#### 1.3 Remove full Event clone in pending_triggers loop

**Current code (apply.rs:589):**
```rust
if let Some(event) = state.event_state.events.get(&event_id).cloned() {
```

**Proposed fix:** Split into separate borrows like 1.1/1.2.

---

### Phase 2: Reduce Allocations in evaluate_trigger (Target: 5-8% improvement)

#### 2.1 Short-circuit And/Or evaluation without collecting

**Current code (evaluate.rs:189-201):**
```rust
EventTrigger::And(triggers) => {
    let results: Vec<bool> = triggers
        .iter()
        .map(|t| evaluate_trigger(event_id, t, state)
            .map(|eval| matches!(eval, TriggerEvent::Triggered)))
        .collect::<Result<Vec<bool>, _>>()?;
    Ok(if results.into_iter().all(|b| b) {
        TriggerEvent::Triggered
    } else {
        TriggerEvent::NotTriggered
    })
}
```

**Proposed fix:** Short-circuit without allocation:
```rust
EventTrigger::And(triggers) => {
    for t in triggers {
        match evaluate_trigger(event_id, t, state)? {
            TriggerEvent::Triggered => continue,
            _ => return Ok(TriggerEvent::NotTriggered),
        }
    }
    Ok(TriggerEvent::Triggered)
}

EventTrigger::Or(triggers) => {
    for t in triggers {
        if matches!(evaluate_trigger(event_id, t, state)?, TriggerEvent::Triggered) {
            return Ok(TriggerEvent::Triggered);
        }
    }
    Ok(TriggerEvent::NotTriggered)
}
```

---

### Phase 3: Scratch Buffer Pattern (Target: 5-10% improvement)

#### 3.1 Use scratch Vec for evaluate_effect results

Instead of SmallVec (which adds a dependency and still allocates when > N items), use a **scratch buffer** pattern. A single Vec is reused across all calls, achieving zero allocations after warmup.

**Current signature:**
```rust
pub fn evaluate_effect(
    effect: &EventEffect,
    state: &SimulationState,
) -> Result<Vec<EvalEvent>, StateEventError>
```

**Proposed signature:**
```rust
pub fn evaluate_effect_into(
    effect: &EventEffect,
    state: &SimulationState,
    out: &mut Vec<EvalEvent>,  // Scratch buffer, cleared before use
) -> Result<(), StateEventError>
```

**Caller side (process_events):**
```rust
pub fn process_events(state: &mut SimulationState) -> Vec<EventId> {
    let mut triggered = Vec::new();
    let mut eval_scratch = Vec::with_capacity(8);  // Reused across all effects

    for event_id in event_ids_to_check {
        // ... trigger evaluation ...

        for effect_idx in 0..effects_len {
            eval_scratch.clear();  // Reset for reuse

            let Some(effect) = state.event_state.events.get(&event_id)
                .and_then(|e| e.effects.get(effect_idx)) else { break };

            if let Ok(()) = evaluate_effect_into(effect, state, &mut eval_scratch) {
                for ee in eval_scratch.drain(..) {
                    apply_eval_event_with_source(state, &ee, Some(event_id))?;
                }
            }
        }
    }
    triggered
}
```

**Benefits over SmallVec:**
- No dependency
- Zero allocations after first use (SmallVec still allocates when exceeding inline capacity)
- Vec capacity grows to accommodate worst case, then stays there

#### 3.2 Scratch buffer for nested evaluate_effect calls

`evaluate_effect` for `Sweep` recursively calls itself for `AssetSale`. Thread a scratch buffer through:

```rust
pub fn evaluate_effect_into(
    effect: &EventEffect,
    state: &SimulationState,
    out: &mut Vec<EvalEvent>,
    nested_scratch: &mut Vec<EvalEvent>,  // For recursive calls
) -> Result<(), StateEventError>
```

Or use a `ScratchBuffers` struct:
```rust
pub struct ScratchBuffers {
    pub eval_events: Vec<EvalEvent>,
    pub nested_eval_events: Vec<EvalEvent>,
}

impl ScratchBuffers {
    pub fn new() -> Self {
        Self {
            eval_events: Vec::with_capacity(8),
            nested_eval_events: Vec::with_capacity(16),
        }
    }
}
```

#### 3.3 Scratch buffer in simulate() for triggered event IDs

**Current code (apply.rs:466):**
```rust
pub fn process_events(state: &mut SimulationState) -> Vec<EventId> {
    let mut triggered = Vec::new();  // Allocates every call
```

**Proposed:** Pass scratch from caller:
```rust
pub fn process_events_into(
    state: &mut SimulationState,
    triggered: &mut Vec<EventId>,
) {
    triggered.clear();
    // ...
}
```

In `simulate()`:
```rust
let mut triggered_scratch = Vec::with_capacity(16);
let mut eval_scratch = ScratchBuffers::new();

while state.timeline.current_date < state.timeline.end_date {
    while something_happened {
        triggered_scratch.clear();
        process_events_into(&mut state, &mut triggered_scratch, &mut eval_scratch);
        something_happened = !triggered_scratch.is_empty();
    }
    advance_time(&mut state, params);
}
```

#### 3.4 Pre-allocate internal vectors with capacity hints

**Current code (evaluate.rs:395):**
```rust
let mut effects = vec![];
```

**Proposed fix (if not using scratch pattern for internal vecs):**
```rust
let mut effects = Vec::with_capacity(2); // Most Income effects produce 2 items
```

---

### Phase 4: Structural Optimizations (Target: 3-5% improvement)

#### 4.1 Cache account balance lookups during event processing

Many events check the same account balances. Add a per-timestep cache:

```rust
pub struct SimulationState {
    // ...
    /// Cached balance lookups, cleared each time step
    balance_cache: FxHashMap<AccountId, f64>,
}
```

#### 4.2 Use Cow<'a, EventEffect> for effects that rarely need cloning

For effects like `CreateAccount(Account)`, we currently clone the Account.
Using `Cow` would avoid cloning when processing effects that don't mutate.

---

### Phase 5: Algorithmic Improvements (Target: 2-5% improvement)

#### 5.1 Skip events more aggressively in process_events

**Current approach:** Checks every event every time step.

**Optimization:** Maintain a sorted list of "next trigger date" for date-based events:
```rust
pub struct SimEventState {
    // ...
    /// Events sorted by next possible trigger date
    events_by_next_date: BTreeMap<Date, Vec<EventId>>,
}
```

Only check events whose `next_date <= current_date`.

#### 5.2 Batch market price lookups

`get_asset_value` is called repeatedly with similar parameters. Cache per time-step.

---

## Implementation Order

| Priority | Task | Est. Improvement | Complexity |
|----------|------|------------------|------------|
| 1 | 1.1 - Avoid trigger cloning | 8-10% | Low |
| 2 | 1.2 - Avoid effects cloning | 5-7% | Medium |
| 3 | 2.1 - Short-circuit And/Or | 3-5% | Low |
| 4 | 3.1 - Scratch Vec for eval_effect | 5-8% | Medium |
| 5 | 3.3 - Scratch for triggered IDs | 1-2% | Low |
| 6 | 1.3 - Fix pending_triggers clone | 2-3% | Low |
| 7 | 3.2 - Nested scratch for Sweep | 1-2% | Medium |
| 8 | 4.1 - Balance cache | 1-2% | Medium |
| 9 | 5.1 - Event date indexing | 2-3% | High |

**Total expected improvement: 25-40%**

**Note:** The scratch buffer pattern (Phase 3) is preferred over SmallVec because:
- No external dependency
- Zero allocations after warmup (SmallVec still heap-allocates when > inline capacity)
- Vec capacity grows to worst-case and stays, amortizing all future calls

---

## Validation Strategy

1. Run existing test suite after each change: `cargo test -p finplan_core`
2. Benchmark before/after using:
   ```bash
   perf record -g --call-graph dwarf ./target/release/finplan
   perf report --stdio
   ```
3. Compare Monte Carlo simulation times for 1000 iterations
4. Ensure no regression in simulation results (deterministic with same seed)

---

## Status

- [x] Phase 1.1 - Avoid trigger cloning
- [x] Phase 1.2 - Avoid effects cloning
- [x] Phase 1.3 - Fix pending_triggers clone
- [x] Phase 2.1 - Short-circuit And/Or
- [x] Phase 3.1 - Scratch Vec for evaluate_effect
- [x] Phase 3.2 - Nested scratch for Sweep calls (now handled by reusing outer scratch via slicing)
- [x] Phase 3.3 - Scratch for triggered event IDs
- [x] Phase 3.4 - Pre-allocate internal vectors
- [x] Phase 4.1 - Balance cache (DEFERRED - see notes)
- [x] Phase 5.1 - Event date indexing (DEFERRED - see notes)

---

## Notes

- The current code already has some optimizations (e.g., inline dedup in `evaluate_effect`)
- Using `FxHashMap` (rustc-hash) is good - faster than std HashMap
- Rayon parallelization in Monte Carlo is effective
- Consider adding `#[inline]` hints to hot functions if needed
- Scratch buffer pattern is idiomatic Rust for hot loops - see `std::io::Read::read_to_string` for precedent
- For Monte Carlo, each thread can have its own `ScratchBuffers` instance (thread-local or passed per-iteration)

### Phase 4.1 Analysis (Balance Cache - DEFERRED)

After detailed analysis, Phase 4.1 (account balance caching) was deferred due to unfavorable cost-benefit:

**Why caching is complex:**
1. Account balances change frequently during effect application (CashCredit, CashDebit, AddAssetLot, SubtractAssetLot, AdjustBalance)
2. The `process_events` function is called multiple times within the `while something_happened` loop
3. After each effect is applied, balances for affected accounts become stale
4. Every mutation operation (5+ types) would need cache invalidation

**Why benefit is limited:**
1. Within a single trigger/effect evaluation pass, the same account balance is rarely queried multiple times
2. The actual `account_balance()` function is O(n) where n = number of positions in account, but positions are typically small (1-5)
3. The expensive part (market.get_asset_value) is already O(1) with pre-computed cumulative rates

**Conclusion:** The complexity of cache invalidation outweighs the marginal performance benefit. If profiling reveals this as a bottleneck in the future, consider:
- Caching asset prices per time step (since market prices don't change within a step)
- Using a "dirty flag" pattern per-account to selectively invalidate

### Phase 5.1 Analysis (Event Date Indexing - DEFERRED)

After analysis, Phase 5.1 (event date indexing) was deferred:

**Current optimization already in place:**
The `advance_time()` function in simulation.rs already jumps directly to the next relevant date by:
1. Scanning all events for their trigger dates
2. Checking repeating event scheduled dates in `event_next_date`
3. Setting `next_checkpoint` to the earliest future date
4. Using a heartbeat (quarterly) to ensure progress even without events

This means `process_events_into` only runs at dates where at least one event might trigger.

**What Phase 5.1 would add:**
- Skip calling `evaluate_trigger` for events whose next trigger date > current_date
- Would require storing `next_trigger_date: Option<Date>` per event after evaluation

**Why benefit is limited:**
1. Most date-based events (Date, Age, RelativeToEvent) should trigger exactly when checked due to `advance_time` logic
2. Condition-based events (AccountBalance, NetWorth) have no predictable date and must always be checked
3. The `evaluate_trigger` function is already O(1) for simple Date events

**Conclusion:** The `advance_time` optimization already provides most of the benefit. Additional indexing would add complexity for marginal gain. Reconsider if profiling shows `evaluate_trigger` calls as a significant cost.

---

## Summary of Phase 1 Optimizations

All phases of the initial optimization plan have been addressed:

### Implemented (Phases 1-3)
- **Phase 1**: Eliminated unnecessary cloning of triggers, effects, and events
- **Phase 2**: Short-circuit evaluation for And/Or triggers
- **Phase 3**: Scratch buffer pattern for Vec reuse, pre-allocated vectors

### Deferred (Phases 4-5)
- **Phase 4.1**: Balance cache - complexity of cache invalidation outweighs benefit
- **Phase 5.1**: Event date indexing - `advance_time()` already provides similar optimization

---

# Phase 2 Performance Optimization Plan

## New Performance Profile (2026-01-21)

Fresh profiling data from `perf.data` (61K samples, ~278B cycles) reveals remaining hotspots:

| Function | % Time | Root Cause |
|----------|--------|------------|
| `evaluate_effect_into` | 15.00% | Large match statement, nested calls |
| `evaluate_trigger` | 14.14% | Date arithmetic (5.50% in saturating_add) |
| `Vec::extend_desugared` | 12.10% | Extending scratch buffers from liquidation |
| `EventEffect::clone` | 11.20% | Cloning effects in process_events loop |
| `process_events_into` | 10.28% | Main event loop overhead |
| `simulate` | 6.64% | Outer simulation loop |
| `DateArithmetic::checked_add` | 4.38% | Date calculations for triggers |
| `account_balance` | 3.41% | Balance lookups |
| `get_asset_value` | 1.71% | Market price lookups |
| `malloc` | 1.50% | Remaining allocations |

**Key Insight:** ~23% of runtime is now in effect/trigger cloning and Vec operations. Date arithmetic is a significant contributor (~10% combined).

---

## Phase 2 Optimization Plan

### P2.1: Eliminate EventEffect cloning (Target: 8-10% improvement)

**Current code (apply.rs:572-580, 656-665):**
```rust
let effect = match state.event_state.events.get(&event_id)
    .and_then(|e| e.effects.get(effect_idx))
{
    Some(effect) => effect.clone(),  // CLONE HERE
    None => break,
};
```

**Problem:** EventEffect is cloned for every effect evaluation, but the clone is only needed to break the borrow before calling `evaluate_effect_into`.

**Proposed fix:** Store effect index and defer evaluation:
```rust
// Collect (event_id, effect_idx) pairs first
let mut effects_to_apply: Vec<(EventId, usize)> = Vec::with_capacity(16);

for event_id in event_ids_to_check {
    // ... trigger evaluation ...
    if should_trigger {
        let effects_len = state.event_state.events.get(&event_id)
            .map(|e| e.effects.len()).unwrap_or(0);
        for effect_idx in 0..effects_len {
            effects_to_apply.push((event_id, effect_idx));
        }
    }
}

// Now evaluate effects - can clone just the individual effect
for (event_id, effect_idx) in effects_to_apply {
    let effect = state.event_state.events.get(&event_id)
        .and_then(|e| e.effects.get(effect_idx))
        .cloned();
    // ... evaluate ...
}
```

**Alternative:** Use Rc<EventEffect> or indices into a separate effects vec to avoid cloning entirely.

**Files to modify:**
- `crates/finplan_core/src/apply.rs`

---

### P2.2: Pre-allocate scratch buffers per-thread for Monte Carlo (Target: 8-12% improvement)

**Current code:**
- `process_events_into` (apply.rs:476) allocates `eval_scratch` every call
- `liquidate_investment` (liquidation.rs:70) allocates and returns new Vec
- `simulate()` is called many times per Monte Carlo batch

**Problem:** Scratch buffers are allocated inside functions, causing repeated allocations across Monte Carlo iterations. Each thread processes ~100 iterations but allocates fresh buffers each time.

**Proposed fix:** Create a `SimulationScratch` struct allocated once per thread:

```rust
/// Pre-allocated scratch buffers for simulation hot paths
/// Allocated once per thread and reused across Monte Carlo iterations
pub struct SimulationScratch {
    /// Scratch for triggered event IDs (process_events_into)
    pub triggered: Vec<EventId>,
    /// Scratch for evaluate_effect results
    pub eval_events: Vec<EvalEvent>,
    /// Scratch for event IDs to check
    pub event_ids_to_check: Vec<EventId>,
    /// Scratch for liquidation effects (avoid extend)
    pub liquidation_effects: Vec<EvalEvent>,
}

impl SimulationScratch {
    pub fn new() -> Self {
        Self {
            triggered: Vec::with_capacity(16),
            eval_events: Vec::with_capacity(8),
            event_ids_to_check: Vec::with_capacity(32),
            liquidation_effects: Vec::with_capacity(16),
        }
    }

    pub fn clear(&mut self) {
        self.triggered.clear();
        self.eval_events.clear();
        self.event_ids_to_check.clear();
        self.liquidation_effects.clear();
    }
}
```

**Monte Carlo integration:**
```rust
// In monte_carlo_simulate_with_config
(0..batch_size)
    .filter_map(|i| {
        // Create scratch once per thread, reuse across batch
        thread_local! {
            static SCRATCH: RefCell<SimulationScratch> = RefCell::new(SimulationScratch::new());
        }

        SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let seed = rng.next_u64();
            simulate_with_scratch(params, seed, &mut scratch).ok()
        })
    })
```

**Alternative:** Pass `&mut SimulationScratch` explicitly through the closure:
```rust
let mut scratch = SimulationScratch::new();
(0..batch_size).filter_map(|_| {
    scratch.clear();
    let seed = rng.next_u64();
    simulate_with_scratch(params, seed, &mut scratch).ok()
})
```

**Benefits:**
- Zero allocations after first iteration per thread
- Buffers grow to max needed size and stay there
- Works naturally with Rayon's work-stealing (each thread has own scratch)

**Files to modify:**
- `crates/finplan_core/src/simulation.rs` - Add SimulationScratch, thread-local usage
- `crates/finplan_core/src/apply.rs` - Accept &mut SimulationScratch
- `crates/finplan_core/src/evaluate.rs` - Accept scratch for liquidation
- `crates/finplan_core/src/liquidation.rs` - Use scratch instead of returning Vec

---

### P2.3: Cache repeating event next-trigger dates (Target: 3-5% improvement)

**Current code (evaluate.rs:244, 261):**
```rust
state.timeline.current_date.saturating_add(interval.span())
```

**Problem:** Date arithmetic is expensive (~10% combined). For repeating events, we already store `event_next_date`, but we recalculate the span every trigger.

**Proposed fix:** Pre-compute interval span once when event is created:
```rust
// In EventTrigger::Repeating
Repeating {
    interval: RepeatInterval,
    interval_span: jiff::Span,  // Pre-computed span
    start_condition: Option<Box<EventTrigger>>,
    end_condition: Option<Box<EventTrigger>>,
}
```

Or cache in SimEventState:
```rust
pub struct SimEventState {
    // ... existing fields ...
    /// Cached interval spans for repeating events
    repeating_interval_spans: FxHashMap<EventId, jiff::Span>,
}
```

**Files to modify:**
- `crates/finplan_core/src/model/events.rs`
- `crates/finplan_core/src/evaluate.rs`

---

### P2.4: Optimize lot_subtractions_to_effects (Target: 2-3% improvement)

**Current code (liquidation.rs:483-500):**
```rust
pub fn lot_subtractions_to_effects(...) -> Vec<EvalEvent> {
    result.lot_subtractions.iter()
        .map(|sub| EvalEvent::SubtractAssetLot { ... })
        .collect()
}
```

**Problem:** Allocates new Vec then extends into scratch buffer.

**Proposed fix:** Inline into the liquidation functions:
```rust
fn liquidate_taxable_into(..., out: &mut Vec<EvalEvent>) -> LiquidationResult {
    // Push SubtractAssetLot directly to out
    for sub in &lot_result.lot_subtractions {
        out.push(EvalEvent::SubtractAssetLot { ... });
    }
    // ... rest of function
}
```

**Files to modify:**
- `crates/finplan_core/src/liquidation.rs`

---

### P2.5: Reduce date arithmetic in trigger evaluation (Target: 2-3% improvement)

**Current code (evaluate.rs:129-134):**
```rust
let trigger_date = state.timeline.current_date
    .checked_add(remaining_years.years().months(remaining_months))?;
```

**Problem:** Age trigger recalculates target date every evaluation.

**Proposed fix:** Cache computed trigger dates in SimEventState for date-based triggers:
```rust
pub struct SimEventState {
    // ... existing fields ...
    /// Cached trigger dates for Age/Date triggers (cleared when date advances)
    cached_trigger_dates: FxHashMap<EventId, Date>,
}
```

Only recompute when `current_date` changes (which happens infrequently due to `advance_time`).

**Files to modify:**
- `crates/finplan_core/src/simulation_state.rs`
- `crates/finplan_core/src/evaluate.rs`

---

## Implementation Order

| Priority | Task | Est. Improvement | Complexity |
|----------|------|------------------|------------|
| 1 | P2.2 - Pre-allocate scratch per-thread | 8-12% | Medium |
| 2 | P2.4 - Inline lot_subtractions into scratch | 2-3% | Low |
| 3 | P2.1 - Avoid EventEffect cloning | 8-10% | Medium |
| 4 | P2.3 - Cache interval spans | 3-5% | Medium |
| 5 | P2.5 - Cache trigger dates | 2-3% | Medium |

**Total expected improvement: 25-35%**

**Note:** P2.2 (scratch buffers) is the foundation - implement first, then P2.4 builds on it by using the scratch for liquidation. P2.1 can be done independently.

---

## Status

- [x] P2.2 - Pre-allocate SimulationScratch per-thread
- [x] P2.4 - Inline lot_subtractions into scratch buffer
- [x] P2.1 - Eliminate EventEffect cloning
- [x] P2.3 - Cache repeating event interval spans
- [x] P2.5 - Cache trigger dates for Age/Date triggers (DEFERRED - see notes)

---

## Implementation Notes

### P2.2 Implementation (2026-01-22)

Created `SimulationScratch` struct in `apply.rs` with pre-allocated buffers:
- `triggered: Vec<EventId>` - for triggered event IDs
- `eval_events: Vec<EvalEvent>` - for evaluate_effect results
- `event_ids_to_check: Vec<EventId>` - for event IDs to process

Changes:
1. Added `SimulationScratch` struct with `new()`, `clear()`, and `Default` impl
2. Added `process_events_with_scratch()` that uses scratch buffers instead of allocating
3. Updated `process_events()` and `process_events_into()` to use the new function
4. Added `simulate_with_scratch()` that accepts pre-allocated scratch
5. Updated Monte Carlo functions to create one scratch per batch and reuse across iterations

Benefits:
- Zero allocations for scratch buffers after first iteration per thread
- Buffers grow to worst-case size and stay there
- Each Rayon thread processes its batch with a single scratch instance

### P2.4 Implementation (2026-01-22)

Inlined lot subtractions to avoid intermediate Vec allocations in liquidation functions.

Changes:
1. Added `push_lot_subtractions()` helper that pushes SubtractAssetLot events directly to output buffer
2. Added `liquidate_investment_into()` that accepts `&mut Vec<EvalEvent>` and returns just `LiquidationResult`
3. Added `liquidate_taxable_into()`, `liquidate_tax_deferred_into()`, `liquidate_tax_free_into()` helper functions
4. Updated `evaluate.rs` to use `liquidate_investment_into()` instead of `liquidate_investment()`
5. Removed the original non-`_into` helper functions (now dead code after refactor)
6. Refactored `liquidate_investment()` to delegate to `liquidate_investment_into()`

Benefits:
- Avoids intermediate Vec allocation and subsequent extend() copy for lot subtractions
- Effects are pushed directly to caller's scratch buffer
- Consistent with scratch buffer pattern established in P2.2

### P2.1 Implementation (2026-01-22)

Eliminated `EventEffect::clone()` calls in `process_events_with_scratch` by restructuring borrow scopes.

**Key insight:** `evaluate_effect_into` takes `&SimulationState` (immutable), so we can hold both `&effect` and `&state` simultaneously. The clone was only needed because the borrow scope extended past where we needed it.

**Changes to `apply.rs`:**
1. Restructured effect evaluation loops to use a two-phase approach:
   - Phase 1: Evaluate with immutable borrows (effect ref + state ref) - fills scratch buffer
   - Phase 2: Apply with mutable borrow (state mut ref) - drains scratch buffer
2. Used a block scope `{ ... }` to ensure effect borrow ends before apply phase begins
3. Applied the same pattern to both the main event loop and the pending_triggers loop

**Before:**
```rust
let effect = state.event_state.events.get(&event_id)
    .and_then(|e| e.effects.get(effect_idx))
    .map(|e| e.clone());  // Expensive clone!
evaluate_effect_into(&effect, state, &mut scratch);
```

**After:**
```rust
let eval_result = {
    let Some(effect) = state.event_state.events.get(&event_id)
        .and_then(|e| e.effects.get(effect_idx)) else { break };
    evaluate_effect_into(effect, state, &mut scratch)
}; // Borrow ends here, no clone needed
```

Benefits:
- Zero-cost effect access - no cloning of potentially large `EventEffect` variants
- `CreateAccount(Account)` with `Vec<AssetLot>` positions no longer copied per evaluation
- Maintains correct sequential semantics (each effect applied before next evaluated)

### P2.3 Implementation (2026-01-22)

Pre-computed interval spans for repeating events to avoid repeated `RepeatInterval::span()` calls during trigger evaluation.

**Changes to `simulation_state.rs`:**
1. Added `repeating_event_spans: FxHashMap<EventId, jiff::Span>` field to `SimEventState`
2. Added `EventTrigger` to imports
3. During `SimulationState::new()`, scan all events for `EventTrigger::Repeating` and cache their `interval.span()` values

**Changes to `evaluate.rs`:**
1. In `evaluate_trigger` for `EventTrigger::Repeating`, lookup cached span from `state.event_state.repeating_event_spans`
2. Falls back to computing `interval.span()` if not found (for safety with dynamically added events)
3. Use cached `interval_span` variable instead of calling `interval.span()` at lines 244 and 261

**Before:**
```rust
state.timeline.current_date.saturating_add(interval.span())
```

**After:**
```rust
let interval_span = state.event_state.repeating_event_spans
    .get(event_id)
    .copied()
    .unwrap_or_else(|| interval.span());
// ...
state.timeline.current_date.saturating_add(interval_span)
```

Benefits:
- `RepeatInterval::span()` match statement evaluated once at initialization, not per trigger evaluation
- Avoids ~3-5% overhead from date arithmetic setup (profiling showed `saturating_add` at 5.50%)
- Zero runtime cost for the cache lookup (FxHashMap is O(1))
- Graceful fallback for edge cases (dynamically created repeating events)

### P2.5 Analysis (Cache Trigger Dates - DEFERRED)

After analysis, P2.5 (caching trigger dates for Age/Date triggers) was deferred:

**Why benefit is limited:**

1. **Date triggers are already O(1):** The `EventTrigger::Date` check is a simple comparison (`current_date >= date`) - no date arithmetic involved.

2. **Age triggers compute dates only when NOT triggered:** The `checked_add()` call in Age trigger only happens to calculate `NextTriggerDate` when the trigger hasn't fired yet. Once triggered (with `once: true`), the event is skipped entirely.

3. **`current_age()` is cheap:** The age calculation uses simple integer arithmetic (year/month subtraction), not jiff date spans.

4. **`advance_time()` optimization exists:** The simulation already jumps directly to next relevant dates, so `process_events_into` only runs at dates where events might trigger. This reduces the number of trigger evaluations significantly.

5. **Most date arithmetic overhead is from Repeating events:** The profiling showed `saturating_add` at 5.50%, but P2.3 already cached interval spans for repeating events, which was the major contributor.

**Complexity concerns:**

1. Caching would require storing computed trigger dates per event
2. Cache invalidation when simulation state changes (new events added, timeline changes)
3. Age triggers depend on `birth_date` which is immutable during simulation, but adding the cache infrastructure adds complexity

**Conclusion:** The simple Date/Age trigger evaluations are already efficient. The `advance_time()` optimization means these triggers are evaluated infrequently. P2.3's caching of repeating event interval spans addressed the main date arithmetic hotspot. Adding trigger date caching would add complexity for marginal benefit (~1-2% at most).

---

## Validation Strategy

1. Run existing test suite: `cargo test -p finplan_core`
2. Benchmark with perf before/after each change
3. Compare Monte Carlo 1000-iteration times
4. Verify deterministic results with same seed

---

## Summary of Phase 2 Optimizations

All Phase 2 optimization work has been addressed:

### Implemented (P2.1-P2.4)
- **P2.1**: Eliminated EventEffect cloning by restructuring borrow scopes
- **P2.2**: Pre-allocated SimulationScratch per-thread for Monte Carlo iterations
- **P2.3**: Cached repeating event interval spans at initialization
- **P2.4**: Inlined lot_subtractions to avoid intermediate Vec allocations

### Deferred (P2.5)
- **P2.5**: Cache trigger dates for Age/Date triggers - Date/Age triggers are already efficient O(1) operations, and `advance_time()` already minimizes trigger evaluations

**Total estimated improvement from Phase 2: 20-28%** (vs original target of 25-35%)

Note: The deferred items (P2.5) had unfavorable cost-benefit ratios after detailed analysis. The implemented optimizations address the major performance hotspots identified in the Phase 2 profiling.

---

## Post-Phase 2 Profiling Results (2026-01-22)

Fresh profiling after all Phase 2 implementations (48K samples, ~218B cycles):

| Function | % Time | Notes |
|----------|--------|-------|
| `evaluate_effect_into` | 19.83% | Core effect evaluation logic |
| `evaluate_trigger` | 16.57% | Trigger condition checking |
| `process_events_with_scratch` | 13.32% | Main event loop with scratch buffers |
| `Vec::extend_desugared` | 8.94% | Remaining Vec extensions |
| `simulate_with_scratch` | 6.96% | Outer simulation loop |
| `DateArithmetic::checked_add` | 4.85% | Date calculations for triggers |
| `Vec::from_iter` | 4.53% | Collecting into new Vecs |
| `account_balance` | 3.79% | Balance lookups during evaluation |
| `hashbrown remove_entry` | 2.17% | HashMap operations |
| `evaluate_transfer_amount` | 2.11% | Transfer amount calculation |

**Confirmed improvements vs pre-Phase 2:**
- `EventEffect::clone`: 11.20% → **<1%** ✅ Eliminated
- `Vec::extend_desugared`: 12.10% → 8.94% ✅ Reduced
- `malloc`: 1.50% → **<1%** ✅ Below threshold
- **Total cycles: ~278B → ~218B (~21% improvement)**

**Remaining optimization opportunities (Phase 3 candidates):**
1. `Vec::extend_desugared` (8.94%) + `Vec::from_iter` (4.53%) = ~13.5% in Vec operations
   - Investigate where remaining extends/collects occur
   - Consider more aggressive scratch buffer usage
2. `DateArithmetic::checked_add` (4.85%) still significant
   - Age trigger date calculations
   - RelativeToEvent offset calculations
3. `account_balance` (3.79%) - balance lookups
   - Consider per-timestep caching if same account queried multiple times

---

# Monte Carlo Progress Tracking (2026-01-24)

## Feature Summary

Added real-time progress tracking for Monte Carlo simulations to support TUI progress bars.

## Implementation

### New Types

**`MonteCarloProgress`** (in `finplan_core/src/model/results.rs`):
- Thread-safe progress tracker using `Arc<AtomicUsize>` for completion count and `Arc<AtomicBool>` for cancellation
- `from_atomics()` constructor for interop with existing TUI code
- `completed()`, `is_cancelled()`, `cancel()`, `reset()` methods for external use
- `increment()` method (crate-internal) called after each iteration

### New Function

**`monte_carlo_simulate_with_progress()`** (in `finplan_core/src/simulation.rs`):
- Identical to `monte_carlo_simulate_with_config()` but accepts `&MonteCarloProgress`
- Updates progress counter after each iteration
- Checks cancellation flag periodically and returns `Err(MarketError::Cancelled)` if set
- TUI can poll `progress.completed()` for real-time updates

### Error Handling

**`MarketError::Cancelled`** (in `finplan_core/src/error.rs`):
- New variant for cancelled simulations
- TUI worker handles this gracefully by returning `Ok(None)`

### TUI Integration

**`worker.rs`**:
- Updated `run_monte_carlo_simulation()` to use `monte_carlo_simulate_with_progress()`
- Creates `MonteCarloProgress` from existing atomics via `from_atomics()`
- Progress bar now shows real iteration progress instead of jumping to 100%

## Benefits

- Accurate progress bars in TUI during Monte Carlo runs
- Responsive cancellation (checks at iteration level, not just batch level)
- Minimal overhead (atomic increments are cheap)
- Backwards compatible (original function unchanged)

## Batch Progress Display

Added to the same feature:

**`SimulationStatus::RunningBatch`** - Extended to track:
- `scenario_index` / `scenario_total` - Which scenario is being processed
- `iteration_current` / `iteration_total` - Iteration progress within current scenario
- `current_scenario_name` - Name of scenario being processed

**`SimulationRequest::Batch`** - New worker request type for batch Monte Carlo

**`SimulationResponse::BatchScenarioComplete`** - Sent after each scenario completes  
**`SimulationResponse::BatchComplete`** - Sent when all scenarios complete

**Status bar display**: Shows combined progress with scenario count, overall percentage, progress bar, and current scenario name:
```
⎯ Batch 2/5 [========       ] 42% (Scenario A) [Esc]
```

The `[R]un All` feature now runs in the background with real-time progress updates instead of blocking the UI.

---

# Returns Model Enhancement Plan

## Overview

Enhance the returns modeling in `finplan_core` to provide more realistic and sophisticated simulation of investment returns. The current implementation supports only basic distributions (None, Fixed, Normal, LogNormal). This plan adds:

1. More asset class presets with historical data
2. Fat-tailed distributions (Student's t)
3. Regime-switching models (bull/bear markets)
4. Correlated multi-asset returns
5. Historical bootstrap sampling

---

## Phase 1: Asset Class Presets and Student's t Distribution

### 1.1 Add Historical Constants for Major Asset Classes

**File:** `crates/finplan_core/src/model/market.rs`

Add preset constants to `ReturnProfile` for common asset classes:

```rust
impl ReturnProfile {
    // Existing S&P 500
    pub const SP_500_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.095668);
    pub const SP_500_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.095668,
        std_dev: 0.165234,
    };

    // US Total Bond Market (1976-2024, Barclays Aggregate proxy)
    pub const US_BOND_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.052);
    pub const US_BOND_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.052,
        std_dev: 0.065,
    };

    // International Developed Stocks (MSCI EAFE, 1970-2024)
    pub const INTL_STOCK_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.075);
    pub const INTL_STOCK_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.075,
        std_dev: 0.180,
    };

    // US Small Cap (Russell 2000 proxy, 1979-2024)
    pub const SMALL_CAP_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.105);
    pub const SMALL_CAP_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.105,
        std_dev: 0.220,
    };

    // REITs (FTSE NAREIT, 1972-2024)
    pub const REITS_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.095);
    pub const REITS_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.095,
        std_dev: 0.200,
    };

    // Treasury Bills / Money Market (1928-2024)
    pub const MONEY_MARKET_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.033);
    pub const MONEY_MARKET_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.033,
        std_dev: 0.031,
    };

    // US Treasury Long-Term Bonds (1928-2024)
    pub const TREASURY_LONG_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.052);
    pub const TREASURY_LONG_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.052,
        std_dev: 0.097,
    };

    // Corporate Bonds (1928-2024)
    pub const CORPORATE_BOND_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.058);
    pub const CORPORATE_BOND_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.058,
        std_dev: 0.072,
    };

    // 60/40 Portfolio (US Stock/Bond blend)
    pub const BALANCED_60_40_FIXED: ReturnProfile = ReturnProfile::Fixed(0.078);
    pub const BALANCED_60_40_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.078,
        std_dev: 0.110,
    };
}
```

**Rationale for values:**
- S&P 500: Existing values (1928-2024 geometric mean ~9.5%, std dev ~16.5%)
- Bonds: Aggregate bond index since 1976, lower vol before that
- International: MSCI EAFE benchmark, higher vol than US
- Small Cap: Russell 2000 since 1979, higher return/vol
- REITs: NAREIT index, equity-like returns with higher vol
- Money Market: T-bill returns, very low vol
- 60/40: Classic balanced portfolio blend

### 1.2 Add Student's t Distribution

**File:** `crates/finplan_core/src/model/market.rs`

Add new variant to `ReturnProfile` enum:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfile {
    None,
    Fixed(f64),
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
    // NEW: Student's t for fat tails
    StudentT {
        mean: f64,
        scale: f64,      // Similar to std_dev but scaled
        df: f64,         // Degrees of freedom (lower = fatter tails)
    },
}
```

**Implementation in `sample()`:**

```rust
ReturnProfile::StudentT { mean, scale, df } => {
    rand_distr::StudentT::new(*df)
        .map(|d| mean + scale * d.sample(rng))
        .map_err(|_| MarketError::InvalidDistributionParameters {
            profile_type: "StudentT return",
            mean: *mean,
            std_dev: *scale,  // Use scale in error for consistency
            reason: "degrees of freedom must be positive and finite",
        })
}
```

**Add presets:**

```rust
// Student's t with 5 df matches historical equity fat tails well
pub const SP_500_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
    mean: 0.095668,
    scale: 0.145,  // Adjusted scale for df=5
    df: 5.0,
};
```

**Add to error.rs if needed:**

Extend `MarketError::InvalidDistributionParameters` or add new variant for df validation.

### 1.3 Add Similar Constants to InflationProfile

**File:** `crates/finplan_core/src/model/market.rs`

```rust
impl InflationProfile {
    // Existing
    pub const US_HISTORICAL_FIXED: InflationProfile = InflationProfile::Fixed(0.035432);

    // Add regional variants
    pub const LOW_INFLATION_FIXED: InflationProfile = InflationProfile::Fixed(0.02);
    pub const TARGET_INFLATION_FIXED: InflationProfile = InflationProfile::Fixed(0.025);
    pub const HIGH_INFLATION_NORMAL: InflationProfile = InflationProfile::Normal {
        mean: 0.05,
        std_dev: 0.03,
    };
}
```

---

## Phase 2: Regime-Switching Model

### 2.1 Add RegimeSwitching Variant

**File:** `crates/finplan_core/src/model/market.rs`

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfile {
    // ... existing variants ...

    /// Markov regime-switching model with bull/bear states
    RegimeSwitching {
        /// Bull market parameters (higher returns, lower volatility)
        bull_mean: f64,
        bull_std_dev: f64,
        /// Bear market parameters (lower/negative returns, higher volatility)
        bear_mean: f64,
        bear_std_dev: f64,
        /// Annual probability of transitioning from bull to bear
        bull_to_bear_prob: f64,
        /// Annual probability of transitioning from bear to bull
        bear_to_bull_prob: f64,
    },
}
```

**Challenge:** Regime state must persist across years within a simulation run.

**Solution:** Track regime state in `Market` struct:

```rust
#[derive(Debug, Clone)]
pub struct Market {
    inflation_values: Vec<Rate>,
    returns: FxHashMap<ReturnProfileId, Vec<Rate>>,
    assets: FxHashMap<AssetId, (f64, ReturnProfileId)>,
    // NEW: Track current regime per profile for regime-switching
    regime_states: FxHashMap<ReturnProfileId, RegimeState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegimeState {
    Bull,
    Bear,
}
```

**Sampling implementation:**

```rust
ReturnProfile::RegimeSwitching {
    bull_mean, bull_std_dev,
    bear_mean, bear_std_dev,
    bull_to_bear_prob, bear_to_bull_prob,
} => {
    // This requires access to current regime state and RNG for transition
    // Return error indicating regime switching requires special handling
    Err(MarketError::RegimeSwitchingRequiresState)
}
```

**Alternative:** Pre-generate regime sequence in `Market::from_profiles()`:

```rust
pub fn from_profiles<R: Rng + ?Sized>(
    rng: &mut R,
    num_years: usize,
    inflation_profile: &InflationProfile,
    return_profiles: &HashMap<ReturnProfileId, ReturnProfile>,
    assets: &FxHashMap<AssetId, (f64, ReturnProfileId)>,
) -> Result<Self, MarketError> {
    let mut returns: FxHashMap<ReturnProfileId, Vec<f64>> = FxHashMap::default();

    for (rp_id, rp) in return_profiles.iter() {
        let rp_returns = match rp {
            ReturnProfile::RegimeSwitching {
                bull_mean, bull_std_dev,
                bear_mean, bear_std_dev,
                bull_to_bear_prob, bear_to_bull_prob
            } => {
                generate_regime_switching_returns(
                    rng, num_years,
                    *bull_mean, *bull_std_dev,
                    *bear_mean, *bear_std_dev,
                    *bull_to_bear_prob, *bear_to_bull_prob,
                )?
            }
            _ => {
                let mut vals = Vec::with_capacity(num_years);
                for _ in 0..num_years {
                    vals.push(rp.sample(rng)?);
                }
                vals
            }
        };
        returns.insert(*rp_id, rp_returns);
    }

    Ok(Self::new(inflation_values, returns, assets.clone()))
}

fn generate_regime_switching_returns<R: Rng + ?Sized>(
    rng: &mut R,
    num_years: usize,
    bull_mean: f64, bull_std_dev: f64,
    bear_mean: f64, bear_std_dev: f64,
    bull_to_bear_prob: f64, bear_to_bull_prob: f64,
) -> Result<Vec<f64>, MarketError> {
    let mut returns = Vec::with_capacity(num_years);
    let mut in_bull = true;  // Start in bull market

    let bull_dist = rand_distr::Normal::new(bull_mean, bull_std_dev)
        .map_err(|_| MarketError::InvalidDistributionParameters { ... })?;
    let bear_dist = rand_distr::Normal::new(bear_mean, bear_std_dev)
        .map_err(|_| MarketError::InvalidDistributionParameters { ... })?;

    for _ in 0..num_years {
        // Sample return from current regime
        let ret = if in_bull {
            bull_dist.sample(rng)
        } else {
            bear_dist.sample(rng)
        };
        returns.push(ret);

        // Transition regime for next year
        let transition_prob = if in_bull { bull_to_bear_prob } else { bear_to_bull_prob };
        if rng.gen::<f64>() < transition_prob {
            in_bull = !in_bull;
        }
    }

    Ok(returns)
}
```

### 2.2 Add Regime-Switching Presets

```rust
// Based on historical analysis of S&P 500 bull/bear cycles
pub const SP_500_REGIME_SWITCHING: ReturnProfile = ReturnProfile::RegimeSwitching {
    bull_mean: 0.15,
    bull_std_dev: 0.12,
    bear_mean: -0.08,
    bear_std_dev: 0.25,
    bull_to_bear_prob: 0.12,   // ~8 year bull cycles
    bear_to_bull_prob: 0.50,   // ~2 year bear cycles
};
```

---

## Phase 3: Correlated Returns

### 3.1 Add Correlation Matrix Support

**New file:** `crates/finplan_core/src/model/correlation.rs`

```rust
use rustc_hash::FxHashMap;
use crate::model::ReturnProfileId;

/// Correlation matrix for multi-asset returns
#[derive(Debug, Clone)]
pub struct CorrelationMatrix {
    /// Profile IDs in order (defines matrix indices)
    profiles: Vec<ReturnProfileId>,
    /// Lower triangular correlation coefficients (row-major)
    /// For n profiles: n*(n-1)/2 values
    correlations: Vec<f64>,
}

impl CorrelationMatrix {
    pub fn new(profiles: Vec<ReturnProfileId>, correlations: Vec<f64>) -> Result<Self, MarketError> {
        let n = profiles.len();
        let expected_len = n * (n - 1) / 2;
        if correlations.len() != expected_len {
            return Err(MarketError::InvalidCorrelationMatrix { ... });
        }
        // Validate correlations are in [-1, 1]
        for &c in &correlations {
            if c < -1.0 || c > 1.0 {
                return Err(MarketError::InvalidCorrelationCoefficient(c));
            }
        }
        Ok(Self { profiles, correlations })
    }

    /// Get correlation between two profiles
    pub fn get(&self, a: ReturnProfileId, b: ReturnProfileId) -> Option<f64> {
        if a == b { return Some(1.0); }
        let idx_a = self.profiles.iter().position(|&p| p == a)?;
        let idx_b = self.profiles.iter().position(|&p| p == b)?;
        let (i, j) = if idx_a < idx_b { (idx_a, idx_b) } else { (idx_b, idx_a) };
        // Lower triangular index
        let idx = j * (j - 1) / 2 + i;
        self.correlations.get(idx).copied()
    }

    /// Compute Cholesky decomposition for correlated sampling
    pub fn cholesky(&self) -> Result<CholeskyDecomp, MarketError> {
        // Build full correlation matrix
        let n = self.profiles.len();
        let mut matrix = vec![vec![0.0; n]; n];
        for i in 0..n {
            matrix[i][i] = 1.0;
            for j in 0..i {
                let idx = i * (i - 1) / 2 + j;
                matrix[i][j] = self.correlations[idx];
                matrix[j][i] = self.correlations[idx];
            }
        }

        // Cholesky decomposition (L * L^T = matrix)
        let mut l = vec![vec![0.0; n]; n];
        for i in 0..n {
            for j in 0..=i {
                let mut sum = 0.0;
                if i == j {
                    for k in 0..j {
                        sum += l[j][k] * l[j][k];
                    }
                    let val = matrix[j][j] - sum;
                    if val <= 0.0 {
                        return Err(MarketError::CorrelationMatrixNotPositiveDefinite);
                    }
                    l[j][j] = val.sqrt();
                } else {
                    for k in 0..j {
                        sum += l[i][k] * l[j][k];
                    }
                    l[i][j] = (matrix[i][j] - sum) / l[j][j];
                }
            }
        }

        Ok(CholeskyDecomp { l, profiles: self.profiles.clone() })
    }
}

#[derive(Debug, Clone)]
pub struct CholeskyDecomp {
    l: Vec<Vec<f64>>,
    profiles: Vec<ReturnProfileId>,
}

impl CholeskyDecomp {
    /// Generate correlated samples from independent standard normal samples
    pub fn correlate(&self, independent: &[f64]) -> Vec<f64> {
        let n = self.profiles.len();
        let mut correlated = vec![0.0; n];
        for i in 0..n {
            for j in 0..=i {
                correlated[i] += self.l[i][j] * independent[j];
            }
        }
        correlated
    }
}
```

### 3.2 Update Market::from_profiles for Correlated Sampling

```rust
pub fn from_profiles_correlated<R: Rng + ?Sized>(
    rng: &mut R,
    num_years: usize,
    inflation_profile: &InflationProfile,
    return_profiles: &HashMap<ReturnProfileId, ReturnProfile>,
    correlation: &CorrelationMatrix,
    assets: &FxHashMap<AssetId, (f64, ReturnProfileId)>,
) -> Result<Self, MarketError> {
    let cholesky = correlation.cholesky()?;
    let profile_ids: Vec<_> = correlation.profiles.clone();
    let n = profile_ids.len();

    // Get means and std_devs for each profile
    let params: Vec<(f64, f64)> = profile_ids.iter()
        .map(|id| {
            match return_profiles.get(id) {
                Some(ReturnProfile::Normal { mean, std_dev }) => Ok((*mean, *std_dev)),
                Some(ReturnProfile::Fixed(r)) => Ok((*r, 0.0)),
                _ => Err(MarketError::CorrelatedSamplingRequiresNormal),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut returns: FxHashMap<ReturnProfileId, Vec<f64>> = FxHashMap::default();
    for id in &profile_ids {
        returns.insert(*id, Vec::with_capacity(num_years));
    }

    let standard_normal = rand_distr::StandardNormal;

    for _ in 0..num_years {
        // Generate independent standard normal samples
        let independent: Vec<f64> = (0..n).map(|_| standard_normal.sample(rng)).collect();

        // Apply Cholesky to get correlated standard normals
        let correlated = cholesky.correlate(&independent);

        // Transform to actual returns using mean and std_dev
        for (i, id) in profile_ids.iter().enumerate() {
            let (mean, std_dev) = params[i];
            let ret = mean + std_dev * correlated[i];
            returns.get_mut(id).unwrap().push(ret);
        }
    }

    // Handle profiles not in correlation matrix (sample independently)
    for (rp_id, rp) in return_profiles.iter() {
        if !profile_ids.contains(rp_id) {
            let mut rp_returns = Vec::with_capacity(num_years);
            for _ in 0..num_years {
                rp_returns.push(rp.sample(rng)?);
            }
            returns.insert(*rp_id, rp_returns);
        }
    }

    // ... rest of function (inflation, etc.)
}
```

### 3.3 Default Correlation Presets

```rust
impl CorrelationMatrix {
    /// Standard US asset class correlations (historical averages)
    pub fn us_standard(
        us_stock: ReturnProfileId,
        intl_stock: ReturnProfileId,
        us_bond: ReturnProfileId,
        reits: ReturnProfileId,
    ) -> Self {
        // Historical correlation estimates:
        // US Stock / Intl Stock: 0.75
        // US Stock / US Bond: 0.05
        // US Stock / REITs: 0.60
        // Intl Stock / US Bond: 0.10
        // Intl Stock / REITs: 0.55
        // US Bond / REITs: 0.15
        Self {
            profiles: vec![us_stock, intl_stock, us_bond, reits],
            correlations: vec![
                0.75,              // [1,0]: US Stock / Intl Stock
                0.05, 0.10,        // [2,0], [2,1]: US Bond / ...
                0.60, 0.55, 0.15,  // [3,0], [3,1], [3,2]: REITs / ...
            ],
        }
    }
}
```

---

## Phase 4: Historical Bootstrap

### 4.1 Add Bootstrap Data Structure

**New file:** `crates/finplan_core/src/model/historical.rs`

```rust
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// Historical return series for bootstrap sampling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalReturns {
    /// Asset/index name
    pub name: String,
    /// Start year of data
    pub start_year: i16,
    /// Annual returns (index 0 = start_year)
    pub returns: Vec<f64>,
}

impl HistoricalReturns {
    /// S&P 500 annual returns 1928-2024 (97 years)
    pub fn sp500() -> Self {
        Self {
            name: "S&P 500".to_string(),
            start_year: 1928,
            returns: vec![
                0.4381, -0.0830, -0.2512, -0.4384, -0.0864, 0.5399, -0.0144, 0.4756,
                0.3392, -0.3503, 0.2994, -0.0110, -0.1078, -0.1267, 0.1917, 0.2551,
                0.1936, 0.3600, -0.0807, 0.0548, 0.0565, 0.1830, 0.3081, 0.2368,
                0.1867, -0.0099, 0.5256, 0.3262, 0.0744, -0.1046, 0.4372, 0.1206,
                0.0034, 0.2664, -0.0881, 0.2261, 0.1642, 0.1245, -0.0997, 0.2380,
                0.1081, -0.0824, 0.0400, 0.1431, 0.1898, -0.1469, -0.2647, 0.3723,
                0.2393, -0.0718, 0.0656, 0.1844, 0.3242, -0.0491, 0.2155, 0.2256,
                0.0627, 0.3173, 0.1867, 0.0525, 0.1661, 0.3169, -0.0310, 0.3047,
                0.0762, 0.1008, 0.0132, 0.3758, 0.2296, 0.3336, 0.2858, 0.2104,
                -0.0910, -0.1189, -0.2210, 0.2689, 0.1088, 0.0491, 0.1579, 0.0549,
                -0.3700, 0.2646, 0.1506, 0.0211, 0.1600, 0.3239, 0.1369, 0.0138,
                0.1196, 0.2183, -0.0438, 0.3149, 0.1840, 0.2861, -0.1830, 0.2650,
                // Add 2024 when available
            ],
        }
    }

    /// Sample a random year's return
    pub fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        let idx = rng.gen_range(0..self.returns.len());
        self.returns[idx]
    }

    /// Sample n years with replacement
    pub fn sample_years<R: Rng + ?Sized>(&self, rng: &mut R, n: usize) -> Vec<f64> {
        (0..n).map(|_| self.sample(rng)).collect()
    }

    /// Block bootstrap: sample contiguous blocks to preserve autocorrelation
    pub fn block_bootstrap<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        n: usize,
        block_size: usize,
    ) -> Vec<f64> {
        let mut result = Vec::with_capacity(n);
        while result.len() < n {
            let start = rng.gen_range(0..self.returns.len());
            for i in 0..block_size {
                if result.len() >= n { break; }
                let idx = (start + i) % self.returns.len();
                result.push(self.returns[idx]);
            }
        }
        result.truncate(n);
        result
    }
}
```

### 4.2 Add Bootstrap Variant to ReturnProfile

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfile {
    // ... existing variants ...

    /// Bootstrap from historical data
    Bootstrap {
        /// Historical returns to sample from
        history: HistoricalReturns,
        /// Optional block size for block bootstrap (1 = iid sampling)
        block_size: Option<usize>,
    },
}
```

**Note:** This requires `ReturnProfile` to derive `Clone` instead of `Copy` due to `HistoricalReturns` containing `Vec<f64>`.

### 4.3 Multi-Asset Bootstrap with Preserved Correlations

```rust
/// Bootstrap multiple assets together, preserving cross-asset correlations
pub struct MultiAssetHistory {
    /// Asset names
    pub names: Vec<String>,
    /// Start year
    pub start_year: i16,
    /// Returns matrix: returns[year][asset]
    pub returns: Vec<Vec<f64>>,
}

impl MultiAssetHistory {
    /// Sample the same year across all assets (preserves correlation)
    pub fn sample_year<R: Rng + ?Sized>(&self, rng: &mut R) -> Vec<f64> {
        let idx = rng.gen_range(0..self.returns.len());
        self.returns[idx].clone()
    }

    /// Sample n years together
    pub fn sample_years<R: Rng + ?Sized>(&self, rng: &mut R, n: usize) -> Vec<Vec<f64>> {
        (0..n).map(|_| self.sample_year(rng)).collect()
    }
}
```

---

## Phase 5: TUI Integration

### 5.1 Add New Profile Types to TUI

**File:** `crates/finplan/src/data/profiles_data.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfileData {
    None,
    Fixed { rate: f64 },
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
    // NEW
    StudentT { mean: f64, scale: f64, df: f64 },
    RegimeSwitching {
        bull_mean: f64,
        bull_std_dev: f64,
        bear_mean: f64,
        bear_std_dev: f64,
        bull_to_bear_prob: f64,
        bear_to_bull_prob: f64,
    },
    // Bootstrap requires more complex handling (file reference?)
}
```

### 5.2 Update Profile Editor

**File:** `crates/finplan/src/actions/profile.rs`

Add form fields for new profile types:
- StudentT: mean, scale, degrees of freedom
- RegimeSwitching: bull params, bear params, transition probabilities

### 5.3 Add Preset Profile Selection

Add a "Load Preset" option in profile editor that offers:
- S&P 500 (Normal)
- S&P 500 (Student's t)
- S&P 500 (Regime Switching)
- US Total Bond
- International Stocks
- Small Cap
- REITs
- Money Market
- 60/40 Balanced

---

## Implementation Order

| Priority | Task | Complexity | Value |
|----------|------|------------|-------|
| 1 | 1.1 - Asset class presets | Low | High |
| 2 | 1.2 - Student's t distribution | Low | High |
| 3 | 1.3 - Inflation presets | Low | Medium |
| 4 | 5.1-5.3 - TUI integration (basic) | Medium | High |
| 5 | 2.1-2.2 - Regime switching | Medium | High |
| 6 | 3.1-3.3 - Correlated returns | High | Medium |
| 7 | 4.1-4.3 - Historical bootstrap | High | Medium |

---

## Status

- [x] Phase 1.1 - Asset class preset constants (11 asset classes with Fixed/Normal/LogNormal)
- [x] Phase 1.2 - Student's t distribution (variant added, presets for high-volatility assets)
- [x] Phase 1.3 - Inflation preset constants (US_HISTORICAL_FIXED, US_HISTORICAL_NORMAL, US_HISTORICAL_LOG_NORMAL)
- [x] Phase 2.1 - Regime switching variant (Box<ReturnProfile> for bull/bear, sample_sequence for stateful)
- [x] Phase 2.2 - Regime switching presets (sp500_regime_switching_normal, sp500_regime_switching_student_t)
- [ ] Phase 3.1 - Correlation matrix structure
- [ ] Phase 3.2 - Correlated sampling in Market
- [ ] Phase 3.3 - Default correlation presets
- [ ] Phase 4.1 - Historical returns data structure
- [ ] Phase 4.2 - Bootstrap variant
- [ ] Phase 4.3 - Multi-asset bootstrap
- [ ] Phase 5.1 - TUI profile data types
- [ ] Phase 5.2 - TUI profile editor updates
- [ ] Phase 5.3 - TUI preset selection

---

## Phase 1 Implementation Notes (2026-01-26)

### Phase 1.1 - Asset Class Presets
Added comprehensive return profile constants sourced from:
- Robert Shiller, Yale University (S&P 500 since 1871)
- Kenneth French Data Library, Dartmouth (Fama-French factors since 1926)
- Yahoo Finance (ETF data for recent history)

Asset classes covered:
- S&P 500 (97 years)
- US Small Cap (98 years)
- US T-Bills (92 years)
- US Long-Term Bonds (97 years)
- International Developed (34 years)
- Emerging Markets (33 years)
- REITs (22 years)
- Gold (26 years)
- US Aggregate Bonds (23 years)
- US Corporate Bonds (24 years)
- TIPS (23 years)

Also added `historical_returns` module with annual return arrays for future bootstrap sampling.

### Phase 1.2 - Student's t Distribution
Added `ReturnProfile::StudentT { mean, scale, df }` variant for fat-tailed returns.

**Key implementation details:**
- `mean`: Location parameter (expected return)
- `scale`: Scale parameter, computed as `std_dev * sqrt((df-2)/df)` to match target std_dev
- `df`: Degrees of freedom (lower = fatter tails, typically 4-6 for equities)

**Presets added:**
- SP_500_HISTORICAL_STUDENT_T (df=5)
- US_SMALL_CAP_HISTORICAL_STUDENT_T (df=5)
- EMERGING_MARKETS_HISTORICAL_STUDENT_T (df=5)

The Python data script (`scripts/fetch_historical_returns.py`) was updated to automatically
generate StudentT constants for high-volatility assets (std_dev > 5%).

### Phase 1.3 - Inflation Presets
Inflation constants were already present:
- US_HISTORICAL_FIXED (geometric mean: 3.43%)
- US_HISTORICAL_NORMAL (mean: 3.47%, std_dev: 2.79%)
- US_HISTORICAL_LOG_NORMAL

### Phase 2.1-2.2 - Regime Switching (2026-01-26)

Added `ReturnProfile::RegimeSwitching` variant for Markov regime-switching models.

**Design decisions:**
- Used `Box<ReturnProfile>` for bull/bear states instead of just mean/std_dev
- This allows any distribution type (Normal, StudentT, etc.) for each regime
- More flexible and composable, follows same pattern as nested EventTriggers
- Removed `Copy` derive from `ReturnProfile` (required for `Box`)

**Key implementation:**
```rust
RegimeSwitching {
    bull: Box<ReturnProfile>,      // Return profile during bull markets
    bear: Box<ReturnProfile>,      // Return profile during bear markets
    bull_to_bear_prob: f64,        // Annual transition probability
    bear_to_bull_prob: f64,        // Annual transition probability
}
```

**Two sampling modes:**
1. `sample()` - Stateless sampling using steady-state regime probabilities
   - P(bull) = bear_to_bull / (bull_to_bear + bear_to_bull)
   - Useful for one-off sampling
2. `sample_sequence()` - Stateful sampling that maintains regime across years
   - Starts in bull market, transitions based on probabilities
   - Used by `Market::from_profiles()` for proper regime clustering

**Presets added (as functions, not const):**
- `sp500_regime_switching_normal()` - Bull: 15%/12% | Bear: -8%/25%
- `sp500_regime_switching_student_t()` - Same with df=5 fat tails
- `regime_switching()` - Custom constructor helper

**Test coverage:**
- Stateless sampling produces mix of bull/bear returns
- Sequence sampling shows regime persistence (fewer sign-change "runs")
- Works with StudentT sub-profiles
- Custom profile construction
- Integration with Market::from_profiles

---

## Testing Strategy

1. **Unit tests for new distributions:**
   - Verify Student's t sampling produces expected moments
   - Verify regime switching transition probabilities
   - Verify Cholesky decomposition correctness
   - Verify correlated samples have expected correlation

2. **Integration tests:**
   - End-to-end simulation with each new profile type
   - Monte Carlo convergence tests

3. **Validation:**
   - Compare simulated distributions to historical data
   - Verify correlation preservation in multi-asset scenarios

---

## Notes

- Student's t with df=5 approximates equity return distributions well (kurtosis ~6 vs ~3 for normal)
- Regime switching captures the clustering of good/bad years
- Correlation becomes more important with diversified portfolios
- Historical bootstrap is non-parametric "gold standard" but requires good data
- Consider lazy-loading historical data to avoid bloating binary size
