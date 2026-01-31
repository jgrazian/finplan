# FinPlan Development Plan

Use this file to track current and future work plans.
Edit as needed to track implementation state, assumptions, and reasoning.

---

## Current Work: TUI Controls Improvement

### Overview

Improve the controls and control display in the TUI with three main goals:
1. Redesign status bar to show global commands (right) and tab commands (left)
2. Create a customizable keybindings system with config.yaml serialization
3. Review and rationalize the current control scheme

---

## Phase 1: Status Bar Redesign - COMPLETED

### Goal
Update `crates/finplan/src/components/status_bar.rs` to display:
- **Right side**: True global commands (work everywhere)
- **Left side**: Tab-specific commands (work within current tab)
- **Panel boxes (inline)**: Panel-local commands (continue existing behavior)

### Implementation - DONE

- [x] Split help text into `get_tab_help_text()` and `get_global_help_text()`
- [x] Modified `StatusBar::render()` to use Layout for left/right regions
- [x] Left side: Tab-specific commands (DarkGray)
- [x] Right side: Global commands (White, right-aligned): "1-5: tabs | Ctrl+S: save | q: quit"
- [x] Preserved error message and simulation status priority rendering
- [x] Dirty indicator [*] shown on left before tab help

---

## Phase 2: Keybindings Configuration System - INFRASTRUCTURE COMPLETE

### Goal
Create a keybindings configuration that can be:
- Serialized to `~/.finplan/keybindings.yaml`
- Loaded at startup
- Used throughout the app for key matching

### Implementation - INFRASTRUCTURE DONE

**New Files Created:**
- [x] `crates/finplan/src/data/keybindings_data.rs` - All keybinding data structures
- [x] `crates/finplan/src/keybindings.rs` - Key matching utilities (`key_to_string`, `matches`, `load_or_default`, `save`)

**Integration - DONE:**
- [x] `crates/finplan/src/data/mod.rs` - Added keybindings_data export
- [x] `crates/finplan/src/lib.rs` - Added keybindings module export
- [x] `crates/finplan/src/data/storage.rs` - Added `load_keybindings()`, `save_keybindings()`, keybindings in `LoadResult`
- [x] `crates/finplan/src/state/app_state.rs` - Added `keybindings: KeybindingsConfig` field, loaded from storage

**Remaining - Use Keybindings Throughout App:**
- [ ] Update `app.rs` global key handling to use `KeybindingsConfig::matches()`
- [ ] Update `tab_bar.rs` to use keybindings config
- [ ] Update each screen to use config bindings instead of hardcoded keys
- [ ] Update `selectable_list.rs` navigation utilities
- [ ] Generate default keybindings.yaml on first run if not present
- [ ] Update status bar to read from keybindings config for display

---

## Phase 3: Control Scheme Review - COMPLETED

### Key Fixes Applied

1. **Fixed `d` key conflict in Scenario tab** - DONE
   - Changed duplicate scenario from `'d'` to `'c'`
   - Updated help text to show `[c]opy` instead of `[d]up`

2. **Fixed `h` key conflict in Portfolio tab** - DONE
   - Changed history mode toggle from `'h'` to `'y'`
   - Updated help text to show `[y] historical` / `[y] parametric`

### Standardized Control Scheme

#### Global Commands
| Key | Action |
|-----|--------|
| `q`, `Ctrl+C` | Quit |
| `Ctrl+S` | Save all |
| `Esc` | Cancel/clear |
| `1-5` | Switch tabs |

#### Universal Navigation
| Key | Action |
|-----|--------|
| `j/k` or `Up/Down` | Navigate list |
| `h/l` or `Left/Right` | Navigate time (Results) |
| `Tab/Shift+Tab` | Switch panels |
| `Shift+J/K` | Reorder items |
| `Enter` | Confirm/select/edit |

#### Universal CRUD
| Key | Action |
|-----|--------|
| `a` | Add new item |
| `e` | Edit selected item |
| `d` | Delete selected item |
| `c` | Copy/duplicate item |

#### Tab-Specific Commands

**Portfolio Profiles**:
- `Enter`: Edit holdings
- `m`: Map/cycle profile for asset
- `A`: Auto-suggest all profiles
- `y`: Toggle historical/parametric mode
- `b`: Pick block size (historical mode)

**Events**:
- Standard CRUD: `a/e/d/c`
- `t`: Toggle enabled
- `f`: Manage effects

**Scenario**:
- `r`: Run single simulation
- `m`: Monte Carlo (1000 iterations)
- `R`: Run all scenarios (batch)
- `n`: New scenario
- `c`: Copy/duplicate scenario
- `Delete/Backspace`: Delete scenario
- `s`: Save as
- `l`: Load
- `e`: Edit parameters
- `i/x`: Import/export
- `p`: Preview projection
- `$`: Toggle real/nominal

**Results**:
- `h/l`: Year navigation
- `Home/End`: First/last year
- `$`: Toggle real/nominal
- `v`: Cycle percentile
- `f`: Cycle ledger filter

**Optimize**:
- `r`: Run optimization
- `s`: Settings
- `a/d`: Add/delete parameters
- `Enter`: Configure selected

---

## File Changes Summary

### New Files
- `crates/finplan/src/data/keybindings_data.rs` - Keybinding data structures
- `crates/finplan/src/keybindings.rs` - Key parsing and matching utilities

### Modified Files
- `crates/finplan/src/components/status_bar.rs` - Two-column layout
- `crates/finplan/src/data/storage.rs` - Load/save keybindings
- `crates/finplan/src/data/mod.rs` - Export keybindings module
- `crates/finplan/src/lib.rs` - Export keybindings module
- `crates/finplan/src/state/app_state.rs` - Add keybindings field
- `crates/finplan/src/screens/scenario.rs` - Changed duplicate key `d` → `c`
- `crates/finplan/src/components/panels/profiles_panel.rs` - Changed history toggle `h` → `y`

---

## Testing Checklist

- [x] Code compiles without errors
- [x] Clippy passes without warnings
- [x] Status bar shows global commands on right, tab commands on left
- [x] `c` in Scenario now duplicates (was `d`)
- [x] `y` in Portfolio toggles history mode (was `h`)
- [ ] `~/.finplan/keybindings.yaml` can be created manually for custom bindings
- [ ] Keybindings are loaded at startup (infrastructure ready)

---

## Future Work

### Complete Keybindings Integration
The keybindings infrastructure is complete. To fully use it throughout the app:

1. Replace hardcoded `KeyCode::Char('x')` matches with:
   ```rust
   if KeybindingsConfig::matches(&key, &state.keybindings.tabs.events.add) {
       // handle add
   }
   ```

2. Update status bar to dynamically show keys from config:
   ```rust
   let add_key = state.keybindings.tabs.events.add.first().unwrap_or(&"a".to_string());
   format!("{}: add", add_key)
   ```

3. Consider adding a keybindings editor in the TUI settings.
