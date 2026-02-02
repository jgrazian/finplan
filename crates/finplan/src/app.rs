use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use rand::RngCore;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

use crate::actions::ActionResult;
use crate::components::{Component, EventResult, status_bar::StatusBar, tab_bar::TabBar};
use crate::data::keybindings_data::KeybindingsConfig;
use crate::data::storage::DataDirectory;
use crate::modals::{
    ConfirmedValue, MessageModal, ModalAction, ModalResult, ModalState, handle_modal_key,
    render_modal,
};
use crate::screens::{
    ModalHandler, analysis::AnalysisScreen, events::EventsScreen,
    portfolio_profiles::PortfolioProfilesScreen, results::ResultsScreen, scenario::ScenarioScreen,
};
use crate::state::{AppState, PercentileView, ResultsState, SimulationStatus, TabId};
use crate::worker::{SimulationResponse, SimulationWorker};

pub struct App {
    state: AppState,
    worker: SimulationWorker,
    tab_bar: TabBar,
    status_bar: StatusBar,
    portfolio_profiles_screen: PortfolioProfilesScreen,
    scenario_screen: ScenarioScreen,
    events_screen: EventsScreen,
    results_screen: ResultsScreen,
    analysis_screen: AnalysisScreen,
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
            worker: SimulationWorker::new(),
            tab_bar: TabBar,
            status_bar: StatusBar,
            portfolio_profiles_screen: PortfolioProfilesScreen,
            scenario_screen: ScenarioScreen,
            events_screen: EventsScreen,
            results_screen: ResultsScreen,
            analysis_screen: AnalysisScreen,
        }
    }

    /// Create app with a data directory path
    /// Handles migration from old single-file format if needed
    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        let state = Self::load_or_migrate(data_dir);

        Self {
            state,
            worker: SimulationWorker::new(),
            tab_bar: TabBar,
            status_bar: StatusBar,
            portfolio_profiles_screen: PortfolioProfilesScreen,
            scenario_screen: ScenarioScreen,
            events_screen: EventsScreen,
            results_screen: ResultsScreen,
            analysis_screen: AnalysisScreen,
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
        const POLL_TIMEOUT: Duration = Duration::from_millis(50);

        while !self.state.exit {
            terminal.draw(|frame| self.draw(frame))?;

            // Process any pending worker responses (non-blocking)
            self.process_worker_responses();

            // Update progress if Monte Carlo is running
            if let SimulationStatus::RunningMonteCarlo { total, .. } = self.state.simulation_status
            {
                let current = self.worker.get_progress();
                self.state.simulation_status =
                    SimulationStatus::RunningMonteCarlo { current, total };
            }

            // Update progress if batch is running
            if let SimulationStatus::RunningBatch {
                scenario_total,
                iteration_total,
                current_scenario_name,
                ..
            } = &self.state.simulation_status
            {
                let scenario_index = self.worker.get_batch_scenario_index();
                let iteration_current = self.worker.get_progress();
                self.state.simulation_status = SimulationStatus::RunningBatch {
                    scenario_index,
                    scenario_total: *scenario_total,
                    iteration_current,
                    iteration_total: *iteration_total,
                    current_scenario_name: current_scenario_name.clone(),
                };
            }

            // Update progress if sweep analysis is running
            if self.state.analysis_state.running {
                self.state.analysis_state.current_point = self.worker.get_progress();
                self.state.analysis_state.total_points = self.worker.get_batch_scenario_total();
            }

            // Check for pending simulation requests
            self.dispatch_pending_simulation();

            // Poll for input events with timeout (allows UI to update during simulation)
            if event::poll(POLL_TIMEOUT)? {
                self.handle_events()?;
            }
        }

        // Shutdown worker thread
        self.worker.shutdown();

        // No auto-save on exit - user must explicitly save with Ctrl+S
        if self.state.has_unsaved_changes() {
            let unsaved: Vec<_> = self.state.dirty_scenarios.iter().cloned().collect();
            tracing::info!(scenarios = ?unsaved, "Exiting with unsaved changes");
        }

        Ok(())
    }

    /// Process responses from the background simulation worker
    fn process_worker_responses(&mut self) {
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
                    self.state.results_state.viewing_monte_carlo = false;
                    self.state.simulation_status = SimulationStatus::Idle;

                    // Switch to results tab
                    self.state.active_tab = TabId::Results;
                }
                SimulationResponse::MonteCarloComplete {
                    stored_result,
                    preview_summary,
                    default_tui_result,
                    default_core_result,
                } => {
                    // Store results
                    self.state.simulation_result = Some(default_tui_result);
                    self.state.core_simulation_result = Some(default_core_result);

                    // Update scenario preview
                    if let Some(preview) = &mut self.state.scenario_state.projection_preview {
                        preview.mc_summary = Some(preview_summary.clone());
                    }

                    // Store full MC result (unbox)
                    let iterations = stored_result.stats.num_iterations;
                    let success_rate = stored_result.stats.success_rate;
                    self.state.monte_carlo_result = Some(*stored_result);

                    // Reset results state for MC viewing
                    self.state.results_state = ResultsState::default();
                    self.state.results_state.viewing_monte_carlo = true;
                    self.state.results_state.percentile_view = PercentileView::P50;
                    self.state.simulation_status = SimulationStatus::Idle;

                    // Update scenario summary cache
                    self.state.update_current_scenario_summary();

                    // Show completion modal
                    self.state.modal = ModalState::Message(MessageModal::info(
                        "Monte Carlo Complete",
                        &format!(
                            "{} iterations | {:.1}% success rate",
                            iterations,
                            success_rate * 100.0
                        ),
                    ));
                }
                SimulationResponse::Cancelled => {
                    self.state.simulation_status = SimulationStatus::Idle;
                    self.state.modal = ModalState::Message(MessageModal::info(
                        "Cancelled",
                        "Simulation cancelled.",
                    ));
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
                    // Store the summary for this scenario
                    self.state
                        .scenario_state
                        .scenario_summaries
                        .insert(scenario_name.clone(), summary);

                    // Update batch status with scenario name
                    if let SimulationStatus::RunningBatch {
                        scenario_index,
                        scenario_total,
                        iteration_total,
                        ..
                    } = &self.state.simulation_status
                    {
                        self.state.simulation_status = SimulationStatus::RunningBatch {
                            scenario_index: *scenario_index,
                            scenario_total: *scenario_total,
                            iteration_current: 0,
                            iteration_total: *iteration_total,
                            current_scenario_name: Some(scenario_name),
                        };
                    }
                }
                SimulationResponse::BatchComplete { completed_count } => {
                    self.state.simulation_status = SimulationStatus::Idle;
                    self.state.scenario_state.batch_running = false;

                    // Persist summaries to disk
                    self.state.save_scenario_summaries();

                    // Show completion modal
                    self.state.modal = ModalState::Message(MessageModal::info(
                        "Batch Run Complete",
                        &format!("Ran Monte Carlo on {} scenarios.", completed_count),
                    ));
                }
                SimulationResponse::SweepProgress { current, total } => {
                    // Update analysis progress
                    self.state.analysis_state.current_point = current;
                    self.state.analysis_state.total_points = total;
                }
                SimulationResponse::SweepComplete { results } => {
                    self.state.analysis_state.running = false;
                    self.state.simulation_status = SimulationStatus::Idle;

                    // Convert SweepResults to AnalysisResults
                    let analysis_results = convert_sweep_to_analysis_results(&results);
                    self.state.analysis_state.results = Some(analysis_results);

                    // Show completion modal
                    let total_points = self.state.analysis_state.total_points;
                    self.state.modal = ModalState::Message(MessageModal::info(
                        "Analysis Complete",
                        &format!("Evaluated {} parameter combinations.", total_points),
                    ));
                }
            }
        }
    }

    /// Check for and dispatch pending simulation requests to the worker
    fn dispatch_pending_simulation(&mut self) {
        use crate::data::convert::to_simulation_config;
        use crate::state::PendingSimulation;
        use crate::worker::SimulationRequest;

        let pending = self.state.pending_simulation.take();
        if let Some(request) = pending {
            match request {
                PendingSimulation::Single
                | PendingSimulation::MonteCarlo { .. }
                | PendingSimulation::MonteCarloConvergence { .. } => {
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
                            let seed = rand::rng().next_u64();
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
                        PendingSimulation::MonteCarloConvergence {
                            min_iterations,
                            max_iterations,
                            relative_threshold,
                            metric,
                        } => {
                            self.worker.send(SimulationRequest::MonteCarloConvergence {
                                config,
                                min_iterations,
                                max_iterations,
                                relative_threshold,
                                metric,
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
                PendingSimulation::SweepAnalysis { sweep_config } => {
                    // Build simulation config for current scenario
                    let config = match self.state.to_simulation_config() {
                        Ok(c) => c,
                        Err(e) => {
                            self.state.analysis_state.running = false;
                            self.state.simulation_status = SimulationStatus::Idle;
                            self.state.set_error(format!("Config error: {}", e));
                            return;
                        }
                    };

                    let birth_date = self.state.data().parameters.birth_date.clone();
                    let start_date = self.state.data().parameters.start_date.clone();

                    self.worker.send(SimulationRequest::SweepAnalysis {
                        config,
                        sweep_config,
                        birth_date,
                        start_date,
                    });
                }
            }
        }
    }

    /// Save all dirty scenarios
    fn save_all(&mut self) {
        match self.state.save_all_dirty() {
            Ok(count) => {
                if count > 0 {
                    self.state.modal = ModalState::Message(MessageModal::info(
                        "Saved",
                        &format!("Saved {} scenario(s)", count),
                    ));
                } else {
                    self.state.modal = ModalState::Message(MessageModal::info(
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
            TabId::Analysis => self.analysis_screen.render(frame, area, &self.state),
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
                ModalResult::AmountFieldActivated(field_idx) => {
                    self.handle_amount_field_activated(field_idx);
                }
                ModalResult::TriggerFieldActivated(field_idx) => {
                    self.handle_trigger_field_activated(field_idx);
                }
                ModalResult::Continue | ModalResult::FieldChanged(_) => {}
            }
            return;
        }

        // Global key bindings (using configurable keybindings)
        if KeybindingsConfig::matches(&key_event, &self.state.keybindings.global.quit) {
            self.state.exit = true;
            return;
        }
        if KeybindingsConfig::matches(&key_event, &self.state.keybindings.global.save) {
            // Ctrl+S: Save all dirty scenarios
            self.save_all();
            return;
        }
        if KeybindingsConfig::matches(&key_event, &self.state.keybindings.global.cancel) {
            // Cancel running simulation first
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
            TabId::Analysis => self.analysis_screen.handle_key(key_event, &mut self.state),
        };

        if result == EventResult::Exit {
            self.state.exit = true
        }
    }

    fn handle_modal_result(&mut self, action: ModalAction, value: ConfirmedValue) {
        // Delegate to screen-specific handlers based on action type
        // Each screen handles its own domain actions
        let result = if self.portfolio_profiles_screen.handles(&action) {
            self.portfolio_profiles_screen
                .handle_modal_result(&mut self.state, action, &value)
        } else if self.events_screen.handles(&action) {
            self.events_screen
                .handle_modal_result(&mut self.state, action, &value)
        } else if self.scenario_screen.handles(&action) {
            self.scenario_screen
                .handle_modal_result(&mut self.state, action, &value)
        } else if self.analysis_screen.handles(&action) {
            self.analysis_screen
                .handle_modal_result(&mut self.state, action, &value)
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

    /// Handle when an Amount field is activated in a form
    fn handle_amount_field_activated(&mut self, field_idx: usize) {
        use crate::actions::launch_amount_picker;
        use crate::modals::context::{EffectContext, ModalContext};

        // Get the current form's context to determine what effect we're editing
        let ModalState::Form(form) = &self.state.modal else {
            return;
        };

        // Extract effect context from the form
        let Some(ModalContext::Effect(effect_ctx)) = &form.context else {
            return;
        };

        // Get event/effect indices and effect type
        let (event_idx, effect_idx, effect_type) = match effect_ctx {
            EffectContext::Edit {
                event,
                effect,
                effect_type,
            } => (*event, *effect, effect_type.clone()),
            EffectContext::Add { event, effect_type } => (*event, 0, effect_type.clone()),
            EffectContext::Existing { .. } => {
                // For existing context, we don't have the effect type
                // This shouldn't normally happen for amount editing
                return;
            }
        };

        // Get the current amount from the field
        let current_amount = form
            .fields
            .get(field_idx)
            .and_then(|f| f.as_amount())
            .cloned()
            .unwrap_or_else(|| crate::data::events_data::AmountData::fixed(0.0));

        // Store the current form so we can restore it after amount editing
        self.state.pending_effect_form = Some(form.clone());

        // Launch the amount picker
        let result = launch_amount_picker(
            &self.state,
            event_idx,
            effect_idx,
            field_idx,
            effect_type,
            &current_amount,
        );

        // Apply the result
        if let ActionResult::Done(Some(modal)) = result {
            self.state.modal = modal;
        }
    }

    /// Handle when a Trigger field is activated in a form
    fn handle_trigger_field_activated(&mut self, field_idx: usize) {
        use crate::modals::context::{
            IndexContext, ModalContext, TriggerChildSlot, TriggerContext,
        };
        use crate::modals::{ModalAction, PickerModal};

        // Get the current form's context
        let ModalState::Form(form) = &self.state.modal else {
            return;
        };

        match &form.context {
            // Case 1: Editing an existing event's trigger
            Some(ModalContext::Index(IndexContext::Event(idx))) => {
                let event_index = *idx;

                // Close the current form and open the trigger type picker
                let trigger_types = vec![
                    "Date".to_string(),
                    "Age".to_string(),
                    "Repeating".to_string(),
                    "Manual".to_string(),
                    "Account Balance".to_string(),
                    "Net Worth".to_string(),
                    "Relative to Event".to_string(),
                ];

                self.state.modal = ModalState::Picker(
                    PickerModal::new(
                        "Select New Trigger Type",
                        trigger_types,
                        ModalAction::EDIT_TRIGGER_TYPE_PICK,
                    )
                    .with_typed_context(ModalContext::Trigger(
                        TriggerContext::EditStart { event_index },
                    )),
                );
            }

            // Case 2: Unified repeating form - editing start/end conditions
            Some(ModalContext::Trigger(TriggerContext::RepeatingBuilder(builder)))
                if builder.unified_form_mode =>
            {
                // Field 3 = Start, Field 4 = End
                let slot = match field_idx {
                    3 => TriggerChildSlot::Start,
                    4 => TriggerChildSlot::End,
                    _ => return, // Unknown field
                };

                // Store the current form so we can return to it after editing
                let form_clone = form.clone();
                self.state.pending_repeating_form = Some(form_clone);

                // Update builder to track which slot we're editing
                let mut builder = builder.clone();
                builder.editing_slot = Some(slot);

                // Show child trigger type picker
                let title = match slot {
                    TriggerChildSlot::Start => "Select Start Condition",
                    TriggerChildSlot::End => "Select End Condition",
                };

                let none_option = match slot {
                    TriggerChildSlot::Start => "None (Start Immediately)",
                    TriggerChildSlot::End => "None (Run Forever)",
                };

                let options = vec![
                    none_option.to_string(),
                    "Date".to_string(),
                    "Age".to_string(),
                    "Account Balance".to_string(),
                    "Net Worth".to_string(),
                    "Relative to Event".to_string(),
                ];

                self.state.modal = ModalState::Picker(
                    PickerModal::new(title, options, ModalAction::PICK_CHILD_TRIGGER_TYPE)
                        .with_typed_context(ModalContext::Trigger(
                            TriggerContext::RepeatingBuilder(builder),
                        )),
                );
            }

            _ => {}
        }
    }
}

/// Convert core SweepResults to TUI AnalysisResults
fn convert_sweep_to_analysis_results(
    results: &finplan_core::analysis::SweepResults,
) -> crate::state::AnalysisResults {
    use crate::state::{AnalysisMetricType, AnalysisResults};
    use finplan_core::analysis::AnalysisMetric;
    use std::collections::HashMap;

    let mut metric_results = HashMap::new();

    // Helper to extract and reshape metric data
    let extract_metric = |core_metric: &AnalysisMetric, scale: f64| -> Vec<Vec<f64>> {
        let (values, _, cols) = results.get_metric_grid(core_metric);
        if cols <= 1 {
            // 1D: each row is a single value
            values.into_iter().map(|v| vec![v * scale]).collect()
        } else {
            // 2D: reshape into rows x cols
            values
                .chunks(cols)
                .map(|chunk| chunk.iter().map(|v| v * scale).collect())
                .collect()
        }
    };

    // Extract all metrics
    metric_results.insert(
        AnalysisMetricType::SuccessRate,
        extract_metric(&AnalysisMetric::SuccessRate, 100.0), // Convert to percentage
    );
    metric_results.insert(
        AnalysisMetricType::P50FinalNetWorth,
        extract_metric(&AnalysisMetric::Percentile { percentile: 50 }, 1.0),
    );
    metric_results.insert(
        AnalysisMetricType::P5FinalNetWorth,
        extract_metric(&AnalysisMetric::Percentile { percentile: 5 }, 1.0),
    );
    metric_results.insert(
        AnalysisMetricType::P95FinalNetWorth,
        extract_metric(&AnalysisMetric::Percentile { percentile: 95 }, 1.0),
    );
    metric_results.insert(
        AnalysisMetricType::LifetimeTaxes,
        extract_metric(&AnalysisMetric::LifetimeTaxes, 1.0),
    );

    AnalysisResults {
        param1_values: results.param1_values().to_vec(),
        param2_values: results.param2_values().to_vec(),
        metric_results,
        param1_label: results.param1_label().to_string(),
        param2_label: results.param2_label().to_string(),
    }
}
