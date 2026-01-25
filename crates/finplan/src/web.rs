//! Web entry point for finplan using ratzilla.
//!
//! This module provides the WASM entry point that uses ratzilla for
//! rendering ratatui widgets in the browser.

use std::cell::RefCell;
use std::rc::Rc;

use ratatui::Terminal;
use ratzilla::event::{KeyCode, KeyEvent as RatzillaKeyEvent};
use ratzilla::{DomBackend, WebRenderer};
use wasm_bindgen::prelude::*;

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::components::status_bar::StatusBar;
use crate::components::tab_bar::TabBar;
use crate::components::{Component, EventResult};
use crate::event::AppKeyEvent;
use crate::modals::{ConfirmedValue, ModalResult, handle_modal_key, render_modal};
use crate::platform::web::{WebStorage, WebWorker};
use crate::platform::{SimulationRequest, SimulationWorker, Storage};
use crate::screens::{
    ModalHandler, events::EventsScreen, optimize::OptimizeScreen,
    portfolio_profiles::PortfolioProfilesScreen, results::ResultsScreen, scenario::ScenarioScreen,
};
use crate::state::{AppState, ModalAction, ModalState, SimulationStatus, TabId};

/// Web application state wrapped for callback access.
struct WebApp {
    state: AppState,
    storage: WebStorage,
    worker: WebWorker,
    // UI components
    tab_bar: TabBar,
    status_bar: StatusBar,
    // Screen instances
    portfolio_profiles_screen: PortfolioProfilesScreen,
    scenario_screen: ScenarioScreen,
    events_screen: EventsScreen,
    results_screen: ResultsScreen,
    optimize_screen: OptimizeScreen,
}

impl WebApp {
    fn new() -> Self {
        let storage = WebStorage::new();

        // Load state from storage
        let state = match storage.load() {
            Ok(result) => {
                let mut state = AppState::default();
                state.app_data = result.app_data;
                state
                    .scenario_state
                    .scenario_summaries
                    .extend(result.scenario_summaries);
                state.current_scenario = result.current_scenario;
                state
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load from storage, using defaults");
                AppState::default()
            }
        };

        Self {
            state,
            storage,
            worker: WebWorker::new(),
            tab_bar: TabBar,
            status_bar: StatusBar,
            portfolio_profiles_screen: PortfolioProfilesScreen,
            scenario_screen: ScenarioScreen,
            events_screen: EventsScreen,
            results_screen: ResultsScreen,
            optimize_screen: OptimizeScreen,
        }
    }

    /// Handle a key event.
    fn handle_key(&mut self, key: AppKeyEvent) {
        // Handle modal first if active (uses shared modal handling)
        if !matches!(self.state.modal, ModalState::None) {
            match handle_modal_key(key, &mut self.state) {
                ModalResult::Confirmed(action, value) => {
                    self.handle_modal_result(action, *value);
                }
                ModalResult::Cancelled => {
                    self.state.modal = ModalState::None;
                }
                ModalResult::Continue | ModalResult::FieldChanged(_) => {}
            }
            return;
        }

        // Global key bindings
        match key.code {
            KeyCode::Char('q') if key.no_modifiers() => {
                // Can't really exit in web, but we could show a message
                tracing::info!("Exit requested (q pressed)");
                return;
            }
            KeyCode::Char('c') if key.ctrl() => {
                // Ctrl+C: Would exit on native, just log on web
                tracing::info!("Ctrl+C pressed");
                return;
            }
            KeyCode::Char('s') if key.ctrl() => {
                // Save current scenario
                self.save_current();
                return;
            }
            KeyCode::Esc => {
                // Cancel running simulation
                if self.state.simulation_status.is_running() {
                    self.worker.cancel();
                    self.state.simulation_status = SimulationStatus::Idle;
                    return;
                }
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

        // Try tab bar first - it checks for editing states before switching tabs
        let result = self.tab_bar.handle_key(key.clone(), &mut self.state);
        if result != EventResult::NotHandled {
            return;
        }

        // Delegate to active screen handler (using shared screen implementations)
        let result = match self.state.active_tab {
            TabId::PortfolioProfiles => self
                .portfolio_profiles_screen
                .handle_key(key, &mut self.state),
            TabId::Scenario => self.scenario_screen.handle_key(key, &mut self.state),
            TabId::Events => self.events_screen.handle_key(key, &mut self.state),
            TabId::Results => self.results_screen.handle_key(key, &mut self.state),
            TabId::Optimize => self.optimize_screen.handle_key(key, &mut self.state),
        };

        if result == EventResult::Exit {
            tracing::info!("Exit requested from screen");
        }
    }

    /// Handle modal result (dispatch to appropriate screen handler).
    fn handle_modal_result(&mut self, action: ModalAction, value: ConfirmedValue) {
        // Legacy string value for handlers not yet migrated
        let legacy_value = value.to_legacy_string();

        // Delegate to screen-specific handlers based on action type
        let result = if self.portfolio_profiles_screen.handles(&action) {
            self.portfolio_profiles_screen.handle_modal_result(
                &mut self.state,
                action,
                &value,
                &legacy_value,
            )
        } else if self.scenario_screen.handles(&action) {
            self.scenario_screen
                .handle_modal_result(&mut self.state, action, &value, &legacy_value)
        } else if self.events_screen.handles(&action) {
            self.events_screen
                .handle_modal_result(&mut self.state, action, &value, &legacy_value)
        } else if self.optimize_screen.handles(&action) {
            self.optimize_screen
                .handle_modal_result(&mut self.state, action, &value, &legacy_value)
        } else {
            crate::actions::ActionResult::close()
        };

        // Process the result
        use crate::actions::ActionResult;
        match result {
            ActionResult::Modified(modal) | ActionResult::Done(modal) => {
                self.state.modal = modal.unwrap_or(ModalState::None);
                self.state.mark_modified();
            }
            ActionResult::Error(msg) => {
                self.state.set_error(msg);
                self.state.modal = ModalState::None;
            }
        }
    }

    /// Save the current scenario.
    fn save_current(&mut self) {
        let name = self.state.current_scenario.clone();
        let data = self.state.data().clone();
        if let Err(e) = self.storage.save_scenario(&name, &data) {
            self.state.set_error(format!("Failed to save: {}", e));
        } else {
            self.state.dirty_scenarios.remove(&name);
            tracing::info!(scenario = name, "Scenario saved");
        }
    }

    /// Draw the UI using the actual screen renderers.
    fn draw(&mut self, frame: &mut ratatui::Frame) {
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

    /// Render the currently active screen.
    fn render_active_screen(&mut self, frame: &mut ratatui::Frame, area: Rect) {
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

    /// Process worker responses.
    fn process_worker_responses(&mut self) {
        use crate::platform::SimulationResponse;
        use crate::state::{PercentileView, ResultsState};

        while let Some(response) = self.worker.try_recv() {
            match response {
                SimulationResponse::SingleComplete {
                    tui_result,
                    core_result,
                } => {
                    self.state.simulation_result = Some(tui_result);
                    self.state.core_simulation_result = Some(core_result);
                    self.state.monte_carlo_result = None;
                    self.state.results_state = ResultsState::default();
                    self.state.simulation_status = SimulationStatus::Idle;
                    self.state.active_tab = TabId::Results;
                }
                SimulationResponse::MonteCarloComplete {
                    stored_result,
                    preview_summary,
                    default_tui_result,
                    default_core_result,
                } => {
                    self.state.simulation_result = Some(default_tui_result);
                    self.state.core_simulation_result = Some(default_core_result);

                    if let Some(preview) = &mut self.state.scenario_state.projection_preview {
                        preview.mc_summary = Some(preview_summary);
                    }

                    self.state.monte_carlo_result = Some(*stored_result);
                    self.state.results_state = ResultsState::default();
                    self.state.results_state.viewing_monte_carlo = true;
                    self.state.results_state.percentile_view = PercentileView::P50;
                    self.state.simulation_status = SimulationStatus::Idle;
                }
                SimulationResponse::Cancelled => {
                    self.state.simulation_status = SimulationStatus::Idle;
                }
                SimulationResponse::Error(msg) => {
                    self.state.simulation_status = SimulationStatus::Idle;
                    self.state.set_error(msg);
                }
                SimulationResponse::Progress { current, total } => {
                    self.state.simulation_status =
                        SimulationStatus::RunningMonteCarlo { current, total };
                }
                SimulationResponse::BatchScenarioComplete {
                    scenario_name,
                    summary,
                } => {
                    self.state
                        .scenario_state
                        .scenario_summaries
                        .insert(scenario_name, summary);
                }
                SimulationResponse::BatchComplete { .. } => {
                    self.state.simulation_status = SimulationStatus::Idle;
                    self.state.scenario_state.batch_running = false;
                }
            }
        }
    }

    /// Check for and dispatch pending simulation requests to the worker.
    fn dispatch_pending_simulation(&mut self) {
        use crate::data::convert::to_simulation_config;
        use crate::state::PendingSimulation;

        let pending = self.state.pending_simulation.take();
        if let Some(request) = pending {
            match request {
                PendingSimulation::Single | PendingSimulation::MonteCarlo { .. } => {
                    // Build simulation config for current scenario
                    let config = match self.state.to_simulation_config() {
                        Ok(c) => c,
                        Err(e) => {
                            self.state.simulation_status = SimulationStatus::Idle;
                            self.state.set_error(format!("Config error: {}", e));
                            return;
                        }
                    };

                    let birth_date = self.state.data().parameters.birth_date.clone();
                    let start_date = self.state.data().parameters.start_date.clone();

                    match request {
                        PendingSimulation::Single => {
                            let seed = js_sys::Math::random().to_bits();
                            self.worker.send(SimulationRequest::Single {
                                config,
                                seed,
                                birth_date,
                                start_date,
                            });
                        }
                        PendingSimulation::MonteCarlo { iterations } => {
                            self.worker.send(SimulationRequest::MonteCarlo {
                                config,
                                iterations,
                                birth_date,
                                start_date,
                            });
                        }
                        _ => unreachable!(),
                    }
                }
                PendingSimulation::Batch { iterations } => {
                    // Build configs for all scenarios
                    let mut scenarios = Vec::new();
                    let mut errors = Vec::new();

                    for (name, data) in &self.state.app_data.simulations {
                        match to_simulation_config(data) {
                            Ok(config) => {
                                let birth_date = data.parameters.birth_date.clone();
                                let start_date = data.parameters.start_date.clone();
                                scenarios.push((name.clone(), config, birth_date, start_date));
                            }
                            Err(e) => {
                                errors.push(format!("{}: {}", name, e));
                            }
                        }
                    }

                    if !errors.is_empty() {
                        tracing::warn!(errors = ?errors, "Some scenarios failed to build config");
                    }

                    if scenarios.is_empty() {
                        self.state.simulation_status = SimulationStatus::Idle;
                        self.state.scenario_state.batch_running = false;
                        self.state
                            .set_error("No valid scenarios to run".to_string());
                        return;
                    }

                    // Sort scenarios by name for consistent ordering
                    scenarios.sort_by(|a, b| a.0.cmp(&b.0));

                    // Update status with first scenario name
                    let first_name = scenarios.first().map(|(n, _, _, _)| n.clone());
                    self.state.simulation_status = SimulationStatus::RunningBatch {
                        scenario_index: 0,
                        scenario_total: scenarios.len(),
                        iteration_current: 0,
                        iteration_total: iterations,
                        current_scenario_name: first_name,
                    };

                    self.worker.send(SimulationRequest::Batch {
                        scenarios,
                        iterations,
                    });
                }
            }
        }
    }
}

/// Set up event listener to prevent default browser behavior for captured keys.
fn setup_prevent_default() {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let window = web_sys::window().expect("no global window");
    let document = window.document().expect("no document");

    let closure = Closure::<dyn FnMut(_)>::new(move |event: web_sys::KeyboardEvent| {
        let key = event.key();
        let ctrl = event.ctrl_key() || event.meta_key();

        // Prevent default for keys we want to capture
        let should_prevent =
            matches!(key.as_str(), "Tab") || (ctrl && matches!(key.to_lowercase().as_str(), "s"));

        if should_prevent {
            event.prevent_default();
        }
    });

    document
        .add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())
        .expect("failed to add keydown listener");

    // Prevent the closure from being dropped
    closure.forget();
}

/// WASM entry point.
#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    // Set up panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize logging to browser console
    crate::init_logging_web();

    tracing::info!("FinPlan web version starting");

    // Prevent default browser behavior for keys we capture (Tab, Ctrl+S)
    setup_prevent_default();

    // Create the app state
    let app = Rc::new(RefCell::new(WebApp::new()));

    // Create the terminal
    let backend = DomBackend::new().map_err(|e| JsValue::from_str(&e.to_string()))?;
    let terminal: Terminal<DomBackend> =
        Terminal::new(backend).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Set up key event handler
    let app_clone = Rc::clone(&app);
    terminal.on_key_event(move |key_event: RatzillaKeyEvent| {
        let key: AppKeyEvent = (&key_event).into();
        app_clone.borrow_mut().handle_key(key);
    });

    // Set up draw callback
    terminal.draw_web(move |frame| {
        let mut app = app.borrow_mut();

        // Dispatch any pending simulation requests
        app.dispatch_pending_simulation();

        // Process any pending worker responses
        app.process_worker_responses();

        // Render the UI using actual screen renderers
        app.draw(frame);
    });

    Ok(())
}
