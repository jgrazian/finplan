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

use crate::event::AppKeyEvent;
use crate::platform::web::{WebStorage, WebWorker};
use crate::platform::{SimulationWorker, Storage};
use crate::state::{AppState, SimulationStatus, TabId};

/// Web application state wrapped for callback access.
struct WebApp {
    state: AppState,
    storage: WebStorage,
    worker: WebWorker,
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
        }
    }

    /// Handle a key event.
    fn handle_key(&mut self, key: AppKeyEvent) {
        // Global key bindings
        match key.code {
            KeyCode::Char('q') if key.no_modifiers() => {
                // Can't really exit in web, but we could show a message
                tracing::info!("Exit requested (q pressed)");
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
                // Clear error
                self.state.clear_error();
                return;
            }
            _ => {}
        }

        // Tab navigation
        match key.code {
            KeyCode::Char('1') if key.no_modifiers() => {
                self.state.active_tab = TabId::PortfolioProfiles;
                return;
            }
            KeyCode::Char('2') if key.no_modifiers() => {
                self.state.active_tab = TabId::Scenario;
                return;
            }
            KeyCode::Char('3') if key.no_modifiers() => {
                self.state.active_tab = TabId::Events;
                return;
            }
            KeyCode::Char('4') if key.no_modifiers() => {
                self.state.active_tab = TabId::Results;
                return;
            }
            KeyCode::Char('5') if key.no_modifiers() => {
                self.state.active_tab = TabId::Optimize;
                return;
            }
            KeyCode::Tab if key.shift() => {
                self.state.active_tab = prev_tab(self.state.active_tab);
                return;
            }
            KeyCode::Tab => {
                self.state.active_tab = next_tab(self.state.active_tab);
                return;
            }
            _ => {}
        }

        // TODO: Delegate to screen-specific handlers
        // For now, just log unhandled keys
        tracing::debug!(key = ?key.code, "Unhandled key event");
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
}

/// Get next tab in order.
fn next_tab(current: TabId) -> TabId {
    let idx = current.index();
    let next_idx = (idx + 1) % TabId::ALL.len();
    TabId::from_index(next_idx).unwrap_or(current)
}

/// Get previous tab in order.
fn prev_tab(current: TabId) -> TabId {
    let idx = current.index();
    let prev_idx = if idx == 0 {
        TabId::ALL.len() - 1
    } else {
        idx - 1
    };
    TabId::from_index(prev_idx).unwrap_or(current)
}

/// WASM entry point.
#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    // Set up panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize logging
    crate::init_logging_web();

    tracing::info!("FinPlan web version starting");

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

        // Process any pending worker responses
        app.process_worker_responses();

        // Render the UI
        render_app(frame, &app.state);
    });

    Ok(())
}

/// Render the application UI.
fn render_app(frame: &mut ratatui::Frame, state: &AppState) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Content
            Constraint::Length(2), // Status bar
        ])
        .split(frame.area());

    // Render tab bar
    let tab_titles: Vec<Line> = vec![
        Line::from("1:Portfolio"),
        Line::from("2:Scenario"),
        Line::from("3:Events"),
        Line::from("4:Results"),
        Line::from("5:Optimize"),
    ];

    let selected_idx = match state.active_tab {
        TabId::PortfolioProfiles => 0,
        TabId::Scenario => 1,
        TabId::Events => 2,
        TabId::Results => 3,
        TabId::Optimize => 4,
    };

    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title("FinPlan"))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .select(selected_idx);

    frame.render_widget(tabs, chunks[0]);

    // Render content area
    let content_block = Block::default()
        .borders(Borders::ALL)
        .title(format!("{:?}", state.active_tab));

    let data = state.data();
    let content_text = match state.active_tab {
        TabId::PortfolioProfiles => {
            format!(
                "Portfolio: {}\nAccounts: {}",
                data.portfolios.name,
                data.portfolios.accounts.len()
            )
        }
        TabId::Scenario => {
            format!(
                "Birth Date: {}\nStart Date: {}\nDuration: {} years",
                data.parameters.birth_date,
                data.parameters.start_date,
                data.parameters.duration_years
            )
        }
        TabId::Events => {
            format!("Events: {}", data.events.len())
        }
        TabId::Results => {
            if let Some(result) = &state.simulation_result {
                format!(
                    "Years simulated: {}\nFinal net worth: ${:.2}",
                    result.years.len(),
                    result.years.last().map(|y| y.net_worth).unwrap_or(0.0)
                )
            } else {
                "No simulation results. Press 'r' to run.".to_string()
            }
        }
        TabId::Optimize => "Optimization parameters".to_string(),
    };

    let content = Paragraph::new(content_text).block(content_block);
    frame.render_widget(content, chunks[1]);

    // Render status bar
    let status_text = if let Some(error) = &state.error_message {
        vec![Line::from(vec![
            Span::styled("Error: ", Style::default().fg(Color::Red)),
            Span::raw(error.as_str()),
        ])]
    } else {
        let scenario_name = &state.current_scenario;
        let dirty = if state.dirty_scenarios.contains(scenario_name) {
            " [modified]"
        } else {
            ""
        };

        vec![Line::from(vec![
            Span::raw(format!("Scenario: {}{} | ", scenario_name, dirty)),
            Span::styled(
                format!("{:?}", state.simulation_status),
                Style::default().fg(Color::Cyan),
            ),
        ])]
    };

    let status = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::TOP))
        .style(Style::default().fg(Color::Gray));

    frame.render_widget(status, chunks[2]);
}
