# Compiled Event Schedule Optimization Plan

## Problem Statement

Profiling data (`perf.data`) shows that `evaluate_trigger` consumes **22.75% of total CPU time** during Monte Carlo simulations. For typical retirement scenarios with 10-12 events, most triggers are pure time-based and could be pre-computed rather than evaluated on every time step.

## Profiling Summary

| Function | % CPU | Optimization Potential |
|----------|-------|------------------------|
| evaluate_trigger | 22.75% | High - pre-compile time-based triggers |
| evaluate_effect_into | 17.49% | Low - must run at trigger time |
| process_events_with_scratch | 16.80% | Medium - reduce iteration overhead |
| Vec::from_iter | 7.66% | Medium - reduce allocations |
| TriggerOffset::add_to_date | 4.08% | High - pre-compute dates |
| deallocate | 3.74% | Medium - reduce allocations |
| jiff::Span::years | 1.10% | High - cache spans |

## Event Classification

Triggers fall into two categories:

### Pre-computable (Pure Time-Based)
- `Date(d)` - single known date
- `Age { years, months }` - computable from birth_date
- `RelativeToEvent` where referenced event is also time-based
- `Repeating` with only time-based start/end conditions

### Runtime-Dependent (State-Based)
- `AccountBalance` - depends on simulation state
- `AssetBalance` - depends on simulation state
- `NetWorth` - depends on simulation state
- `Repeating` with balance-based start/end conditions

## Example Scenario Analysis

For a typical retirement simulation with 12 events:

| Event | Trigger Type | Pre-computable? |
|-------|-------------|-----------------|
| Bi-Weekly Salary | Repeating, end=RelativeToEvent(Age) | Yes |
| Living Expenses | Pure Repeating | Yes |
| 401k Contribution | Repeating, end=RelativeToEvent(Age) | Yes |
| Roth Backdoor | Repeating, end=RelativeToEvent(Age) | Yes |
| Stock Vest | Repeating, end=Date | Yes |
| Buy House | Date | Yes |
| Mortgage Payment | Repeating, end=AccountBalance | **No** |
| Retirement | Age | Yes |
| Keep Checking | AccountBalance | **No** |
| Medicare Part B | Repeating, start=Age | Yes |
| Social Security | Repeating, start=Age | Yes |
| RMD | Repeating, start=Age | Yes |

**Result: 10 of 12 events (83%) are fully pre-computable.**

## Estimated Performance Impact

- Pre-computable events eliminate ~80% of `evaluate_trigger` calls
- 22.75% × 0.80 = **~18% CPU reduction** from trigger pre-computation
- Additional ~5% from pre-computed date arithmetic
- **Total: ~20-25% faster simulations**

## Proposed Design

### New Data Structures

```rust
/// Pre-computed event schedule for pure time-based events
pub struct CompiledEventSchedule {
    /// Date -> EventIds that trigger on that date
    /// BTreeMap for efficient range queries during time advancement
    scheduled_triggers: BTreeMap<Date, SmallVec<[EventId; 4]>>,

    /// Pre-computed repeating schedules: EventId -> queue of trigger dates
    /// Only for repeating events with no balance-based conditions
    repeating_schedules: Vec<Option<VecDeque<Date>>>,

    /// Events that require runtime evaluation (balance-dependent)
    runtime_events: Vec<EventId>,

    /// Quick classification per event (indexed by EventId)
    trigger_classification: Vec<TriggerClassification>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum TriggerClassification {
    /// Pure date trigger - fires on specific date, no evaluation needed
    Scheduled,
    /// Pre-computed repeating schedule - just pop next date
    PrecomputedRepeating,
    /// Depends on runtime state - must call evaluate_trigger
    RuntimeDependent,
    /// Waiting for another event (RelativeToEvent where ref hasn't fired)
    Pending { waiting_for: EventId },
}
```

### Compilation Phase

Add to `SimulationState::from_parameters()`:

```rust
fn compile_event_schedule(
    events: &[Event],
    birth_date: Date,
    start_date: Date,
    end_date: Date,
) -> CompiledEventSchedule {
    // 1. Classify each trigger recursively
    // 2. Resolve dependency chains (RelativeToEvent -> Age -> Date)
    // 3. Generate full schedules for pre-computable repeating events
    // 4. Build the scheduled_triggers map
}

fn classify_trigger(trigger: &EventTrigger) -> TriggerClassification {
    match trigger {
        EventTrigger::Date(_) => TriggerClassification::Scheduled,
        EventTrigger::Age { .. } => TriggerClassification::Scheduled,
        EventTrigger::AccountBalance { .. } => TriggerClassification::RuntimeDependent,
        EventTrigger::AssetBalance { .. } => TriggerClassification::RuntimeDependent,
        EventTrigger::NetWorth { .. } => TriggerClassification::RuntimeDependent,
        EventTrigger::Repeating { start_condition, end_condition, .. } => {
            // Check if start/end conditions are time-based
            let start_ok = start_condition.as_ref()
                .map(|c| is_time_based(c))
                .unwrap_or(true);
            let end_ok = end_condition.as_ref()
                .map(|c| is_time_based(c))
                .unwrap_or(true);
            if start_ok && end_ok {
                TriggerClassification::PrecomputedRepeating
            } else {
                TriggerClassification::RuntimeDependent
            }
        }
        EventTrigger::RelativeToEvent { event_id, .. } => {
            // Resolve transitively - if referenced event is time-based, so is this
            TriggerClassification::Pending { waiting_for: *event_id }
        }
        EventTrigger::And(triggers) | EventTrigger::Or(triggers) => {
            // All must be time-based for And, any for Or (conservative: require all)
            if triggers.iter().all(|t| is_time_based(t)) {
                TriggerClassification::Scheduled
            } else {
                TriggerClassification::RuntimeDependent
            }
        }
        EventTrigger::Manual => TriggerClassification::RuntimeDependent,
    }
}
```

### Execution Phase

Modify `process_events_with_scratch()`:

```rust
pub fn process_events_with_scratch(state: &mut SimulationState, scratch: &mut SimulationScratch) {
    let current_date = state.timeline.current_date;

    // Step 1: Fire pre-scheduled events for today (O(log N) lookup)
    if let Some(event_ids) = state.compiled_schedule.scheduled_triggers.get(&current_date) {
        for &event_id in event_ids {
            // No evaluate_trigger needed! Just fire directly.
            fire_event(state, event_id, scratch);
        }
    }

    // Step 2: Check pre-computed repeating events (O(1) per event)
    for event_id in &state.compiled_schedule.precomputed_repeating_ids {
        if let Some(schedule) = &mut state.repeating_schedules[event_id.0 as usize] {
            while schedule.front().map(|d| *d <= current_date).unwrap_or(false) {
                schedule.pop_front();
                fire_event(state, *event_id, scratch);
            }
        }
    }

    // Step 3: Only evaluate_trigger for runtime-dependent events
    for &event_id in &state.compiled_schedule.runtime_events {
        // Existing early-skip optimization still applies here
        if let Some(next_trigger) = state.event_state.next_possible_trigger(event_id) {
            if current_date < next_trigger {
                continue;
            }
        }

        // This is now the ONLY place we call evaluate_trigger
        let trigger = &state.event_state.get_event(event_id).unwrap().trigger;
        match evaluate_trigger(&event_id, trigger, state) {
            // ... existing logic
        }
    }

    // Step 4: Process chained triggers (existing logic)
}
```

## Implementation Steps

### Phase 1: Classification Infrastructure
1. Add `TriggerClassification` enum to `model/events.rs`
2. Add `classify_trigger()` function with recursive analysis
3. Add `is_time_based()` helper for trigger analysis
4. Unit tests for classification logic

### Phase 2: Schedule Compilation
1. Add `CompiledEventSchedule` struct to `simulation_state.rs`
2. Implement `compile_event_schedule()` function
3. Handle `RelativeToEvent` dependency resolution
4. Generate repeating event schedules up to `end_date`
5. Integration into `SimulationState::from_parameters()`

### Phase 3: Execution Integration
1. Modify `process_events_with_scratch()` to use compiled schedule
2. Add fast path for scheduled triggers
3. Add fast path for pre-computed repeating events
4. Preserve existing logic for runtime-dependent events
5. Benchmarks comparing before/after

### Phase 4: Monte Carlo Optimization (Optional)
1. Share compiled schedule across parallel iterations via `Arc<CompiledEventSchedule>`
2. Only runtime-dependent state needs per-iteration storage
3. Reduces memory pressure and compilation overhead

## Testing Strategy

1. **Unit tests**: Trigger classification for all trigger types
2. **Property tests**: Pre-computed schedules match runtime evaluation
3. **Integration tests**: Full simulation results unchanged
4. **Benchmarks**: Measure actual speedup on representative scenarios

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Classification bugs causing missed triggers | Comprehensive tests comparing pre-computed vs runtime |
| Memory overhead for large schedules | Lazy generation, limit schedule horizon |
| Complexity increase | Clear separation between compilation and execution |
| Edge cases in date arithmetic | Reuse existing `TriggerOffset::add_to_date` |

## Success Criteria

- [ ] All existing tests pass
- [ ] Benchmark shows 15-25% speedup on typical scenarios
- [ ] No increase in memory usage > 10%
- [ ] Code remains maintainable and well-documented
