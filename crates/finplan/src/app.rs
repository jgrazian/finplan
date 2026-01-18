use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

use crate::actions::{self, ActionContext, ActionResult};
use crate::components::{Component, EventResult, status_bar::StatusBar, tab_bar::TabBar};
use crate::modals::{ModalResult, handle_modal_key, render_modal};
use crate::screens::{
    events::EventsScreen, portfolio_profiles::PortfolioProfilesScreen, results::ResultsScreen,
    scenario::ScenarioScreen,
};
use crate::state::{
    AccountAction, AppState, ConfigAction, EffectAction, EventAction, HoldingAction, ModalAction,
    ModalState, ProfileAction, ScenarioAction, TabId,
};

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
        match self.state.save() {
            Ok(()) => {
                if let Some(path) = &self.state.config_path {
                    eprintln!("Saved config to {:?}", path);
                }
            }
            Err(e) => {
                if let Some(path) = &self.state.config_path {
                    eprintln!("Warning: Failed to save config to {:?}: {:?}", path, e);
                } else {
                    eprintln!("Warning: No config path set, changes not saved: {:?}", e);
                }
            }
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
            TabId::PortfolioProfiles => {
                self.portfolio_profiles_screen
                    .render(frame, area, &self.state)
            }
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
                // Let holdings editing mode handle Esc first
                if self.state.portfolio_profiles_state.editing_holdings {
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
        };

        match result {
            EventResult::Exit => self.state.exit = true,
            _ => {}
        }
    }

    fn handle_modal_result(&mut self, action: ModalAction, value: String) {
        // Extract context from the modal before we clear it
        let context = match &self.state.modal {
            ModalState::Form(form) => form.context.clone(),
            ModalState::Confirm(confirm) => confirm.context.clone(),
            _ => None,
        };

        let ctx = ActionContext::new(context.as_ref(), &value);

        // Dispatch to domain-specific action handlers
        let result = match action {
            // Scenario actions
            ModalAction::Scenario(ScenarioAction::SaveAs) => {
                actions::handle_save_as(&mut self.state, &value)
            }
            ModalAction::Scenario(ScenarioAction::Load) => {
                actions::handle_load_scenario(&mut self.state, &value)
            }
            ModalAction::Scenario(ScenarioAction::SwitchTo) => {
                actions::handle_switch_to(&mut self.state, &value)
            }

            // Account actions
            ModalAction::Account(AccountAction::PickCategory) => {
                actions::handle_category_pick(&value)
            }
            ModalAction::Account(AccountAction::PickType) => actions::handle_type_pick(&value),
            ModalAction::Account(AccountAction::Create) => {
                actions::handle_create_account(&mut self.state, ctx)
            }
            ModalAction::Account(AccountAction::Edit) => {
                actions::handle_edit_account(&mut self.state, ctx)
            }
            ModalAction::Account(AccountAction::Delete) => {
                actions::handle_delete_account(&mut self.state, ctx)
            }

            // Profile actions
            ModalAction::Profile(ProfileAction::PickType) => {
                actions::handle_profile_type_pick(&value)
            }
            ModalAction::Profile(ProfileAction::Create) => {
                actions::handle_create_profile(&mut self.state, ctx)
            }
            ModalAction::Profile(ProfileAction::Edit) => {
                actions::handle_edit_profile(&mut self.state, ctx)
            }
            ModalAction::Profile(ProfileAction::Delete) => {
                actions::handle_delete_profile(&mut self.state, ctx)
            }

            // Holding actions
            ModalAction::Holding(HoldingAction::PickReturnProfile) => ActionResult::close(),
            ModalAction::Holding(HoldingAction::Add) => {
                actions::handle_add_holding(&mut self.state, ctx)
            }
            ModalAction::Holding(HoldingAction::Edit) => {
                actions::handle_edit_holding(&mut self.state, ctx)
            }
            ModalAction::Holding(HoldingAction::Delete) => {
                actions::handle_delete_holding(&mut self.state, ctx)
            }

            // Config actions
            ModalAction::Config(ConfigAction::PickFederalBrackets) => {
                actions::handle_federal_brackets_pick(&mut self.state, &value)
            }
            ModalAction::Config(ConfigAction::EditTax) => {
                actions::handle_edit_tax_config(&mut self.state, ctx)
            }
            ModalAction::Config(ConfigAction::PickInflationType) => {
                actions::handle_inflation_type_pick(&mut self.state, &value)
            }
            ModalAction::Config(ConfigAction::EditInflation) => {
                actions::handle_edit_inflation(&mut self.state, ctx)
            }

            // Event actions
            ModalAction::Event(EventAction::PickTriggerType) => {
                actions::handle_trigger_type_pick(&self.state, &value)
            }
            ModalAction::Event(EventAction::PickEventReference) => {
                actions::handle_event_reference_pick(&value)
            }
            ModalAction::Event(EventAction::PickInterval) => actions::handle_interval_pick(&value),
            ModalAction::Event(EventAction::Create) => {
                actions::handle_create_event(&mut self.state, ctx)
            }
            ModalAction::Event(EventAction::Edit) => {
                actions::handle_edit_event(&mut self.state, ctx)
            }
            ModalAction::Event(EventAction::Delete) => {
                actions::handle_delete_event(&mut self.state, ctx)
            }

            // Effect actions
            ModalAction::Effect(EffectAction::Manage) => {
                actions::handle_manage_effects(&self.state, &value)
            }
            ModalAction::Effect(EffectAction::PickType) => ActionResult::close(),
            ModalAction::Effect(EffectAction::PickTypeForAdd) => {
                actions::handle_effect_type_for_add(&self.state, &value)
            }
            ModalAction::Effect(EffectAction::PickAccountForEffect) => {
                actions::handle_account_for_effect_pick(&value)
            }
            ModalAction::Effect(EffectAction::Add) => {
                actions::handle_add_effect(&mut self.state, ctx)
            }
            ModalAction::Effect(EffectAction::Delete) => {
                actions::handle_delete_effect(&mut self.state, ctx)
            }
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
