use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, List, ListItem, Paragraph},
};

use super::Screen;
use crate::actions::ActionResult;
use crate::actions::analysis::handle_analysis_action;
use crate::components::{Component, EventResult};
use crate::data::keybindings_data::KeybindingsConfig;
use crate::modals::{AnalysisAction, ConfirmedValue, ModalAction, ModalState};
use crate::state::{AnalysisMetricType, AnalysisPanel, AnalysisResults, AppState};
use crate::util::format::format_currency;

pub struct AnalysisScreen;

impl AnalysisScreen {
    /// Render the Parameters panel (left 40%)
    fn render_parameters(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = if focused {
            " SWEEP PARAMETERS [a add, d del, Enter edit] "
        } else {
            " SWEEP PARAMETERS "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let params = &state.analysis_state.sweep_parameters;
        let selected_idx = state.analysis_state.selected_param_index;

        if params.is_empty() {
            let content = vec![
                Line::from(""),
                Line::from("No parameters selected for analysis."),
                Line::from(""),
                Line::from(Span::styled(
                    "Press 'a' to add a parameter.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Parameters you can sweep:",
                    Style::default().fg(Color::Cyan),
                )),
                Line::from("  - Event trigger ages"),
                Line::from("  - Effect amounts"),
                Line::from("  - Repeating event start/end"),
            ];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        } else {
            let items: Vec<ListItem> = params
                .iter()
                .enumerate()
                .map(|(idx, param)| {
                    let bounds = format!(
                        "[{:.0}-{:.0}, {} steps]",
                        param.min_value, param.max_value, param.step_count
                    );

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
                        Span::styled(&param.name, style),
                        Span::raw(" "),
                        Span::styled(bounds, Style::default().fg(Color::DarkGray)),
                    ]))
                })
                .collect();

            let list = List::new(items).block(block);
            frame.render_widget(list, area);
        }
    }

    /// Render the Metrics panel (right-top)
    fn render_metrics(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = if focused {
            " METRICS [m toggle] "
        } else {
            " METRICS "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let selected = &state.analysis_state.selected_metrics;

        // Available metrics
        let all_metrics = [
            AnalysisMetricType::SuccessRate,
            AnalysisMetricType::P50FinalNetWorth,
            AnalysisMetricType::P5FinalNetWorth,
            AnalysisMetricType::P95FinalNetWorth,
            AnalysisMetricType::LifetimeTaxes,
        ];

        let items: Vec<ListItem> = all_metrics
            .iter()
            .map(|metric| {
                let checked = if selected.contains(metric) {
                    "[x]"
                } else {
                    "[ ]"
                };
                let style = if selected.contains(metric) {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(checked, style),
                    Span::raw(" "),
                    Span::styled(metric.label(), style),
                ]))
            })
            .collect();

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }

    /// Render the Config panel (right-middle)
    fn render_config(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = if focused {
            " CONFIGURATION [s settings] "
        } else {
            " CONFIGURATION "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let mc_iter = state.analysis_state.mc_iterations;
        let default_steps = state.analysis_state.default_steps;
        let total_points = state.analysis_state.total_sweep_points();

        let content = vec![
            Line::from(""),
            Line::from(vec![
                Span::raw("  MC Iterations: "),
                Span::styled(format!("{}", mc_iter), Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::raw("  Default Steps: "),
                Span::styled(
                    format!("{}", default_steps),
                    Style::default().fg(Color::Green),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  Total Points: "),
                Span::styled(
                    format!("{}", total_points),
                    Style::default().fg(Color::Cyan),
                ),
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

    /// Render the Results panel (bottom) with 1D line chart or 2D heatmap
    fn render_results(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = if state.analysis_state.running {
            format!(
                " RESULTS [{}/{}] ",
                state.analysis_state.current_point, state.analysis_state.total_points
            )
        } else if focused {
            " RESULTS [r run analysis] ".to_string()
        } else {
            " RESULTS ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        if let Some(results) = &state.analysis_state.results {
            if results.is_1d() {
                // Render 1D line chart
                self.render_1d_chart(frame, area, state, block, results);
            } else {
                // Render 2D heatmap
                self.render_2d_heatmap(frame, area, state, block, results);
            }
        } else if state.analysis_state.running {
            let progress_pct = if state.analysis_state.total_points > 0 {
                (state.analysis_state.current_point as f64
                    / state.analysis_state.total_points as f64
                    * 100.0) as usize
            } else {
                0
            };

            let bar_width = 40;
            let filled = (progress_pct * bar_width / 100).min(bar_width);
            let empty = bar_width - filled;
            let progress_bar = format!(
                "[{}{}] {}%",
                "=".repeat(filled),
                " ".repeat(empty),
                progress_pct
            );

            let content = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Analysis in progress...",
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(progress_bar, Style::default().fg(Color::Green)),
                ]),
            ];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        } else {
            let content = vec![
                Line::from(""),
                Line::from("  No analysis results yet."),
                Line::from(""),
                Line::from(Span::styled(
                    "  1. Add parameters to sweep (a)",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  2. Select metrics to compute (m)",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  3. Run analysis (r)",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        }
    }

    /// Render a 1D line chart for sweep results using ratatui Chart widget
    fn render_1d_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        state: &AppState,
        block: Block,
        results: &AnalysisResults,
    ) {
        // Get the primary metric (success rate if available)
        let metric = if state
            .analysis_state
            .selected_metrics
            .contains(&AnalysisMetricType::SuccessRate)
        {
            AnalysisMetricType::SuccessRate
        } else {
            state
                .analysis_state
                .selected_metrics
                .iter()
                .next()
                .cloned()
                .unwrap_or(AnalysisMetricType::SuccessRate)
        };

        let values = results.get_1d_values(&metric);
        let param_values = &results.param1_values;

        if values.is_empty() || param_values.is_empty() {
            let content = vec![Line::from("  No data to display.")];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
            return;
        }

        // Convert to chart data points: (x, y) tuples
        // Note: Success rate is already stored as percentage (0-100) in AnalysisResults
        let data: Vec<(f64, f64)> = param_values
            .iter()
            .zip(values.iter())
            .map(|(&x, &y)| (x, y))
            .collect();

        // Calculate bounds with padding to ensure data is visible
        let x_min = param_values.first().copied().unwrap_or(0.0);
        let x_max = param_values.last().copied().unwrap_or(1.0);
        let x_padding = (x_max - x_min).abs() * 0.02;

        let (y_min, y_max) = if metric == AnalysisMetricType::SuccessRate {
            // Find actual min/max in data to scale appropriately
            let actual_min = data.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
            let actual_max = data
                .iter()
                .map(|(_, y)| *y)
                .fold(f64::NEG_INFINITY, f64::max);
            let range = (actual_max - actual_min).max(5.0); // At least 5% range
            let padding = range * 0.1;
            (
                (actual_min - padding).max(0.0),
                (actual_max + padding).min(105.0),
            )
        } else {
            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let padding = (max - min).abs().max(1.0) * 0.1;
            (min - padding, max + padding)
        };

        // Create dataset
        let dataset = Dataset::default()
            .name(metric.short_label())
            .marker(symbols::Marker::Dot)
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(Color::Green))
            .data(&data);

        // Create axis labels
        let x_labels = vec![
            Span::raw(format!("{:.0}", x_min)),
            Span::raw(format!("{:.0}", (x_min + x_max) / 2.0)),
            Span::raw(format!("{:.0}", x_max)),
        ];

        let y_labels = if metric == AnalysisMetricType::SuccessRate {
            vec![
                Span::raw(format!("{:.0}%", y_min)),
                Span::raw(format!("{:.0}%", (y_min + y_max) / 2.0)),
                Span::raw(format!("{:.0}%", y_max)),
            ]
        } else {
            vec![
                Span::raw(format_currency(y_min)),
                Span::raw(format_currency((y_min + y_max) / 2.0)),
                Span::raw(format_currency(y_max)),
            ]
        };

        let x_axis = Axis::default()
            .title(results.param1_label.clone().dark_gray())
            .bounds([x_min - x_padding, x_max + x_padding])
            .labels(x_labels);

        let y_axis = Axis::default()
            .title(metric.short_label().dark_gray())
            .bounds([y_min, y_max])
            .labels(y_labels);

        let chart = Chart::new(vec![dataset])
            .block(block)
            .x_axis(x_axis)
            .y_axis(y_axis);

        frame.render_widget(chart, area);
    }

    /// Render a 2D heatmap for sweep results
    fn render_2d_heatmap(
        &self,
        frame: &mut Frame,
        area: Rect,
        state: &AppState,
        block: Block,
        results: &AnalysisResults,
    ) {
        let inner = block.inner(area);

        // Get the primary metric
        let metric = if state
            .analysis_state
            .selected_metrics
            .contains(&AnalysisMetricType::SuccessRate)
        {
            AnalysisMetricType::SuccessRate
        } else {
            state
                .analysis_state
                .selected_metrics
                .iter()
                .next()
                .cloned()
                .unwrap_or(AnalysisMetricType::SuccessRate)
        };

        let Some(matrix) = results.metric_results.get(&metric) else {
            let content = vec![Line::from("  No data to display.")];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
            return;
        };

        if matrix.is_empty() || matrix[0].is_empty() {
            let content = vec![Line::from("  No data to display.")];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();

        // Title
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                metric.short_label(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": "),
            Span::styled(&results.param1_label, Style::default().fg(Color::Magenta)),
            Span::raw(" x "),
            Span::styled(&results.param2_label, Style::default().fg(Color::Magenta)),
        ]));
        lines.push(Line::from(""));

        // Find min/max for color scaling
        let (min_val, max_val) = if metric == AnalysisMetricType::SuccessRate {
            (0.0, 1.0)
        } else {
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            for row in matrix {
                for &val in row {
                    min = min.min(val);
                    max = max.max(val);
                }
            }
            (min, max)
        };
        let range = (max_val - min_val).max(0.0001);

        // Heatmap characters
        let heat_chars = [' ', '.', ':', '+', '*', '#', '@'];

        // Render rows (param1 on Y-axis, param2 on X-axis)
        let max_rows = inner.height.saturating_sub(6) as usize;
        for (i, row) in matrix.iter().enumerate().take(max_rows) {
            let y_val = results
                .param1_values
                .get(i)
                .map(|v| format!("{:>6.0}", v))
                .unwrap_or_default();

            let mut spans = vec![
                Span::styled(y_val, Style::default().fg(Color::DarkGray)),
                Span::raw(" |"),
            ];

            for &val in row.iter().take(inner.width.saturating_sub(10) as usize) {
                let normalized = ((val - min_val) / range).clamp(0.0, 1.0);
                let char_idx = (normalized * (heat_chars.len() - 1) as f64).round() as usize;
                let ch = heat_chars[char_idx.min(heat_chars.len() - 1)];

                let color = if normalized < 0.33 {
                    Color::Red
                } else if normalized < 0.66 {
                    Color::Yellow
                } else {
                    Color::Green
                };

                spans.push(Span::styled(format!(" {}", ch), Style::default().fg(color)));
            }

            lines.push(Line::from(spans));
        }

        // X-axis label
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("       "),
            Span::styled(
                format!("{} ->", results.param2_label),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        // Legend
        lines.push(Line::from(vec![
            Span::raw("  Legend: "),
            Span::styled("Low", Style::default().fg(Color::Red)),
            Span::raw(" -> "),
            Span::styled("Mid", Style::default().fg(Color::Yellow)),
            Span::raw(" -> "),
            Span::styled("High", Style::default().fg(Color::Green)),
        ]));

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, area);
    }
}

impl Component for AnalysisScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let panel = state.analysis_state.focused_panel;
        let kb = &state.keybindings;

        // Don't handle keys if analysis is running
        if state.analysis_state.running {
            return EventResult::NotHandled;
        }

        // Panel navigation
        if KeybindingsConfig::matches(&key, &kb.navigation.next_panel) {
            state.analysis_state.focused_panel = panel.next();
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.navigation.prev_panel) {
            state.analysis_state.focused_panel = panel.prev();
            return EventResult::Handled;
        }

        // r: Run analysis (from any panel)
        if KeybindingsConfig::matches(&key, &kb.tabs.optimize.run) {
            let result = handle_analysis_action(state, AnalysisAction::RunAnalysis, "");
            match result {
                ActionResult::Done(modal) => {
                    state.modal = modal.unwrap_or(ModalState::None);
                }
                ActionResult::Modified(modal) => {
                    state.mark_modified();
                    state.modal = modal.unwrap_or(ModalState::None);
                }
                ActionResult::Error(msg) => {
                    state.set_error(msg);
                }
            }
            return EventResult::Handled;
        }

        // s: Settings (from any panel)
        if KeybindingsConfig::matches(&key, &kb.tabs.optimize.settings) {
            let result = handle_analysis_action(state, AnalysisAction::ConfigureSettings, "");
            match result {
                ActionResult::Done(modal) | ActionResult::Modified(modal) => {
                    state.modal = modal.unwrap_or(ModalState::None);
                }
                ActionResult::Error(msg) => {
                    state.set_error(msg);
                }
            }
            return EventResult::Handled;
        }

        // Parameter list navigation (j/k or Up/Down in Parameters panel)
        if KeybindingsConfig::matches(&key, &kb.navigation.down) {
            if panel == AnalysisPanel::Parameters {
                let param_count = state.analysis_state.sweep_parameters.len();
                if param_count > 0 {
                    state.analysis_state.selected_param_index =
                        (state.analysis_state.selected_param_index + 1) % param_count;
                }
            }
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.navigation.up) {
            if panel == AnalysisPanel::Parameters {
                let param_count = state.analysis_state.sweep_parameters.len();
                if param_count > 0 {
                    if state.analysis_state.selected_param_index == 0 {
                        state.analysis_state.selected_param_index = param_count - 1;
                    } else {
                        state.analysis_state.selected_param_index -= 1;
                    }
                }
            }
            return EventResult::Handled;
        }

        // a: Add parameter
        if KeybindingsConfig::matches(&key, &kb.tabs.optimize.add_param) {
            if panel == AnalysisPanel::Parameters {
                let result = handle_analysis_action(state, AnalysisAction::AddParameter, "");
                match result {
                    ActionResult::Done(modal) | ActionResult::Modified(modal) => {
                        state.modal = modal.unwrap_or(ModalState::None);
                    }
                    ActionResult::Error(msg) => {
                        state.set_error(msg);
                    }
                }
            }
            return EventResult::Handled;
        }

        // d: Delete selected parameter
        if KeybindingsConfig::matches(&key, &kb.tabs.optimize.delete_param) {
            if panel == AnalysisPanel::Parameters {
                let params = &mut state.analysis_state.sweep_parameters;
                let idx = state.analysis_state.selected_param_index;
                if idx < params.len() {
                    params.remove(idx);
                    // Adjust selection index if needed
                    if state.analysis_state.selected_param_index >= params.len()
                        && !params.is_empty()
                    {
                        state.analysis_state.selected_param_index = params.len() - 1;
                    }
                }
            }
            return EventResult::Handled;
        }

        // m: Toggle metrics (in Metrics panel)
        if KeybindingsConfig::matches(&key, &[String::from("m")]) {
            if panel == AnalysisPanel::Metrics {
                let result = handle_analysis_action(state, AnalysisAction::ToggleMetric, "");
                match result {
                    ActionResult::Done(modal) | ActionResult::Modified(modal) => {
                        state.modal = modal.unwrap_or(ModalState::None);
                    }
                    ActionResult::Error(msg) => {
                        state.set_error(msg);
                    }
                }
            }
            return EventResult::Handled;
        }

        // Enter: Configure based on panel
        if KeybindingsConfig::matches(&key, &kb.navigation.confirm) {
            match panel {
                AnalysisPanel::Parameters => {
                    // Edit selected parameter (if any)
                    if !state.analysis_state.sweep_parameters.is_empty() {
                        let idx = state.analysis_state.selected_param_index;
                        let result = handle_analysis_action(
                            state,
                            AnalysisAction::ConfigureParameter { index: idx },
                            "",
                        );
                        match result {
                            ActionResult::Done(modal) | ActionResult::Modified(modal) => {
                                state.modal = modal.unwrap_or(ModalState::None);
                            }
                            ActionResult::Error(msg) => {
                                state.set_error(msg);
                            }
                        }
                    } else {
                        // No parameters, show add dialog
                        let result =
                            handle_analysis_action(state, AnalysisAction::AddParameter, "");
                        match result {
                            ActionResult::Done(modal) | ActionResult::Modified(modal) => {
                                state.modal = modal.unwrap_or(ModalState::None);
                            }
                            ActionResult::Error(msg) => {
                                state.set_error(msg);
                            }
                        }
                    }
                }
                AnalysisPanel::Metrics => {
                    // Toggle metric
                    let result = handle_analysis_action(state, AnalysisAction::ToggleMetric, "");
                    match result {
                        ActionResult::Done(modal) | ActionResult::Modified(modal) => {
                            state.modal = modal.unwrap_or(ModalState::None);
                        }
                        ActionResult::Error(msg) => {
                            state.set_error(msg);
                        }
                    }
                }
                AnalysisPanel::Config => {
                    // Show settings
                    let result =
                        handle_analysis_action(state, AnalysisAction::ConfigureSettings, "");
                    match result {
                        ActionResult::Done(modal) | ActionResult::Modified(modal) => {
                            state.modal = modal.unwrap_or(ModalState::None);
                        }
                        ActionResult::Error(msg) => {
                            state.set_error(msg);
                        }
                    }
                }
                AnalysisPanel::Results => {
                    // Run analysis
                    let result = handle_analysis_action(state, AnalysisAction::RunAnalysis, "");
                    match result {
                        ActionResult::Done(modal) => {
                            state.modal = modal.unwrap_or(ModalState::None);
                        }
                        ActionResult::Modified(modal) => {
                            state.mark_modified();
                            state.modal = modal.unwrap_or(ModalState::None);
                        }
                        ActionResult::Error(msg) => {
                            state.set_error(msg);
                        }
                    }
                }
            }
            return EventResult::Handled;
        }

        EventResult::NotHandled
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let panel = state.analysis_state.focused_panel;

        // Main layout: top section and bottom results
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),        // Top section (Parameters | Metrics + Config)
                Constraint::Percentage(50), // Results
            ])
            .split(area);

        // Top section: left (Parameters) and right (Metrics + Config)
        let top_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // Parameters (full height of top)
                Constraint::Percentage(60), // Metrics + Config stacked
            ])
            .split(main_layout[0]);

        // Right side of top: Metrics (top) and Config (bottom)
        let right_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(60), // Metrics
                Constraint::Percentage(40), // Config
            ])
            .split(top_layout[1]);

        // Render all 4 panels
        self.render_parameters(
            frame,
            top_layout[0],
            state,
            panel == AnalysisPanel::Parameters,
        );
        self.render_metrics(
            frame,
            right_layout[0],
            state,
            panel == AnalysisPanel::Metrics,
        );
        self.render_config(
            frame,
            right_layout[1],
            state,
            panel == AnalysisPanel::Config,
        );
        self.render_results(
            frame,
            main_layout[1],
            state,
            panel == AnalysisPanel::Results,
        );
    }
}

impl Screen for AnalysisScreen {
    fn title(&self) -> &str {
        "Analysis"
    }
}

impl super::ModalHandler for AnalysisScreen {
    fn handles(&self, action: &ModalAction) -> bool {
        matches!(action, ModalAction::Analysis(_))
    }

    fn handle_modal_result(
        &self,
        state: &mut AppState,
        action: ModalAction,
        value: &ConfirmedValue,
    ) -> ActionResult {
        match action {
            ModalAction::Analysis(analysis_action) => {
                handle_analysis_action(state, analysis_action, value.as_str().unwrap_or_default())
            }
            // This shouldn't happen if handles() is correct
            _ => ActionResult::close(),
        }
    }
}
