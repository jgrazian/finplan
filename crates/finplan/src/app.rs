use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

use crate::actions::ActionResult;
use crate::components::{Component, EventResult, status_bar::StatusBar, tab_bar::TabBar};
use crate::data::storage::DataDirectory;
use crate::modals::{ConfirmedValue, ModalResult, handle_modal_key, render_modal};
use crate::screens::{
    ModalHandler, events::EventsScreen, optimize::OptimizeScreen,
    portfolio_profiles::PortfolioProfilesScreen, results::ResultsScreen, scenario::ScenarioScreen,
};
use crate::state::{AppState, ModalAction, ModalState, TabId};

pub struct App {
    state: AppState,
    tab_bar: TabBar,
    status_bar: StatusBar,
    portfolio_profiles_screen: PortfolioProfilesScreen,
    scenario_screen: ScenarioScreen,
    events_screen: EventsScreen,
    results_screen: ResultsScreen,
    optimize_screen: OptimizeScreen,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let state = AppState::default();

        Self {
            state,
            tab_bar: TabBar,
            status_bar: StatusBar,
            portfolio_profiles_screen: PortfolioProfilesScreen,
            scenario_screen: ScenarioScreen,
            events_screen: EventsScreen,
            results_screen: ResultsScreen,
            optimize_screen: OptimizeScreen,
        }
    }

    /// Create app with a data directory path
    /// Handles migration from old single-file format if needed
    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        let state = Self::load_or_migrate(data_dir);

        Self {
            state,
            tab_bar: TabBar,
            status_bar: StatusBar,
            portfolio_profiles_screen: PortfolioProfilesScreen,
            scenario_screen: ScenarioScreen,
            events_screen: EventsScreen,
            results_screen: ResultsScreen,
            optimize_screen: OptimizeScreen,
        }
    }

    /// Load from data directory, migrating from old format if needed
    fn load_or_migrate(data_dir: PathBuf) -> AppState {
        let storage = DataDirectory::new(data_dir.clone());

        // Check if we need to migrate from old format
        let old_config_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".finplan.yaml");

        if !storage.exists() && old_config_path.exists() {
            // Migrate from old format
            match storage.migrate_from_single_file(&old_config_path) {
                Ok(true) => {
                    tracing::info!(
                        from = ?old_config_path,
                        to = ?data_dir,
                        backup = ?old_config_path.with_extension("yaml.backup"),
                        "Migrated data from old format"
                    );
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(error = ?e, "Migration failed, starting with defaults");
                    let mut state = AppState::default();
                    state.data_dir = Some(data_dir);
                    return state;
                }
            }
        }

        // Load from data directory
        match AppState::load_from_data_dir(data_dir.clone()) {
            Ok(state) => state,
            Err(e) => {
                tracing::warn!(path = ?data_dir, error = ?e, "Failed to load, using defaults");
                let mut state = AppState::default();
                state.data_dir = Some(data_dir);
                state
            }
        }
    }
}

impl App {
    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> color_eyre::Result<()> {
        while !self.state.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }

        // No auto-save on exit - user must explicitly save with Ctrl+S
        if self.state.has_unsaved_changes() {
            let unsaved: Vec<_> = self.state.dirty_scenarios.iter().cloned().collect();
            tracing::info!(scenarios = ?unsaved, "Exiting with unsaved changes");
        }

        Ok(())
    }

    /// Save all dirty scenarios
    fn save_all(&mut self) {
        match self.state.save_all_dirty() {
            Ok(count) => {
                if count > 0 {
                    self.state.modal = ModalState::Message(crate::state::MessageModal::info(
                        "Saved",
                        &format!("Saved {} scenario(s)", count),
                    ));
                } else {
                    self.state.modal = ModalState::Message(crate::state::MessageModal::info(
                        "No Changes",
                        "No unsaved changes to save",
                    ));
                }
            }
            Err(e) => {
                self.state.set_error(format!("Failed to save: {}", e));
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        // Create main layout: tab bar, content, status bar
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tab bar
                Constraint::Min(0),    // Content
                Constraint::Length(2), // Status bar
            ])
            .split(frame.area());

        // Render tab bar
        self.tab_bar.render(frame, chunks[0], &self.state);

        // Render active screen
        self.render_active_screen(frame, chunks[1]);

        // Render status bar
        self.status_bar.render(frame, chunks[2], &self.state);

        // Render modal overlay (if active)
        render_modal(frame, &self.state);
    }

    fn render_active_screen(&mut self, frame: &mut Frame, area: Rect) {
        match self.state.active_tab {
            TabId::PortfolioProfiles => {
                self.portfolio_profiles_screen
                    .render(frame, area, &self.state)
            }
            TabId::Scenario => self.scenario_screen.render(frame, area, &self.state),
            TabId::Events => self.events_screen.render(frame, area, &self.state),
            TabId::Results => self.results_screen.render(frame, area, &self.state),
            TabId::Optimize => self.optimize_screen.render(frame, area, &self.state),
        }
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        // Handle modal first if active
        if !matches!(self.state.modal, ModalState::None) {
            match handle_modal_key(key_event, &mut self.state) {
                ModalResult::Confirmed(action, value) => {
                    self.handle_modal_result(action, *value);
                }
                ModalResult::Cancelled => {
                    self.state.modal = ModalState::None;
                }
                ModalResult::Continue => {}
            }
            return;
        }

        // Global key bindings
        match key_event.code {
            KeyCode::Char('q') if key_event.modifiers.is_empty() => {
                self.state.exit = true;
                return;
            }
            KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.exit = true;
                return;
            }
            KeyCode::Char('s') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+S: Save all dirty scenarios
                self.save_all();
                return;
            }
            KeyCode::Esc => {
                // Let holdings editing mode handle Esc first
                if self
                    .state
                    .portfolio_profiles_state
                    .account_mode
                    .is_editing_holdings()
                {
                    // Fall through to screen handler
                } else {
                    // Clear error message on Esc
                    self.state.clear_error();
                    return;
                }
            }
            _ => {}
        }

        // Try tab bar first
        let result = self.tab_bar.handle_key(key_event, &mut self.state);
        if result != EventResult::NotHandled {
            return;
        }

        // Then try active screen
        let result = match self.state.active_tab {
            TabId::PortfolioProfiles => self
                .portfolio_profiles_screen
                .handle_key(key_event, &mut self.state),
            TabId::Scenario => self.scenario_screen.handle_key(key_event, &mut self.state),
            TabId::Events => self.events_screen.handle_key(key_event, &mut self.state),
            TabId::Results => self.results_screen.handle_key(key_event, &mut self.state),
            TabId::Optimize => self.optimize_screen.handle_key(key_event, &mut self.state),
        };

        if result == EventResult::Exit {
            self.state.exit = true
        }
    }

    fn handle_modal_result(&mut self, action: ModalAction, value: ConfirmedValue) {
        // Legacy string value for handlers not yet migrated
        let legacy_value = value.to_legacy_string();

        // Delegate to screen-specific handlers based on action type
        // Each screen handles its own domain actions
        let result = if self.portfolio_profiles_screen.handles(&action) {
            self.portfolio_profiles_screen.handle_modal_result(
                &mut self.state,
                action,
                &value,
                &legacy_value,
            )
        } else if self.events_screen.handles(&action) {
            self.events_screen
                .handle_modal_result(&mut self.state, action, &value, &legacy_value)
        } else if self.scenario_screen.handles(&action) {
            self.scenario_screen
                .handle_modal_result(&mut self.state, action, &value, &legacy_value)
        } else if self.optimize_screen.handles(&action) {
            self.optimize_screen
                .handle_modal_result(&mut self.state, action, &value, &legacy_value)
        } else {
            // No handler found - this shouldn't happen with proper coverage
            ActionResult::close()
        };

        // Handle the action result
        self.apply_action_result(result);
    }

    /// Apply the result of an action handler
    fn apply_action_result(&mut self, result: ActionResult) {
        match result {
            ActionResult::Done(modal) => {
                self.state.modal = modal.unwrap_or(ModalState::None);
            }
            ActionResult::Modified(modal) => {
                self.state.mark_modified();
                self.state.modal = modal.unwrap_or(ModalState::None);
            }
            ActionResult::Error(msg) => {
                self.state.set_error(msg);
                self.state.modal = ModalState::None;
            }
        }
    }
}
