# FinPlan TUI Architecture Improvement Plan

Use this file as a store of current and future plans for the repo.
Edit the file as needed to track the implementation state, assumptions and reasoning about feature implementation.
When work is complete make sure to update the state of PLAN.md.

---

## Executive Summary

The TUI codebase (~11,000 LOC) is solid for an MVP but showing scaling strain in three areas:
1. **Mega-components** - PortfolioProfilesScreen is 3,519 lines
2. **Code duplication** - 500+ LOC of repeated patterns (list nav, panel focus, form parsing)
3. **Lack of abstraction** - No typed forms, no event-based state management, limited Component usage

This plan outlines a phased approach to improve maintainability, testability, and extensibility.

---

## Phase 1: Component Trait Expansion

**Goal:** Break monolithic screens into composable, testable sub-components.

**Status:** In Progress

### Current State
- 7 Component implementations (5 screens + TabBar + StatusBar)
- 29+ `render_*` methods that should be Components
- Screens mix rendering, input handling, and state management

### Completed Work
- Created `components/lists/` module with `SelectableList`, `calculate_centered_scroll`, input helpers
- Created `PanelNavigable` trait implemented by all panel enums
- Created `components/panels/` module structure
- Extracted `AccountsPanel` (~500 LOC) and integrated with PortfolioProfilesScreen
- Extracted `ProfilesPanel` (~814 LOC) with list, details, and distribution chart rendering
- Extracted `DistributionChart` components to `components/charts/distribution.rs` (~561 LOC)
- Extracted `EventListPanel` (~393 LOC) with event list rendering and key handling
- Extracted `LedgerPanel` (~443 LOC) with ledger rendering and formatting helpers
- **PortfolioProfilesScreen: 3,519 → 1,315 lines (63% reduction)**
- **EventsScreen: 1,272 → 908 lines (29% reduction)**
- **ResultsScreen: 1,240 → 846 lines (32% reduction)**

### Target Architecture

```
App
├── TabBar (exists)
├── StatusBar (exists)
├── PortfolioProfilesScreen
│   ├── AccountsPanel (NEW)
│   │   ├── AccountList (NEW)
│   │   └── AccountDetails (NEW)
│   ├── ProfilesPanel (NEW)
│   │   └── DistributionChart (NEW)
│   ├── AssetMappingsPanel (NEW)
│   └── TaxConfigPanel (NEW)
├── EventsScreen
│   ├── EventListPanel (NEW)
│   ├── EventDetailsPanel (NEW)
│   └── TimelinePanel (NEW)
├── ResultsScreen
│   ├── NetWorthChartPanel (NEW)
│   ├── YearlyBreakdownPanel (NEW)
│   ├── AccountChartPanel (NEW)
│   └── LedgerPanel (NEW)
├── ScenarioScreen
│   ├── ScenarioListPanel (NEW)
│   ├── ScenarioDetailsPanel (NEW)
│   └── ComparisonChartPanel (NEW)
├── OptimizeScreen
│   └── (smaller, lower priority)
└── Modal Layer
    ├── FormModal (NEW - implement Component)
    ├── PickerModal (NEW - implement Component)
    └── ConfirmModal (NEW - implement Component)
```

### Tasks

- [x] **1.1** Create `components/panels/` module structure
- [x] **1.2** Extract `FocusedBlock` helper (already in `util/styles.rs` - `focused_block_with_help()`)
- [x] **1.3** Extract `SelectableList` component with input helpers (`components/lists/selectable_list.rs`)
- [x] **1.4** Extract `AccountsPanel` from portfolio_profiles.rs - integrated with screen
- [x] **1.5** Extract `ProfilesPanel` from portfolio_profiles.rs - integrated with screen
- [x] **1.6** Extract `DistributionChart` components to `components/charts/distribution.rs`
- [x] **1.7** Extract `EventListPanel` from events.rs - integrated with screen
- [x] **1.8** Extract `LedgerPanel` from results.rs - integrated with screen
- [ ] **1.9** Make modals implement Component trait for consistency

### Actual Results
- PortfolioProfilesScreen: **3,519 → 1,315 lines** (63% reduction)
- EventsScreen: **1,272 → 908 lines** (29% reduction)
- ResultsScreen: **1,240 → 846 lines** (32% reduction)
- **Total screen reduction: 6,031 → 3,069 lines (49% reduction)**
- New reusable components: ~3,063 lines (panels, charts, lists)
- **Massive testability and maintainability improvement**

---

## Phase 2: Label-Based Form Access

**Goal:** Replace fragile magic-index form parsing with label-based field access.

**Status:** Complete

### Problem

Forms used hardcoded indices that break silently if field order changes:

```rust
// FRAGILE - magic indices
let name = parts.get(0)?;
let amount = parts.get(3)?.parse::<f64>()?;
```

### Solution

Added label-based access methods to `FormModal` in `state/modal.rs`:

```rust
// Self-documenting, resistant to field reordering
let name = form.str("Name")?;
let amount = form.currency_or("Amount", 0.0);
let rate = form.percentage_or("Rate", 0.05);
let enabled = form.bool_or("Enabled", true);
let age = form.int_or::<u8>("Age", 65);
```

### Implementation

Added to `FormModal`:
- `str(label)` / `str_non_empty(label)` / `optional_str(label)` - String access
- `currency(label)` / `currency_or(label, default)` - Parse currency (handles $, commas)
- `percentage(label)` / `percentage_or(label, default)` - Parse percentage to decimal
- `bool(label)` / `bool_or(label, default)` - Parse Yes/No, Y/N, true/false
- `int(label)` / `int_or(label, default)` - Parse integers

### Benefits
- ~50 lines added vs ~2,700 lines for typed form structs
- No new modules or types to maintain
- Labels in form creation and extraction are the same strings
- IDE can find all usages of a label
- Runtime string matching is negligible (~4-6 fields per form)

### Migration Path
Action handlers can migrate incrementally from index-based to label-based access.
Old index-based methods (`get_str(index)`, etc.) remain for backwards compatibility.

---

## Phase 3: Event-Based State Management

**Goal:** Introduce event-driven architecture for better traceability, undo/redo, and testing.

**Status:** Not Started

### Current Problem

Actions directly mutate `&mut AppState`:
- Hard to track what changed
- No undo/redo capability
- Scattered mutation points
- Difficult to log/replay for debugging

### Target Architecture

```rust
// events/mod.rs - Domain events
pub enum AppEvent {
    // Account domain
    AccountCreated { account: AccountData },
    AccountUpdated { id: AccountId, changes: AccountChanges },
    AccountDeleted { id: AccountId },
    HoldingAdded { account_id: AccountId, holding: HoldingData },

    // Profile domain
    ProfileCreated { profile: ProfileData },
    ProfileUpdated { id: ProfileId, changes: ProfileChanges },
    ProfileDeleted { id: ProfileId },

    // Event domain
    EventCreated { event: EventData },
    EventUpdated { id: EventId, changes: EventChanges },
    EventDeleted { id: EventId },
    EffectAdded { event_id: EventId, effect: EffectData },
    EffectRemoved { event_id: EventId, effect_index: usize },

    // Scenario domain
    ScenarioCreated { name: String, data: SimulationData },
    ScenarioSwitched { name: String },
    ScenarioDeleted { name: String },

    // Simulation domain
    SimulationRequested { request: SimulationRequest },
    SimulationCompleted { result: SimulationResult },
    MonteCarloCompleted { result: MonteCarloResult },

    // UI domain
    TabChanged { tab: TabId },
    PanelFocused { panel: PanelId },
    ModalOpened { modal: ModalState },
    ModalClosed,
    ErrorOccurred { message: String },
}

// reducer.rs - Central state transitions
pub fn reduce(state: &mut AppState, event: &AppEvent) {
    match event {
        AppEvent::AccountCreated { account } => {
            state.data_mut().portfolios.accounts.push(account.clone());
            state.mark_modified();
        }
        AppEvent::AccountDeleted { id } => {
            state.data_mut().portfolios.accounts.retain(|a| a.id != *id);
            state.mark_modified();
        }
        AppEvent::ScenarioSwitched { name } => {
            state.switch_scenario(name);
        }
        // ... all state transitions centralized
    }
}

// Inverse events for undo
pub fn inverse(event: &AppEvent, state: &AppState) -> Option<AppEvent> {
    match event {
        AppEvent::AccountCreated { account } => {
            Some(AppEvent::AccountDeleted { id: account.id.clone() })
        }
        AppEvent::AccountDeleted { id } => {
            let account = state.find_account(id)?;
            Some(AppEvent::AccountCreated { account: account.clone() })
        }
        // ... inverse for undoable events
    }
}
```

### Hybrid Migration Strategy

Keep `ActionResult` but add `Event` variant for gradual migration:

```rust
pub enum ActionResult {
    Done(Option<ModalState>),
    Modified(Option<ModalState>),  // Legacy - direct mutation
    Event(AppEvent),                // NEW - dispatch through reducer
    Events(Vec<AppEvent>),          // NEW - multiple events
    Error(String),
}

// App handles both during migration
fn apply_action_result(&mut self, result: ActionResult) {
    match result {
        ActionResult::Modified(modal) => {
            // Legacy path
            self.state.mark_modified();
            if let Some(m) = modal { self.state.modal = m; }
        }
        ActionResult::Event(event) => {
            // New path
            self.dispatch(event);
        }
        ActionResult::Events(events) => {
            for event in events {
                self.dispatch(event);
            }
        }
        // ...
    }
}
```

### Tasks

- [ ] **3.1** Define `AppEvent` enum with all domain events
- [ ] **3.2** Create `reduce()` function for state transitions
- [ ] **3.3** Add `ActionResult::Event` variant
- [ ] **3.4** Add event dispatch to `App::apply_action_result()`
- [ ] **3.5** Add event logging/tracing
- [ ] **3.6** Migrate account actions to emit events
- [ ] **3.7** Migrate profile actions to emit events
- [ ] **3.8** Migrate event actions to emit events
- [ ] **3.9** Migrate effect actions to emit events
- [ ] **3.10** Migrate scenario actions to emit events
- [ ] **3.11** Implement `inverse()` for undoable events
- [ ] **3.12** Add event history for undo/redo
- [ ] **3.13** Implement undo (Ctrl+Z) command
- [ ] **3.14** Implement redo (Ctrl+Shift+Z) command

### Estimated Impact
- Centralized state transitions (easier debugging)
- Full undo/redo capability
- Event replay for testing
- Audit trail for all changes

---

## Phase 4: Input System Improvements

**Goal:** Reduce duplication in input handling, formalize input modes, enable key rebinding.

**Status:** Not Started

### Current Problems

1. **List navigation duplicated 4+ times** (j/k, Up/Down handling)
2. **Panel cycling duplicated 5 times** (Tab/Shift+Tab)
3. **Input modes scattered** (browse, edit, modal)
4. **No keybinding registry** (hardcoded throughout)

### Target Architecture

```rust
// input/handlers.rs - Reusable input handlers
pub fn handle_list_navigation(
    key: &AppKeyEvent,
    selected: &mut usize,
    total: usize,
) -> Option<EventResult> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down if total > 0 => {
            *selected = (*selected + 1) % total;
            Some(EventResult::Handled)
        }
        KeyCode::Char('k') | KeyCode::Up if total > 0 => {
            *selected = selected.checked_sub(1).unwrap_or(total - 1);
            Some(EventResult::Handled)
        }
        _ => None,
    }
}

pub fn handle_panel_navigation<P: PanelNavigable>(
    key: &AppKeyEvent,
    focused: &mut P,
) -> Option<EventResult> {
    match key {
        _ if key.is_back_tab() => {
            *focused = focused.prev();
            Some(EventResult::Handled)
        }
        AppKeyEvent { code: KeyCode::Tab, .. } if key.no_modifiers() => {
            *focused = focused.next();
            Some(EventResult::Handled)
        }
        _ => None,
    }
}

// input/modes.rs - Formalized input modes
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Browse { panel: PanelId },
    Edit { field: EditFieldType },
    Modal { modal_type: ModalType },
    HoldingsEdit { account: usize, state: HoldingEditState },
}

// input/keybindings.rs - Keybinding registry
pub struct KeyBinding {
    pub key: KeyPattern,
    pub modifiers: Modifiers,
    pub context: InputContext,
    pub action: &'static str,
    pub description: &'static str,
}

pub static KEYBINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: KeyPattern::Char('j'),
        modifiers: Modifiers::NONE,
        context: InputContext::List,
        action: "move_down",
        description: "Move down",
    },
    // ... all keybindings
];

// Generate help text from registry
pub fn generate_help_text(context: InputContext) -> Vec<(&'static str, &'static str)> {
    KEYBINDINGS
        .iter()
        .filter(|kb| kb.context == context)
        .map(|kb| (kb.key.display(), kb.description))
        .collect()
}
```

### Tasks

- [ ] **4.1** Create `input/handlers.rs` with `handle_list_navigation()`
- [ ] **4.2** Create `handle_panel_navigation()` generic handler
- [ ] **4.3** Create `handle_reorder()` for Shift+J/K patterns
- [ ] **4.4** Define `InputMode` enum
- [ ] **4.5** Create keybinding registry
- [ ] **4.6** Migrate list navigation in all screens
- [ ] **4.7** Migrate panel navigation in all screens
- [ ] **4.8** Generate help text from registry
- [ ] **4.9** Add help overlay (?) showing all keybindings

### Estimated Impact
- Eliminate ~150 lines of duplicate input handling
- Consistent keybindings across screens
- Auto-generated help text
- Foundation for key rebinding

---

## Phase 5: Data Layer Improvements

**Goal:** Improve data integrity, add schema versioning, enable safer persistence.

**Status:** Not Started

### Current Problems

1. **Non-atomic writes** - crash during save = data corruption
2. **No schema versioning** - format changes break old files
3. **Duplicate Monte Carlo logic** - `run_monte_carlo()` and `run_monte_carlo_for_scenario()` 90% identical
4. **Magic number duplication** - `0.001` percentile tolerance appears 13+ times

### Target Architecture

```rust
// data/atomic.rs - Atomic file operations
pub fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let temp = path.with_extension("yaml.tmp");
    fs::write(&temp, content)?;
    fs::rename(&temp, path)?;  // Atomic on POSIX
    Ok(())
}

// data/schema.rs - Schema versioning
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
pub struct VersionedData {
    pub schema_version: u32,
    #[serde(flatten)]
    pub data: SimulationData,
}

pub fn migrate(data: &mut VersionedData) {
    while data.schema_version < CURRENT_SCHEMA_VERSION {
        match data.schema_version {
            1 => migrate_v1_to_v2(&mut data.data),
            _ => break,
        }
        data.schema_version += 1;
    }
}

// simulation/percentiles.rs - Extracted helpers
pub const PERCENTILE_TOLERANCE: f64 = 0.001;

pub fn find_percentile_value(
    stats: &PercentileStats,
    target: f64,
) -> Option<f64> {
    stats.percentile_values
        .iter()
        .find(|(p, _)| (*p - target).abs() < PERCENTILE_TOLERANCE)
        .map(|(_, v)| *v)
}

pub struct PercentileSet {
    pub p5: f64,
    pub p50: f64,
    pub p95: f64,
    pub mean: f64,
}

pub fn extract_percentiles(stats: &PercentileStats) -> Option<PercentileSet> {
    Some(PercentileSet {
        p5: find_percentile_value(stats, 0.05)?,
        p50: find_percentile_value(stats, 0.50)?,
        p95: find_percentile_value(stats, 0.95)?,
        mean: stats.mean,
    })
}
```

### Tasks

- [ ] **5.1** Create `atomic_write()` function
- [ ] **5.2** Migrate all file writes to use `atomic_write()`
- [ ] **5.3** Add `schema_version` to saved YAML
- [ ] **5.4** Create migration framework
- [ ] **5.5** Extract `PERCENTILE_TOLERANCE` constant
- [ ] **5.6** Create `find_percentile_value()` helper
- [ ] **5.7** Create `extract_percentiles()` helper
- [ ] **5.8** Consolidate `run_monte_carlo()` and `run_monte_carlo_for_scenario()`
- [ ] **5.9** Add `Storage` trait for dependency injection
- [ ] **5.10** Create `MemoryStorage` for testing

### Estimated Impact
- Eliminate data corruption risk
- Safe schema evolution
- Remove 13 duplications of percentile logic
- Testable storage layer

---

## Phase 6: Testing Infrastructure

**Goal:** Enable comprehensive testing of TUI components.

**Status:** Not Started

### Current State
- 31 unit tests covering pure logic (cache, context, modals, wizard)
- 0 tests for screens (7,892 LOC untested)
- No integration tests
- No snapshot tests

### Target Architecture

```rust
// test_utils/mod.rs
pub fn create_test_state() -> AppState {
    AppState::new_with_defaults()
}

pub fn create_test_state_with_scenario(name: &str) -> AppState {
    let mut state = create_test_state();
    state.new_scenario(name);
    state
}

// Integration tests
#[cfg(test)]
mod integration_tests {
    #[test]
    fn test_account_workflow() {
        let mut state = create_test_state();

        // Create account via event
        let event = AppEvent::AccountCreated {
            account: AccountData::checking("Test")
        };
        reduce(&mut state, &event);

        assert_eq!(state.data().portfolios.accounts.len(), 1);
        assert!(state.is_dirty("Default"));
    }

    #[test]
    fn test_undo_redo() {
        let mut app = TestApp::new();

        app.dispatch(AppEvent::AccountCreated { ... });
        assert_eq!(app.state.accounts().len(), 1);

        app.undo();
        assert_eq!(app.state.accounts().len(), 0);

        app.redo();
        assert_eq!(app.state.accounts().len(), 1);
    }
}

// Snapshot tests for rendering
#[cfg(test)]
mod snapshot_tests {
    #[test]
    fn test_scenario_screen_render() {
        let state = create_test_state();
        let output = render_to_string::<ScenarioScreen>(&state);
        insta::assert_snapshot!(output);
    }
}
```

### Tasks

- [ ] **6.1** Create `test_utils` module with state factories
- [ ] **6.2** Add integration tests for account CRUD workflow
- [ ] **6.3** Add integration tests for event CRUD workflow
- [ ] **6.4** Add integration tests for scenario management
- [ ] **6.5** Add event replay tests (dispatch sequence → expected state)
- [ ] **6.6** Set up `insta` for snapshot testing
- [ ] **6.7** Add snapshot tests for key screens
- [ ] **6.8** Create `MockStorage` for I/O testing
- [ ] **6.9** Create `MockSimulationBackend` for worker testing

### Estimated Impact
- Catch regressions in state management
- Verify complex workflows
- Visual regression detection for screens
- Testable I/O and simulation layers

---

## Priority Order

1. **Phase 2: Typed Forms** - Immediate safety improvement, enables other work
2. **Phase 5.5-5.8: Consolidate Duplications** - Quick wins, reduce tech debt
3. **Phase 1: Component Extraction** - Major maintainability improvement
4. **Phase 4.1-4.3: Input Handlers** - Quick wins, reduce duplication
5. **Phase 3: Event Architecture** - Foundation for undo/redo and testing
6. **Phase 5: Data Layer** - Important but less urgent
7. **Phase 6: Testing** - Builds on other phases
8. **Phase 4.4+: Full Input System** - Nice to have

---

## Quick Wins (Can Do Anytime)

These are isolated improvements that don't require larger refactoring:

- [x] Extract `PERCENTILE_TOLERANCE` constant - Created `util/percentiles.rs` with constant and helpers
- [x] Extract `find_percentile_value()` helper - Added `find_percentile_value()`, `find_percentile_result()`, `find_percentile_result_pair()`, `PercentileSet`
- [x] Create `focused_block()` style helper - Created `util/styles.rs` with `focused_block()`, `focused_block_with_help()`
- [x] Add `atomic_write()` function - Created `util/io.rs` with `atomic_write()`, `atomic_write_bytes()`
- [x] Extract `yes_no_options()` to common module - Created `util/common.rs` with `yes_no_options()`, `no_yes_options()`
- [x] Consolidate duplicate `parse_yes_no()` functions - Unified in `util/common.rs`, updated `actions/event.rs` and `actions/effect.rs`
- [x] Add style constants - Added `FOCUS_COLOR`, `HEADER_COLOR`, `POSITIVE_COLOR`, `NEGATIVE_COLOR`, etc. in `util/styles.rs`

---

## Metrics to Track

| Metric | Current | After Phase 1 | After All |
|--------|---------|---------------|-----------|
| PortfolioProfilesScreen LOC | 1,315 | ✓ Done | ~600 |
| EventsScreen LOC | 908 | ✓ Done | ~400 |
| ResultsScreen LOC | 846 | ✓ Done | ~500 |
| Total screen LOC | 3,069 | ✓ Done | ~1,500 |
| Magic index usages | ~50 | ~50 | 0 (label-based API ready) |
| Duplicate code patterns | ~500 LOC | ~200 LOC | ~50 LOC |
| Test coverage (screens) | 0% | 0% | ~60% |
| Undo/redo support | No | No | Yes |

---

## Notes

- All phases are designed to be incremental - no big bang rewrites
- Hybrid migration strategy allows mixing old and new patterns
- Each phase provides standalone value even if later phases aren't completed
- Event architecture is optional but highly recommended for undo/redo
