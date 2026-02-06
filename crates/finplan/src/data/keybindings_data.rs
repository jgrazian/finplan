//! Keybindings configuration data structures.
//!
//! Defines the structure for customizable keyboard shortcuts that can be
//! serialized to/from `~/.finplan/keybindings.yaml`.

use serde::{Deserialize, Serialize};

/// Root keybindings configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    /// Global keybindings (work everywhere)
    pub global: GlobalBindings,
    /// Navigation keybindings (consistent across panels)
    pub navigation: NavigationBindings,
    /// Tab-specific keybindings
    pub tabs: TabBindings,
}

/// Global keybindings that work everywhere in the app.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalBindings {
    pub quit: Vec<String>,
    pub save: Vec<String>,
    pub cancel: Vec<String>,
    pub tab_1: Vec<String>,
    pub tab_2: Vec<String>,
    pub tab_3: Vec<String>,
    pub tab_4: Vec<String>,
    pub tab_5: Vec<String>,
}

impl Default for GlobalBindings {
    fn default() -> Self {
        Self {
            quit: vec!["q".into(), "ctrl+c".into()],
            save: vec!["ctrl+s".into()],
            cancel: vec!["esc".into()],
            tab_1: vec!["1".into()],
            tab_2: vec!["2".into()],
            tab_3: vec!["3".into()],
            tab_4: vec!["4".into()],
            tab_5: vec!["5".into()],
        }
    }
}

/// Navigation keybindings used consistently across panels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NavigationBindings {
    pub up: Vec<String>,
    pub down: Vec<String>,
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub next_panel: Vec<String>,
    pub prev_panel: Vec<String>,
    pub reorder_up: Vec<String>,
    pub reorder_down: Vec<String>,
    pub confirm: Vec<String>,
}

impl Default for NavigationBindings {
    fn default() -> Self {
        Self {
            up: vec!["k".into(), "up".into()],
            down: vec!["j".into(), "down".into()],
            left: vec!["h".into(), "left".into()],
            right: vec!["l".into(), "right".into()],
            next_panel: vec!["tab".into()],
            prev_panel: vec!["shift+tab".into()],
            reorder_up: vec!["shift+k".into(), "shift+up".into()],
            reorder_down: vec!["shift+j".into(), "shift+down".into()],
            confirm: vec!["enter".into()],
        }
    }
}

/// Tab-specific keybindings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TabBindings {
    pub portfolio: PortfolioBindings,
    pub events: EventsBindings,
    pub scenario: ScenarioBindings,
    pub results: ResultsBindings,
    pub analyze: AnalyzeBindings,
}

/// Keybindings for the Portfolio & Profiles tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PortfolioBindings {
    pub add: Vec<String>,
    pub edit: Vec<String>,
    pub delete: Vec<String>,
    pub map: Vec<String>,
    pub suggest: Vec<String>,
    pub suggest_all: Vec<String>,
    pub history_mode: Vec<String>,
    pub block_size: Vec<String>,
}

impl Default for PortfolioBindings {
    fn default() -> Self {
        Self {
            add: vec!["a".into()],
            edit: vec!["e".into()],
            delete: vec!["d".into()],
            map: vec!["m".into()],
            suggest: vec!["a".into()],
            suggest_all: vec!["shift+a".into()],
            history_mode: vec!["y".into()],
            block_size: vec!["b".into()],
        }
    }
}

/// Keybindings for the Events tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EventsBindings {
    pub add: Vec<String>,
    pub edit: Vec<String>,
    pub delete: Vec<String>,
    pub copy: Vec<String>,
    pub toggle: Vec<String>,
    pub effects: Vec<String>,
}

impl Default for EventsBindings {
    fn default() -> Self {
        Self {
            add: vec!["a".into()],
            edit: vec!["e".into()],
            delete: vec!["d".into()],
            copy: vec!["c".into()],
            toggle: vec!["t".into()],
            effects: vec!["f".into()],
        }
    }
}

/// Keybindings for the Scenario tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScenarioBindings {
    pub run: Vec<String>,
    pub monte_carlo: Vec<String>,
    pub monte_carlo_convergence: Vec<String>,
    pub run_all: Vec<String>,
    pub new: Vec<String>,
    pub copy: Vec<String>,
    pub save_as: Vec<String>,
    pub load: Vec<String>,
    pub edit_params: Vec<String>,
    pub import: Vec<String>,
    pub export: Vec<String>,
    pub preview: Vec<String>,
    pub toggle_real: Vec<String>,
}

impl Default for ScenarioBindings {
    fn default() -> Self {
        Self {
            run: vec!["r".into()],
            monte_carlo: vec!["m".into()],
            monte_carlo_convergence: vec!["shift+m".into()],
            run_all: vec!["shift+r".into()],
            new: vec!["n".into()],
            copy: vec!["c".into()],
            save_as: vec!["s".into()],
            load: vec!["l".into()],
            edit_params: vec!["e".into()],
            import: vec!["i".into()],
            export: vec!["x".into()],
            preview: vec!["p".into()],
            toggle_real: vec!["$".into()],
        }
    }
}

/// Keybindings for the Results tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ResultsBindings {
    pub prev_year: Vec<String>,
    pub next_year: Vec<String>,
    pub first_year: Vec<String>,
    pub last_year: Vec<String>,
    pub toggle_real: Vec<String>,
    pub cycle_percentile: Vec<String>,
    pub cycle_filter: Vec<String>,
    pub run: Vec<String>,
    pub monte_carlo: Vec<String>,
    pub toggle_granularity: Vec<String>,
}

impl Default for ResultsBindings {
    fn default() -> Self {
        Self {
            prev_year: vec!["h".into(), "left".into()],
            next_year: vec!["l".into(), "right".into()],
            first_year: vec!["home".into()],
            last_year: vec!["end".into()],
            toggle_real: vec!["$".into()],
            cycle_percentile: vec!["v".into()],
            cycle_filter: vec!["f".into()],
            run: vec!["r".into()],
            monte_carlo: vec!["m".into()],
            toggle_granularity: vec!["g".into()],
        }
    }
}

/// Keybindings for the Analyze tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalyzeBindings {
    pub add_param: Vec<String>,
    pub delete_param: Vec<String>,
    pub run: Vec<String>,
    pub settings: Vec<String>,
    pub toggle_metric: Vec<String>,
    /// Configure chart (in Results panel)
    pub configure_chart: Vec<String>,
    /// Add new chart (in Results panel)
    pub add_chart: Vec<String>,
    /// Delete chart (in Results panel)
    pub delete_chart: Vec<String>,
}

impl Default for AnalyzeBindings {
    fn default() -> Self {
        Self {
            add_param: vec!["a".into()],
            delete_param: vec!["d".into()],
            run: vec!["r".into()],
            settings: vec!["s".into()],
            toggle_metric: vec!["t".into()],
            configure_chart: vec!["c".into(), "enter".into()],
            add_chart: vec!["+".into()],
            delete_chart: vec!["-".into()],
        }
    }
}
