use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::Screen;
use crate::actions::ActionResult;
use crate::actions::optimize::{handle_optimize_action, show_objective_picker, show_settings_form};
use crate::components::{Component, EventResult};
use crate::data::keybindings_data::KeybindingsConfig;
use crate::modals::{ConfirmedValue, ModalAction, ModalState, OptimizeAction};
use crate::state::{AppState, OptimizePanel};
use crate::util::format::format_currency;

pub struct OptimizeScreen;

impl OptimizeScreen {
    /// Render the Parameters panel (left 40%)
    fn render_parameters(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = if focused {
            " PARAMETERS [a add, d del, Enter edit] "
        } else {
            " PARAMETERS "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let params = &state.optimize_state.selected_parameters;
        let selected_idx = state.optimize_state.selected_param_index;

        if params.is_empty() {
            let content = vec![
                Line::from(""),
                Line::from("No parameters selected for optimization."),
                Line::from(""),
                Line::from(Span::styled(
                    "Press 'a' to add a parameter.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Parameters you can optimize:",
                    Style::default().fg(Color::Cyan),
                )),
                Line::from("  - Retirement Age"),
                Line::from("  - Contribution Rate"),
                Line::from("  - Withdrawal Amount"),
                Line::from("  - Asset Allocation"),
            ];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        } else {
            let items: Vec<ListItem> = params
                .iter()
                .enumerate()
                .map(|(idx, param)| {
                    let param_name = match param.param_type {
                        crate::state::ParameterType::RetirementAge => "Retirement Age",
                        crate::state::ParameterType::ContributionRate => "Contribution Rate",
                        crate::state::ParameterType::WithdrawalAmount => "Withdrawal Amount",
                        crate::state::ParameterType::AssetAllocation => "Asset Allocation",
                    };

                    let bounds = match param.param_type {
                        crate::state::ParameterType::RetirementAge => {
                            format!("[{:.0} - {:.0} yrs]", param.min_value, param.max_value)
                        }
                        crate::state::ParameterType::ContributionRate
                        | crate::state::ParameterType::WithdrawalAmount => {
                            format!(
                                "[{} - {}]",
                                format_currency(param.min_value),
                                format_currency(param.max_value)
                            )
                        }
                        crate::state::ParameterType::AssetAllocation => {
                            format!(
                                "[{:.0}% - {:.0}% stocks]",
                                param.min_value * 100.0,
                                param.max_value * 100.0
                            )
                        }
                    };

                    let is_selected = idx == selected_idx;
                    let style = if is_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };

                    let prefix = if is_selected { "> " } else { "  " };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, style),
                        Span::styled(param_name, style),
                        Span::raw(" "),
                        Span::styled(bounds, Style::default().fg(Color::DarkGray)),
                    ]))
                })
                .collect();

            let list = List::new(items).block(block);
            frame.render_widget(list, area);
        }
    }

    /// Render the Objective panel (right-top)
    fn render_objective(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = if focused {
            " OBJECTIVE [Enter change, s settings] "
        } else {
            " OBJECTIVE "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let objective_name = match &state.optimize_state.objective {
            crate::state::OptimizationObjectiveSelection::MaxWealthAtDeath => {
                "Maximize Wealth at Death"
            }
            crate::state::OptimizationObjectiveSelection::MaxWealthAtRetirement { .. } => {
                "Maximize Wealth at Retirement"
            }
            crate::state::OptimizationObjectiveSelection::MaxSustainableWithdrawal {
                success_rate,
                ..
            } => {
                return self.render_objective_with_rate(
                    frame,
                    area,
                    block,
                    "Maximize Sustainable Withdrawal",
                    *success_rate,
                    state,
                );
            }
            crate::state::OptimizationObjectiveSelection::MinLifetimeTax => {
                "Minimize Lifetime Taxes"
            }
        };

        let mc_iter = state.optimize_state.mc_iterations;
        let max_iter = state.optimize_state.max_iterations;

        let content = vec![
            Line::from(""),
            Line::from(vec![
                Span::raw("  Objective: "),
                Span::styled(objective_name, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  MC Iterations: "),
                Span::styled(format!("{}", mc_iter), Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::raw("  Max Opt Iterations: "),
                Span::styled(format!("{}", max_iter), Style::default().fg(Color::Green)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  Press 's' to change settings",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(content).block(block);
        frame.render_widget(paragraph, area);
    }

    /// Helper for rendering objective with success rate
    fn render_objective_with_rate(
        &self,
        frame: &mut Frame,
        area: Rect,
        block: Block,
        name: &str,
        success_rate: f64,
        state: &AppState,
    ) {
        let mc_iter = state.optimize_state.mc_iterations;
        let max_iter = state.optimize_state.max_iterations;

        let content = vec![
            Line::from(""),
            Line::from(vec![
                Span::raw("  Objective: "),
                Span::styled(name, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::raw("  Target Success Rate: "),
                Span::styled(
                    format!("{:.0}%", success_rate * 100.0),
                    Style::default().fg(Color::Magenta),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  MC Iterations: "),
                Span::styled(format!("{}", mc_iter), Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::raw("  Max Opt Iterations: "),
                Span::styled(format!("{}", max_iter), Style::default().fg(Color::Green)),
            ]),
        ];

        let paragraph = Paragraph::new(content).block(block);
        frame.render_widget(paragraph, area);
    }

    /// Render the Progress panel (right-middle) with convergence info
    fn render_progress(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = if state.optimize_state.running {
            format!(
                " PROGRESS [{}/{}] ",
                state.optimize_state.current_iteration, state.optimize_state.max_iterations
            )
        } else if focused {
            " PROGRESS [r run optimization] ".to_string()
        } else {
            " PROGRESS ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let convergence_data = &state.optimize_state.convergence_data;

        if convergence_data.is_empty() {
            let content = if state.optimize_state.running {
                vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "  Optimization in progress...",
                        Style::default().fg(Color::Yellow),
                    )),
                    Line::from(""),
                    Line::from("  Running Monte Carlo simulations..."),
                ]
            } else {
                vec![
                    Line::from(""),
                    Line::from("  No optimization data yet."),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  Press 'r' to run optimization.",
                        Style::default().fg(Color::Cyan),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  Make sure you have parameters configured",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(Span::styled(
                        "  before running optimization.",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            };
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        } else {
            // Show text-based progress information with sparkline-style trend
            let current_iter = state.optimize_state.current_iteration;
            let max_iter = state.optimize_state.max_iterations;

            // Check if optimization is complete (has result)
            let is_complete = state.optimize_state.result.is_some();
            let actual_iterations = if let Some(ref result) = state.optimize_state.result {
                result.iterations
            } else {
                current_iter
            };

            // Get latest objective value
            let latest_value = convergence_data.last().map(|(_, v)| *v).unwrap_or(0.0);
            let first_value = convergence_data.first().map(|(_, v)| *v).unwrap_or(0.0);

            // Build progress bar - show 100% if complete, otherwise show progress
            let bar_width = 30;
            let (progress_bar, iteration_display) = if is_complete {
                let filled = bar_width;
                (
                    format!("[{}] Complete", "=".repeat(filled)),
                    format!("{} iterations", actual_iterations),
                )
            } else {
                let progress_pct = (current_iter as f64 / max_iter as f64 * 100.0) as usize;
                let filled = (progress_pct * bar_width / 100).min(bar_width);
                let empty = bar_width - filled;
                (
                    format!(
                        "[{}{}] {}%",
                        "=".repeat(filled),
                        " ".repeat(empty),
                        progress_pct
                    ),
                    format!("{}/{}", current_iter, max_iter),
                )
            };

            // Build sparkline from convergence data (last 20 points)
            let sparkline_chars = [
                ' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}',
                '\u{2587}', '\u{2588}',
            ];
            let recent_data: Vec<f64> = convergence_data
                .iter()
                .rev()
                .take(20)
                .map(|(_, v)| *v)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();

            let sparkline = if !recent_data.is_empty() {
                let min_val = recent_data.iter().cloned().fold(f64::INFINITY, f64::min);
                let max_val = recent_data
                    .iter()
                    .cloned()
                    .fold(f64::NEG_INFINITY, f64::max);
                let range = max_val - min_val;

                recent_data
                    .iter()
                    .map(|v| {
                        if range.abs() < 0.0001 {
                            sparkline_chars[4] // middle height for flat line
                        } else {
                            let normalized = ((v - min_val) / range * 8.0) as usize;
                            sparkline_chars[normalized.min(8)]
                        }
                    })
                    .collect::<String>()
            } else {
                String::new()
            };

            // Calculate improvement
            let improvement = if first_value.abs() > 0.0001 {
                ((latest_value - first_value) / first_value.abs()) * 100.0
            } else {
                0.0
            };

            let improvement_style = if improvement >= 0.0 {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };

            let content = vec![
                Line::from(""),
                Line::from(vec![
                    Span::raw("  Iteration: "),
                    Span::styled(
                        iteration_display,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(progress_bar, Style::default().fg(Color::Green)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::raw(if is_complete {
                        "  Best Objective: "
                    } else {
                        "  Current Objective: "
                    }),
                    Span::styled(
                        format_currency(latest_value),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::raw("  Improvement: "),
                    Span::styled(format!("{:+.1}%", improvement), improvement_style),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::raw("  Trend: "),
                    Span::styled(sparkline, Style::default().fg(Color::Cyan)),
                ]),
            ];

            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        }
    }

    /// Render the Results panel (right-bottom)
    fn render_results(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = " RESULTS ";

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        if let Some(result) = &state.optimize_state.result {
            let status_style = if result.converged {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            };

            let status_text = if result.converged {
                "CONVERGED"
            } else {
                "Max Iterations"
            };

            // Success rate color based on value
            let success_color = if result.success_rate >= 0.95 {
                Color::Green
            } else if result.success_rate >= 0.85 {
                Color::Yellow
            } else {
                Color::Red
            };

            let mut content = vec![
                Line::from(vec![
                    Span::raw("  Status: "),
                    Span::styled(status_text, status_style),
                    Span::raw("  |  "),
                    Span::raw("Iterations: "),
                    Span::styled(
                        format!("{}", result.iterations),
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "  OPTIMAL VALUES:",
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::Green),
                )),
            ];

            for (name, value) in &result.optimal_values {
                let formatted_value = if name.contains("Age") {
                    format!("{:.0} years", value)
                } else if name.contains("Allocation") {
                    format!("{:.1}% stocks", value * 100.0)
                } else {
                    format_currency(*value)
                };

                content.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(name, Style::default().fg(Color::White)),
                    Span::raw(": "),
                    Span::styled(
                        formatted_value,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }

            content.push(Line::from(""));
            content.push(Line::from(Span::styled(
                "  METRICS:",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Blue),
            )));
            content.push(Line::from(vec![
                Span::raw("    Objective Value: "),
                Span::styled(
                    format_currency(result.objective_value),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            content.push(Line::from(vec![
                Span::raw("    Success Rate: "),
                Span::styled(
                    format!("{:.1}%", result.success_rate * 100.0),
                    Style::default()
                        .fg(success_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(if result.success_rate >= 0.95 {
                    "  (Excellent)"
                } else if result.success_rate >= 0.85 {
                    "  (Good)"
                } else {
                    "  (Needs improvement)"
                }),
            ]));

            // Add convergence trend if we have data
            let convergence = &state.optimize_state.convergence_data;
            if convergence.len() >= 2 {
                let first_value = convergence.first().map(|(_, v)| *v).unwrap_or(0.0);
                let last_value = convergence.last().map(|(_, v)| *v).unwrap_or(0.0);
                let improvement = last_value - first_value;
                let improvement_pct = if first_value.abs() > 0.0 {
                    (improvement / first_value.abs()) * 100.0
                } else {
                    0.0
                };

                content.push(Line::from(vec![
                    Span::raw("    Improvement: "),
                    Span::styled(
                        format!("{:+.1}%", improvement_pct),
                        Style::default().fg(if improvement >= 0.0 {
                            Color::Green
                        } else {
                            Color::Red
                        }),
                    ),
                ]));
            }

            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        } else {
            let content = vec![
                Line::from(""),
                Line::from("  No optimization results yet."),
                Line::from(""),
                Line::from(Span::styled(
                    "  1. Add parameters to optimize (a)",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  2. Configure objective (Enter on Objective)",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  3. Run optimization (r)",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        }
    }
}

impl Component for OptimizeScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let panel = state.optimize_state.focused_panel;
        let kb = &state.keybindings;

        // Don't handle keys if optimization is running
        if state.optimize_state.running {
            return EventResult::NotHandled;
        }

        // Panel navigation
        if KeybindingsConfig::matches(&key, &kb.navigation.next_panel) {
            state.optimize_state.focused_panel = panel.next();
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.navigation.prev_panel) {
            state.optimize_state.focused_panel = panel.prev();
            return EventResult::Handled;
        }

        // r: Run optimization (from any panel)
        if KeybindingsConfig::matches(&key, &kb.tabs.optimize.run) {
            let result = crate::actions::optimize::handle_optimize_action(
                state,
                OptimizeAction::RunOptimization,
                "",
            );
            match result {
                crate::actions::ActionResult::Done(modal) => {
                    state.modal = modal.unwrap_or(ModalState::None);
                }
                crate::actions::ActionResult::Modified(modal) => {
                    state.mark_modified();
                    state.modal = modal.unwrap_or(ModalState::None);
                }
                crate::actions::ActionResult::Error(msg) => {
                    state.set_error(msg);
                }
            }
            return EventResult::Handled;
        }

        // s: Settings (from any panel)
        if KeybindingsConfig::matches(&key, &kb.tabs.optimize.settings) {
            let result = show_settings_form(state);
            match result {
                crate::actions::ActionResult::Done(modal)
                | crate::actions::ActionResult::Modified(modal) => {
                    state.modal = modal.unwrap_or(ModalState::None);
                }
                crate::actions::ActionResult::Error(msg) => {
                    state.set_error(msg);
                }
            }
            return EventResult::Handled;
        }

        // Parameter list navigation (j/k or Up/Down in Parameters panel)
        if KeybindingsConfig::matches(&key, &kb.navigation.down) {
            if panel == OptimizePanel::Parameters {
                let param_count = state.optimize_state.selected_parameters.len();
                if param_count > 0 {
                    state.optimize_state.selected_param_index =
                        (state.optimize_state.selected_param_index + 1) % param_count;
                }
            }
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.navigation.up) {
            if panel == OptimizePanel::Parameters {
                let param_count = state.optimize_state.selected_parameters.len();
                if param_count > 0 {
                    if state.optimize_state.selected_param_index == 0 {
                        state.optimize_state.selected_param_index = param_count - 1;
                    } else {
                        state.optimize_state.selected_param_index -= 1;
                    }
                }
            }
            return EventResult::Handled;
        }

        // a: Add parameter
        if KeybindingsConfig::matches(&key, &kb.tabs.optimize.add_param) {
            if panel == OptimizePanel::Parameters {
                let result = crate::actions::optimize::handle_optimize_action(
                    state,
                    OptimizeAction::AddParameter,
                    "",
                );
                match result {
                    crate::actions::ActionResult::Done(modal)
                    | crate::actions::ActionResult::Modified(modal) => {
                        state.modal = modal.unwrap_or(ModalState::None);
                    }
                    crate::actions::ActionResult::Error(msg) => {
                        state.set_error(msg);
                    }
                }
            }
            return EventResult::Handled;
        }

        // d: Delete selected parameter
        if KeybindingsConfig::matches(&key, &kb.tabs.optimize.delete_param) {
            if panel == OptimizePanel::Parameters {
                let params = &mut state.optimize_state.selected_parameters;
                let idx = state.optimize_state.selected_param_index;
                if idx < params.len() {
                    params.remove(idx);
                    // Adjust selection index if needed
                    if state.optimize_state.selected_param_index >= params.len()
                        && !params.is_empty()
                    {
                        state.optimize_state.selected_param_index = params.len() - 1;
                    }
                }
            }
            return EventResult::Handled;
        }

        // Enter: Configure based on panel
        if KeybindingsConfig::matches(&key, &kb.navigation.confirm) {
            match panel {
                OptimizePanel::Parameters => {
                    // Edit selected parameter (if any)
                    if !state.optimize_state.selected_parameters.is_empty() {
                        // For now, just show add dialog since editing is complex
                        let result = crate::actions::optimize::handle_optimize_action(
                            state,
                            OptimizeAction::AddParameter,
                            "",
                        );
                        match result {
                            crate::actions::ActionResult::Done(modal)
                            | crate::actions::ActionResult::Modified(modal) => {
                                state.modal = modal.unwrap_or(ModalState::None);
                            }
                            crate::actions::ActionResult::Error(msg) => {
                                state.set_error(msg);
                            }
                        }
                    } else {
                        // No parameters, show add dialog
                        let result = crate::actions::optimize::handle_optimize_action(
                            state,
                            OptimizeAction::AddParameter,
                            "",
                        );
                        match result {
                            crate::actions::ActionResult::Done(modal)
                            | crate::actions::ActionResult::Modified(modal) => {
                                state.modal = modal.unwrap_or(ModalState::None);
                            }
                            crate::actions::ActionResult::Error(msg) => {
                                state.set_error(msg);
                            }
                        }
                    }
                }
                OptimizePanel::Objective => {
                    // Show objective picker
                    let result = show_objective_picker(state);
                    match result {
                        crate::actions::ActionResult::Done(modal)
                        | crate::actions::ActionResult::Modified(modal) => {
                            state.modal = modal.unwrap_or(ModalState::None);
                        }
                        crate::actions::ActionResult::Error(msg) => {
                            state.set_error(msg);
                        }
                    }
                }
                OptimizePanel::Progress => {
                    // Run optimization
                    let result = crate::actions::optimize::handle_optimize_action(
                        state,
                        OptimizeAction::RunOptimization,
                        "",
                    );
                    match result {
                        crate::actions::ActionResult::Done(modal) => {
                            state.modal = modal.unwrap_or(ModalState::None);
                        }
                        crate::actions::ActionResult::Modified(modal) => {
                            state.mark_modified();
                            state.modal = modal.unwrap_or(ModalState::None);
                        }
                        crate::actions::ActionResult::Error(msg) => {
                            state.set_error(msg);
                        }
                    }
                }
                OptimizePanel::Results => {
                    // Nothing to configure in results
                }
            }
            return EventResult::Handled;
        }

        EventResult::NotHandled
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let panel = state.optimize_state.focused_panel;

        // Main layout: left (40%) and right (60%)
        let main_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // Parameters
                Constraint::Percentage(60), // Right side
            ])
            .split(area);

        // Right side: top, middle, bottom
        let right_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30), // Objective
                Constraint::Percentage(35), // Progress
                Constraint::Percentage(35), // Results
            ])
            .split(main_layout[1]);

        // Render all 4 panels
        self.render_parameters(
            frame,
            main_layout[0],
            state,
            panel == OptimizePanel::Parameters,
        );
        self.render_objective(
            frame,
            right_layout[0],
            state,
            panel == OptimizePanel::Objective,
        );
        self.render_progress(
            frame,
            right_layout[1],
            state,
            panel == OptimizePanel::Progress,
        );
        self.render_results(
            frame,
            right_layout[2],
            state,
            panel == OptimizePanel::Results,
        );
    }
}

impl Screen for OptimizeScreen {
    fn title(&self) -> &str {
        "Optimize"
    }
}

impl super::ModalHandler for OptimizeScreen {
    fn handles(&self, action: &ModalAction) -> bool {
        matches!(action, ModalAction::Optimize(_))
    }

    fn handle_modal_result(
        &self,
        state: &mut AppState,
        action: ModalAction,
        value: &ConfirmedValue,
    ) -> ActionResult {
        match action {
            ModalAction::Optimize(optimize_action) => {
                handle_optimize_action(state, optimize_action, value.as_str().unwrap_or_default())
            }
            // This shouldn't happen if handles() is correct
            _ => crate::actions::ActionResult::close(),
        }
    }
}
