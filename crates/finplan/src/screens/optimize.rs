use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, List, ListItem, Paragraph},
};

use super::Screen;
use crate::actions::optimize::{show_objective_picker, show_settings_form};
use crate::components::{Component, EventResult};
use crate::state::{AppState, ModalState, OptimizeAction, OptimizePanel};
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

    /// Render the Progress panel (right-middle) with convergence chart
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
            // Render convergence line chart
            let inner = block.inner(area);
            frame.render_widget(block, area);

            // Prepare data for line chart
            let chart_data: Vec<(f64, f64)> = convergence_data
                .iter()
                .map(|(iter, value)| (*iter as f64, *value))
                .collect();

            if !chart_data.is_empty() {
                let max_iter = convergence_data.iter().map(|(i, _)| *i).max().unwrap_or(1) as f64;
                let max_value = convergence_data
                    .iter()
                    .map(|(_, v)| *v)
                    .fold(f64::NEG_INFINITY, f64::max);
                let min_value = convergence_data
                    .iter()
                    .map(|(_, v)| *v)
                    .fold(f64::INFINITY, f64::min);

                let y_range = if (max_value - min_value).abs() < 1.0 {
                    [min_value - 1000.0, max_value + 1000.0]
                } else {
                    [min_value * 0.95, max_value * 1.05]
                };

                let dataset = Dataset::default()
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Cyan))
                    .data(&chart_data);

                let chart = Chart::new(vec![dataset])
                    .x_axis(
                        Axis::default()
                            .title("Iteration")
                            .style(Style::default().fg(Color::DarkGray))
                            .bounds([1.0, max_iter]),
                    )
                    .y_axis(
                        Axis::default()
                            .title("Objective")
                            .style(Style::default().fg(Color::DarkGray))
                            .bounds(y_range),
                    );

                frame.render_widget(chart, inner);
            }
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

        // Don't handle keys if optimization is running
        if state.optimize_state.running {
            return EventResult::NotHandled;
        }

        match key.code {
            // Panel navigation
            KeyCode::Tab => {
                state.optimize_state.focused_panel = panel.next();
                EventResult::Handled
            }
            KeyCode::BackTab => {
                state.optimize_state.focused_panel = panel.prev();
                EventResult::Handled
            }

            // r: Run optimization (from any panel)
            KeyCode::Char('r') => {
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
                EventResult::Handled
            }

            // s: Settings (from any panel)
            KeyCode::Char('s') => {
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
                EventResult::Handled
            }

            // Parameter list navigation (j/k or Up/Down in Parameters panel)
            KeyCode::Char('j') | KeyCode::Down => {
                if panel == OptimizePanel::Parameters {
                    let param_count = state.optimize_state.selected_parameters.len();
                    if param_count > 0 {
                        state.optimize_state.selected_param_index =
                            (state.optimize_state.selected_param_index + 1) % param_count;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
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
                EventResult::Handled
            }

            // a: Add parameter
            KeyCode::Char('a') => {
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
                EventResult::Handled
            }

            // d: Delete selected parameter
            KeyCode::Char('d') => {
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
                EventResult::Handled
            }

            // Enter: Configure based on panel
            KeyCode::Enter => {
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
                EventResult::Handled
            }

            _ => EventResult::NotHandled,
        }
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
    fn handles(&self, action: &crate::state::ModalAction) -> bool {
        matches!(action, crate::state::ModalAction::Optimize(_))
    }

    fn handle_modal_result(
        &self,
        state: &mut AppState,
        action: crate::state::ModalAction,
        _value: &crate::modals::ConfirmedValue,
        legacy_value: &str,
    ) -> crate::actions::ActionResult {
        use crate::actions::optimize::handle_optimize_action;

        match action {
            crate::state::ModalAction::Optimize(optimize_action) => {
                handle_optimize_action(state, optimize_action, legacy_value)
            }
            // This shouldn't happen if handles() is correct
            _ => crate::actions::ActionResult::close(),
        }
    }
}
