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

The left column is split evenly: Events on top and Parameters below. Details
and Timeline retain their own columns. Cycle panel focus with Tab/Shift+Tab;
use the focused Parameters panel to add, edit, or delete named Money, Rate,
Date, and Age inputs.

Named inputs live in `SimulationData.named_parameters`, separate from the
existing `parameters` field for scenario settings (birth date, duration, taxes,
etc.). Old scenario files load with an empty named-input list. The converter
registers typed core values and resolves references by name to `ParameterId`.

Event type lists offer Date and Age triggers. Each editor has a **Value source**
selector: **Enter value**, or one of the matching Date/Age parameters. The same
choice appears in recurring start/end conditions.

Every effect **Amount** field supports inline static entry and expression mode.
Press **Enter** to edit the amount or type a number directly; press **x** to enter
expression mode. Expressions use the core DSL, for example
`inflation($MonthlySpending)` or `$WithdrawalRate * balance("Vanguard")`.
**Tab** completes a parameter name after `$`; repeated Tab cycles matches and
**Shift+Tab** cycles backwards. Names containing spaces are quoted automatically.
Press **?** on an amount field (in either mode), or while navigating a form with
amounts, to open scrollable expression help. It includes syntax, variables,
examples, all built-in functions, and source/target and gross/net rules. Use
**Up/Down**, **j/k**, **Page Up/Down**, or **Home/End** to scroll; **Esc**, **Enter**,
or **?** returns to the unchanged form draft, cursor, and completion state.
**Enter** finishes the field, **F10/Ctrl+S** submits the form, and **Esc** reverts
an edit. Outside editing, **x** switches modes again and preserves both drafts.
Compiler errors stay in the form with the entered text and error byte range.
Outer `gross(...)` / `net(...)` annotations update the Amount Type selection for
income, sales, and sweeps; other effects reject these annotations.

Existing saved amount builders reopen as editable expressions. New expression
amounts retain source text in scenario YAML and compile against the same names,
asset IDs, and typed parameters used by the simulation. This applies to income,
expenses, purchases, sales, sweeps, balance adjustments, and cash transfers.
Date/Age pickers and other static settings retain their typed value editors;
the amount DSL does not define expressions for those settings.

When a parameter is selected in a Date/Age picker, its current value is shown
in grey, read-only fields. Switching to Enter value makes those fields editable, using a copy of
the displayed value; it does not change the shared parameter. Existing literal
values and parameter references are preselected when reopening their editors.
Changing a parameter's value affects every event referencing it. Renaming updates
nested references; deleting or changing the type of a referenced parameter is
blocked until its references are removed, including references in disabled events.

The Analysis tab selects sweep dimensions exclusively from these named Parameters.
The picker excludes variables already selected for analysis. Range forms use dollars
for Money, percentages for Rate, YYYY-MM-DD dates for Date, and years/months for Age.
Date sweeps advance in whole days; Age sweeps advance in whole months. Each sweep
updates the shared parameter once and rebinds all event references for that run.
Charts and saved results retain the variable names and their typed units.

Renaming a variable updates its saved and active sweeps. Remove a variable's sweep
before deleting it or changing its type. Old event-based sweep selections are ignored
when loading a scenario; choose the corresponding named variables in Analysis.
The core sweep API still supports event targets for other callers.

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
