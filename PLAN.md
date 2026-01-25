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
