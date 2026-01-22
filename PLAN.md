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

## Summary

All phases of the performance optimization plan have been addressed:

### Implemented (Phases 1-3)
- **Phase 1**: Eliminated unnecessary cloning of triggers, effects, and events
- **Phase 2**: Short-circuit evaluation for And/Or triggers
- **Phase 3**: Scratch buffer pattern for Vec reuse, pre-allocated vectors

### Deferred (Phases 4-5)
- **Phase 4.1**: Balance cache - complexity of cache invalidation outweighs benefit
- **Phase 5.1**: Event date indexing - `advance_time()` already provides similar optimization

The implemented optimizations target the original profiling hotspots:
- ~35% of runtime from cloning/allocation → addressed by Phases 1 and 3
- Short-circuit And/Or evaluation → addressed by Phase 2

Further optimization should be guided by new profiling data to identify remaining bottlenecks.
