use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, List, ListItem, Paragraph},
};

use super::Screen;
use crate::components::{Component, EventResult};
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
            " PARAMETERS [j/k nav, a add, d del, Enter config] "
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

                    let bounds = format!("[{:.0} - {:.0}]", param.min_value, param.max_value);

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
            " OBJECTIVE [Enter config] "
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
                Span::raw("  Max Optimization Iterations: "),
                Span::styled(format!("{}", max_iter), Style::default().fg(Color::Green)),
            ]),
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
                Span::raw("  Max Optimization Iterations: "),
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
            " PROGRESS [r run] ".to_string()
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
                ]
            } else {
                vec![
                    Line::from(""),
                    Line::from("  No optimization data yet."),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  Press 'r' to run optimization.",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            };
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        } else {
            // Render convergence chart
            let inner = block.inner(area);
            frame.render_widget(block, area);

            // Find max value for scaling
            let max_value = convergence_data
                .iter()
                .map(|(_, v)| *v)
                .fold(f64::NEG_INFINITY, f64::max);

            // Create bars for convergence history
            let bars: Vec<Bar> = convergence_data
                .iter()
                .map(|(iter, value)| {
                    let scaled = if max_value > 0.0 {
                        ((value / max_value) * 100.0) as u64
                    } else {
                        0
                    };

                    Bar::default()
                        .value(scaled)
                        .label(Line::from(format!("{}", iter)))
                        .style(Style::default().fg(Color::Cyan))
                })
                .collect();

            if !bars.is_empty() {
                let chart = BarChart::default()
                    .data(BarGroup::default().bars(&bars))
                    .bar_width(3)
                    .bar_gap(1)
                    .direction(Direction::Vertical);

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

        let title = if focused { " RESULTS " } else { " RESULTS " };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        if let Some(result) = &state.optimize_state.result {
            let mut content = vec![
                Line::from(""),
                Line::from(vec![
                    Span::raw("  Status: "),
                    if result.converged {
                        Span::styled("Converged", Style::default().fg(Color::Green))
                    } else {
                        Span::styled("Not Converged", Style::default().fg(Color::Yellow))
                    },
                ]),
                Line::from(vec![
                    Span::raw("  Iterations: "),
                    Span::styled(
                        format!("{}", result.iterations),
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "  Optimal Values:",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
            ];

            for (name, value) in &result.optimal_values {
                content.push(Line::from(vec![
                    Span::raw("    "),
                    Span::raw(name),
                    Span::raw(": "),
                    Span::styled(format!("{:.2}", value), Style::default().fg(Color::Green)),
                ]));
            }

            content.push(Line::from(""));
            content.push(Line::from(vec![
                Span::raw("  Objective Value: "),
                Span::styled(
                    format_currency(result.objective_value),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            content.push(Line::from(vec![
                Span::raw("  Success Rate: "),
                Span::styled(
                    format!("{:.1}%", result.success_rate * 100.0),
                    Style::default().fg(Color::Magenta),
                ),
            ]));

            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        } else {
            let content = vec![
                Line::from(""),
                Line::from("  No optimization results yet."),
                Line::from(""),
                Line::from(Span::styled(
                    "  Configure parameters and run optimization.",
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

            // r: Run optimization
            KeyCode::Char('r') => {
                state.set_error("Optimization not yet connected".to_string());
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
                    state.set_error("Press Enter to configure".to_string());
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

            // Enter: Configure (show message for now)
            KeyCode::Enter => {
                state.set_error("Press Enter to configure".to_string());
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
                Constraint::Percentage(33), // Objective
                Constraint::Percentage(34), // Progress
                Constraint::Percentage(33), // Results
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
