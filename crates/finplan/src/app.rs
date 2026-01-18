use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

use crate::components::{Component, EventResult, status_bar::StatusBar, tab_bar::TabBar};
use crate::modals::{ModalResult, handle_modal_key, render_modal};
use crate::screens::{
    events::EventsScreen, portfolio_profiles::PortfolioProfilesScreen,
    results::ResultsScreen, scenario::ScenarioScreen,
};
use crate::state::{AppState, ModalState, TabId};

pub struct App {
    state: AppState,
    tab_bar: TabBar,
    status_bar: StatusBar,
    portfolio_profiles_screen: PortfolioProfilesScreen,
    scenario_screen: ScenarioScreen,
    events_screen: EventsScreen,
    results_screen: ResultsScreen,
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
            tab_bar: TabBar::new(),
            status_bar: StatusBar::new(),
            portfolio_profiles_screen: PortfolioProfilesScreen::new(),
            scenario_screen: ScenarioScreen::new(),
            events_screen: EventsScreen::new(),
            results_screen: ResultsScreen::new(),
        }
    }

    /// Create app with a specific config file path
    /// Loads existing data if the file exists, otherwise creates default with sample data
    pub fn with_config_path(config_path: PathBuf) -> Self {
        let state = if config_path.exists() {
            match AppState::load_from_file(config_path.clone()) {
                Ok(mut state) => {
                    state.config_path = Some(config_path);
                    state
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to load config from {:?}: {:?}",
                        config_path, e
                    );
                    eprintln!("Starting with default configuration.");
                    let mut state = AppState::default();
                    state.config_path = Some(config_path);
                    state
                }
            }
        } else {
            // File doesn't exist, create default with sample data
            let mut state = AppState::default();
            state.config_path = Some(config_path);
            state
        };

        Self {
            state,
            tab_bar: TabBar::new(),
            status_bar: StatusBar::new(),
            portfolio_profiles_screen: PortfolioProfilesScreen::new(),
            scenario_screen: ScenarioScreen::new(),
            events_screen: EventsScreen::new(),
            results_screen: ResultsScreen::new(),
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

        // Auto-save on exit
        if let Some(path) = &self.state.config_path
            && let Err(e) = self.state.save_to_file(path)
        {
            eprintln!("Warning: Failed to save config to {:?}: {:?}", path, e);
        }

        Ok(())
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
            TabId::PortfolioProfiles => self.portfolio_profiles_screen.render(frame, area, &self.state),
            TabId::Scenario => self.scenario_screen.render(frame, area, &self.state),
            TabId::Events => self.events_screen.render(frame, area, &self.state),
            TabId::Results => self.results_screen.render(frame, area, &self.state),
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
                    self.handle_modal_result(action, value);
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
            KeyCode::Esc => {
                // Clear error message on Esc
                self.state.clear_error();
                return;
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
            TabId::PortfolioProfiles => self.portfolio_profiles_screen.handle_key(key_event, &mut self.state),
            TabId::Scenario => self.scenario_screen.handle_key(key_event, &mut self.state),
            TabId::Events => self.events_screen.handle_key(key_event, &mut self.state),
            TabId::Results => self.results_screen.handle_key(key_event, &mut self.state),
        };

        match result {
            EventResult::Exit => self.state.exit = true,
            _ => {}
        }
    }

    fn handle_modal_result(&mut self, action: crate::state::ModalAction, value: String) {
        use crate::state::ModalAction;

        match action {
            ModalAction::SaveAs => {
                // Save current scenario with the given name
                self.state.save_scenario_as(&value);
                self.state.modal = ModalState::Message(crate::state::MessageModal::info(
                    "Success",
                    &format!("Scenario saved as '{}'", value),
                ));
            }
            ModalAction::Load => {
                // Switch to the selected scenario
                if self.state.app_data.simulations.contains_key(&value) {
                    self.state.switch_scenario(&value);
                    self.state.modal = ModalState::Message(crate::state::MessageModal::info(
                        "Success",
                        &format!("Switched to scenario '{}'", value),
                    ));
                } else {
                    self.state.modal = ModalState::Message(crate::state::MessageModal::error(
                        "Error",
                        &format!("Scenario '{}' not found", value),
                    ));
                }
            }
            ModalAction::SwitchTo => {
                // Just switch to the scenario without showing a message
                if self.state.app_data.simulations.contains_key(&value) {
                    self.state.switch_scenario(&value);
                }
                self.state.modal = ModalState::None;
            }
        }
    }
}
