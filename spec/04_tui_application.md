# TUI Application Architecture

## Overview

The terminal UI (`finplan` crate) provides an interactive interface for creating and managing financial simulation scenarios using the ratatui framework.

## Module Structure

```
finplan/src/
├── main.rs              # Entry point, CLI args
├── app.rs               # Main App struct, event loop
├── lib.rs               # Public exports
├── screens/             # Tab content
│   ├── portfolio_profiles.rs
│   ├── scenario.rs
│   ├── events.rs
│   └── results.rs
├── state/               # Application state
│   ├── app_state.rs     # Root state
│   ├── tabs.rs          # Tab definitions
│   ├── modal.rs         # Modal state
│   ├── modal_action.rs  # Action dispatch
│   ├── screen_state.rs  # Per-screen state
│   ├── panels.rs        # Collapsible panels
│   └── context.rs       # Modal context
├── components/          # Reusable UI components
│   ├── tab_bar.rs
│   ├── status_bar.rs
│   ├── portfolio_overview.rs
│   └── collapsible.rs
├── modals/              # Modal dialogs
│   ├── form.rs          # Generic form builder
│   ├── picker.rs        # Selection picker
│   ├── confirm.rs       # Confirmation dialog
│   ├── message.rs       # Info/error messages
│   ├── text_input.rs    # Text input
│   └── scenario_picker.rs
├── actions/             # Business logic handlers
│   ├── scenario.rs      # Scenario CRUD
│   ├── profile.rs       # Return profile ops
│   ├── account.rs       # Account management
│   ├── event.rs         # Event configuration
│   ├── holding.rs       # Asset holdings
│   ├── effect.rs        # Event effects
│   ├── config.rs        # Tax/inflation config
│   └── wizard.rs        # Multi-step creation
├── data/                # Data layer
│   ├── storage.rs       # File persistence
│   ├── app_data.rs      # In-memory data
│   ├── portfolio_data.rs
│   ├── parameters_data.rs
│   ├── profiles_data.rs
│   ├── events_data.rs
│   └── convert.rs       # Type conversions
└── util/
    └── format.rs        # Currency formatting
```

## Application Flow

### Startup

```
main.rs
  └── App::with_data_dir(path)
        ├── Check for old format (~/.finplan.yaml)
        ├── Migrate if needed
        └── Load from ~/.finplan/scenarios/
```

### Event Loop

```
App::run()
  └── loop:
        ├── terminal.draw(|frame| self.draw(frame))
        │     ├── Render tab bar
        │     ├── Render active screen
        │     ├── Render status bar
        │     └── Render modal overlay
        │
        └── self.handle_events()
              ├── Modal key handling (if active)
              ├── Global shortcuts (q, Ctrl+C, Ctrl+S, Esc)
              ├── Tab bar navigation
              └── Screen-specific handling
```

## Screens (Tabs)

### TabId Enum

```rust
pub enum TabId {
    PortfolioProfiles,  // Return profiles and accounts
    Scenario,           // Simulation parameters
    Events,             // Life events
    Results,            // Monte Carlo results
}
```

### Portfolio/Profiles Screen

- List of return profiles (Fixed, Historical, Monte Carlo)
- List of accounts by category (Bank, Investment, Property, Liability)
- Holdings editor for investment accounts

### Scenario Screen

- Simulation parameters (start date, duration, birth date)
- Tax configuration (federal brackets)
- Inflation settings
- Live net worth calculation

### Events Screen

- List of configured events
- Trigger configuration
- Effect management (add/edit/delete)

### Results Screen

- Monte Carlo statistics
- Wealth projection charts
- Percentile breakdown

## State Management

### AppState

```rust
pub struct AppState {
    pub exit: bool,
    pub active_tab: TabId,
    pub modal: ModalState,
    pub error_message: Option<String>,

    // Data
    pub data_dir: Option<PathBuf>,
    pub scenario_name: String,
    pub scenarios: HashMap<String, SimulationData>,
    pub dirty_scenarios: HashSet<String>,
    pub scenario_summaries: HashMap<String, ScenarioSummary>,

    // Per-screen state
    pub portfolio_profiles_state: PortfolioProfilesState,
    pub scenario_state: ScenarioState,
    pub events_state: EventsState,
    pub results_state: ResultsState,
}
```

### Modal State

```rust
pub enum ModalState {
    None,
    Form(FormModal),
    Confirm(ConfirmModal),
    Picker(PickerModal),
    Message(MessageModal),
}
```

### Dirty Tracking

Scenarios are marked "dirty" when modified:
- `state.mark_modified()` - Mark current scenario as dirty
- `state.has_unsaved_changes()` - Check for any dirty scenarios
- `state.save_all_dirty()` - Save all modified scenarios

## Action System

### Modal Flow

1. Screen triggers modal with `ModalAction`
2. User interacts with modal
3. `ModalResult::Confirmed(action, value)` dispatched
4. `App::handle_modal_result()` routes to action handler
5. Handler returns `ActionResult`:

```rust
pub enum ActionResult {
    Done(Option<ModalState>),      // Success, optionally show next modal
    Modified(Option<ModalState>),  // Success + mark scenario dirty
    Error(String),                 // Show error message
}
```

### Action Categories

| Module | Actions |
|--------|---------|
| `scenario.rs` | New, Load, SaveAs, Duplicate, Delete, Import, Export |
| `account.rs` | Create, Edit, Delete (with type pickers) |
| `profile.rs` | Create, Edit, Delete return profiles |
| `holding.rs` | Add, Edit, Delete asset holdings |
| `event.rs` | Create, Edit, Delete, Trigger builders |
| `effect.rs` | Add, Edit, Delete event effects |
| `config.rs` | Tax brackets, Inflation settings |

## Data Persistence

### Directory Structure

```
~/.finplan/
├── config.yaml           # { active_scenario: "retirement" }
├── summaries.yaml        # Cached Monte Carlo stats per scenario
└── scenarios/
    ├── retirement.yaml
    ├── aggressive.yaml
    └── conservative.yaml
```

### SimulationData (per scenario)

```yaml
portfolios:
  name: "My Portfolio"
  profiles: [...]
  accounts: [...]
parameters:
  start_date: 2025-01-01
  duration_years: 30
  birth_date: 1980-06-15
events: [...]
```

### Migration

Old single-file format (`~/.finplan.yaml`) is auto-migrated on first run:
1. Parse old format
2. Create new directory structure
3. Save each scenario to individual file
4. Backup old file as `.finplan.yaml.backup`

## Key Bindings

### Global

| Key | Action |
|-----|--------|
| `q` | Quit |
| `Ctrl+C` | Quit |
| `Ctrl+S` | Save all |
| `Esc` | Clear error / Exit mode |
| `1-4` | Switch tabs |
| `Tab` | Next tab |
| `Shift+Tab` | Previous tab |

### Navigation

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `h` / `←` | Move left / collapse |
| `l` / `→` | Move right / expand |
| `Enter` | Select / Edit |

### Actions

| Key | Action |
|-----|--------|
| `a` | Add new item |
| `e` | Edit selected |
| `d` | Delete selected |
| `r` | Run simulation |

## Component Trait

All screens and components implement:

```rust
pub trait Component {
    fn render(&self, frame: &mut Frame, area: Rect, state: &AppState);
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult;
}

pub enum EventResult {
    Handled,
    NotHandled,
    Exit,
}
```

## Error Handling

- Errors display in status bar (red background)
- Clear with `Esc` key
- Modal errors close modal and show message
- File I/O errors logged to stderr and shown to user
