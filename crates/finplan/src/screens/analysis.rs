use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Chart, Dataset, GraphType, LegendPosition, List, ListItem, Paragraph,
    },
};

use super::Screen;
use crate::actions::analysis::handle_analysis_action;
use crate::components::{Component, EventResult};
use crate::data::keybindings_data::KeybindingsConfig;
use crate::modals::{AnalysisAction, ConfirmedValue, ModalAction, ModalState};
use crate::state::{AnalysisPanel, AnalysisResults, AppState};
use crate::util::format::format_currency;
use crate::{actions::ActionResult, data::analysis_data::AnalysisMetricData};

/// Minimum width for a single chart in the results panel
const MIN_CHART_WIDTH: u16 = 60;
/// Maximum width for a single chart in the results panel
const MAX_CHART_WIDTH: u16 = 80;

/// Available metrics for selection
const AVAILABLE_METRICS: &[AnalysisMetricData] = &[
    AnalysisMetricData::SuccessRate,
    AnalysisMetricData::P50FinalNetWorth,
    AnalysisMetricData::P5FinalNetWorth,
    AnalysisMetricData::P95FinalNetWorth,
    AnalysisMetricData::LifetimeTaxes,
    AnalysisMetricData::MaxDrawdown,
];

/// Colors for different metrics
const METRIC_COLORS: &[(AnalysisMetricData, Color)] = &[
    (AnalysisMetricData::SuccessRate, Color::Green),
    (AnalysisMetricData::P50FinalNetWorth, Color::Cyan),
    (AnalysisMetricData::P5FinalNetWorth, Color::Blue),
    (AnalysisMetricData::P95FinalNetWorth, Color::Magenta),
    (AnalysisMetricData::LifetimeTaxes, Color::Yellow),
    (AnalysisMetricData::MaxDrawdown, Color::Red),
];

fn metric_color(metric: &AnalysisMetricData) -> Color {
    METRIC_COLORS
        .iter()
        .find(|(m, _)| m == metric)
        .map(|(_, c)| *c)
        .unwrap_or(Color::Green)
}

/// Handle chart configuration - show modal to configure chart at the selected index
fn handle_chart_configure(state: &mut AppState) {
    let selected_idx = state.analysis_state.selected_chart_index;

    // Show the chart configuration modal
    let result = handle_analysis_action(
        state,
        AnalysisAction::ConfigureChart {
            index: selected_idx,
        },
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
}

/// Handle adding a new chart
fn handle_add_chart(state: &mut AppState) {
    use crate::data::analysis_data::ChartConfigData;

    let results = match &state.analysis_state.results {
        Some(r) => r,
        None => return,
    };

    let ndim = results.ndim();
    let num_charts = state.analysis_state.chart_configs.len();

    // Limit to 4 charts
    if num_charts >= 4 {
        state.set_error("Maximum of 4 charts. Delete one to add another.".to_string());
        return;
    }

    // Cycle through metrics for variety
    let metric = AVAILABLE_METRICS
        .get(num_charts % AVAILABLE_METRICS.len())
        .copied()
        .unwrap_or(AnalysisMetricData::SuccessRate);

    // Default to 1D if only 1 param, otherwise alternate
    let chart = if ndim == 1 || num_charts.is_multiple_of(2) {
        ChartConfigData::new_1d(0, metric)
    } else {
        ChartConfigData::new_2d(0, 1.min(ndim - 1), metric)
    };

    state.analysis_state.chart_configs.push(chart);
    state.analysis_state.selected_chart_index = state.analysis_state.chart_configs.len() - 1;
    state.mark_modified();
}

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
                        Span::styled(&param.event_name, style),
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
            " METRICS [t toggle, j/k nav] "
        } else {
            " METRICS "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let selected = &state.analysis_state.selected_metrics;
        let selected_idx = state.analysis_state.selected_metric_index;

        let items: Vec<ListItem> = AVAILABLE_METRICS
            .iter()
            .enumerate()
            .map(|(idx, metric)| {
                let is_cursor = focused && idx == selected_idx;
                let is_enabled = selected.contains(metric);

                let checked = if is_enabled { "[x]" } else { "[ ]" };

                let style = if is_cursor {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if is_enabled {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let prefix = if is_cursor { "> " } else { "  " };

                ListItem::new(Line::from(vec![
                    Span::styled(prefix, style),
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

    /// Render the Results panel (bottom) with 1D line chart(s) or 2D heatmap
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
            // Check if we have chart configs
            if state.analysis_state.chart_configs.is_empty() {
                // Show [CONFIGURE] prompt
                self.render_configure_prompt(frame, area, state, block, focused);
            } else {
                // Render configured charts
                self.render_configured_charts(frame, area, state, block, results);
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

    /// Render empty chart slots when results are available but no charts configured
    fn render_configure_prompt(
        &self,
        frame: &mut Frame,
        area: Rect,
        state: &AppState,
        block: Block,
        focused: bool,
    ) {
        frame.render_widget(block.clone(), area);
        let inner = block.inner(area);

        // Calculate how many empty slots fit (same logic as configured charts)
        let num_slots = ((inner.width as usize) / MIN_CHART_WIDTH as usize).clamp(1, 4);
        let chart_width = (inner.width / num_slots as u16).clamp(MIN_CHART_WIDTH, MAX_CHART_WIDTH);

        let mut constraints: Vec<Constraint> = (0..num_slots)
            .map(|_| Constraint::Length(chart_width))
            .collect();
        constraints.push(Constraint::Min(0));

        let slots = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(inner);

        let selected_idx = state.analysis_state.selected_chart_index;

        // Render empty chart slots
        for i in 0..num_slots {
            let is_selected = focused && i == selected_idx;
            self.render_empty_chart_slot(frame, slots[i], i, is_selected);
        }
    }

    /// Render an empty chart slot with [CONFIGURE] prompt
    fn render_empty_chart_slot(
        &self,
        frame: &mut Frame,
        area: Rect,
        index: usize,
        is_selected: bool,
    ) {
        let border_color = if is_selected {
            Color::Yellow
        } else {
            Color::DarkGray
        };

        let chart_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                format!(" Chart {} ", index + 1),
                Style::default().fg(if is_selected {
                    Color::Yellow
                } else {
                    Color::DarkGray
                }),
            ));

        let inner = chart_block.inner(area);
        frame.render_widget(chart_block, area);

        // Center the CONFIGURE text vertically
        let v_padding = inner.height.saturating_sub(3) / 2;

        let content = vec![
            Line::from(""),
            Line::from(Span::styled(
                if is_selected {
                    "[ CONFIGURE ]"
                } else {
                    "  CONFIGURE  "
                },
                Style::default()
                    .fg(if is_selected {
                        Color::Yellow
                    } else {
                        Color::Cyan
                    })
                    .add_modifier(if is_selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "c or Enter",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        // Add vertical padding
        let mut padded_content = vec![Line::from(""); v_padding as usize];
        padded_content.extend(content);

        let paragraph =
            Paragraph::new(padded_content).alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, inner);
    }

    /// Render charts based on chart_configs, showing empty slots for unconfigured positions
    fn render_configured_charts(
        &self,
        frame: &mut Frame,
        area: Rect,
        state: &AppState,
        block: Block,
        results: &AnalysisResults,
    ) {
        let charts = &state.analysis_state.chart_configs;
        let selected_idx = state.analysis_state.selected_chart_index;
        let focused = state.analysis_state.focused_panel == AnalysisPanel::Results;

        // Render the outer block
        frame.render_widget(block.clone(), area);
        let inner = block.inner(area);

        // Calculate how many slots fit (always show up to 4, based on available width)
        let num_slots = ((inner.width as usize) / MIN_CHART_WIDTH as usize).clamp(1, 4);
        let chart_width = (inner.width / num_slots as u16).clamp(MIN_CHART_WIDTH, MAX_CHART_WIDTH);

        let mut constraints: Vec<Constraint> = (0..num_slots)
            .map(|_| Constraint::Length(chart_width))
            .collect();
        constraints.push(Constraint::Min(0)); // Fill remaining space

        let slots = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(inner);

        // Render each slot: either a configured chart or an empty [CONFIGURE] slot
        for i in 0..num_slots {
            let is_selected = focused && i == selected_idx;
            if let Some(chart_config) = charts.get(i) {
                self.render_chart_from_config(frame, slots[i], results, chart_config, is_selected);
            } else {
                self.render_empty_chart_slot(frame, slots[i], i, is_selected);
            }
        }
    }

    /// Render a single chart based on its configuration
    fn render_chart_from_config(
        &self,
        frame: &mut Frame,
        area: Rect,
        results: &AnalysisResults,
        config: &crate::data::analysis_data::ChartConfigData,
        is_selected: bool,
    ) {
        use crate::data::analysis_data::ChartType;

        match config.chart_type {
            ChartType::Scatter1D => {
                self.render_single_1d_chart(frame, area, results, &config.metric, is_selected);
            }
            ChartType::Heatmap2D => {
                self.render_mini_2d_heatmap(frame, area, results, config, is_selected);
            }
        }
    }

    /// Render a mini 2D heatmap in a chart slot
    fn render_mini_2d_heatmap(
        &self,
        frame: &mut Frame,
        area: Rect,
        results: &AnalysisResults,
        config: &crate::data::analysis_data::ChartConfigData,
        is_selected: bool,
    ) {
        let border_color = if is_selected {
            Color::Yellow
        } else {
            Color::DarkGray
        };

        let metric_color = metric_color(&config.metric);
        let chart_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                format!(" {} ", config.metric.short_label()),
                Style::default().fg(metric_color),
            ));

        let inner = chart_block.inner(area);
        frame.render_widget(chart_block, area);

        // Get 2D data
        let Some((matrix, min_val, max_val)) = results.get_2d_metric_matrix(&config.metric) else {
            let placeholder = Paragraph::new("No data");
            frame.render_widget(placeholder, inner);
            return;
        };

        if matrix.is_empty() {
            return;
        }

        // Render a simplified heatmap
        let heat_chars = [' ', '.', ':', '+', '*', '#', '@'];
        let (scale_min, scale_max) = if config.metric == AnalysisMetricData::SuccessRate {
            (0.0, 100.0)
        } else {
            (min_val, max_val)
        };
        let range = (scale_max - scale_min).max(0.0001);

        let max_rows = inner.height as usize;
        let max_cols = inner.width as usize;

        let mut lines: Vec<Line> = Vec::new();
        for row in matrix.iter().take(max_rows) {
            let mut spans: Vec<Span> = Vec::new();
            for &val in row.iter().take(max_cols) {
                let normalized = ((val - scale_min) / range).clamp(0.0, 1.0);
                let char_idx = (normalized * (heat_chars.len() - 1) as f64).round() as usize;
                let ch = heat_chars[char_idx.min(heat_chars.len() - 1)];

                let color = if normalized < 0.33 {
                    Color::Red
                } else if normalized < 0.66 {
                    Color::Yellow
                } else {
                    Color::Green
                };

                spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
            }
            lines.push(Line::from(spans));
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    /// Render a single 1D chart for a specific metric
    fn render_single_1d_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        results: &AnalysisResults,
        metric: &AnalysisMetricData,
        is_selected: bool,
    ) {
        let (param_values, values) = results.get_1d_metric_data(metric);

        if values.is_empty() || param_values.is_empty() {
            return;
        }

        // Convert to chart data points: (x, y) tuples
        let data: Vec<(f64, f64)> = param_values
            .iter()
            .zip(values.iter())
            .map(|(&x, &y)| (x, y))
            .collect();

        // Calculate bounds with padding
        let x_min = param_values.first().copied().unwrap_or(0.0);
        let x_max = param_values.last().copied().unwrap_or(1.0);
        let x_padding = (x_max - x_min).abs() * 0.02;

        let (y_min, y_max) = if *metric == AnalysisMetricData::SuccessRate {
            let actual_min = data.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
            let actual_max = data
                .iter()
                .map(|(_, y)| *y)
                .fold(f64::NEG_INFINITY, f64::max);
            let range = (actual_max - actual_min).max(5.0);
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

        // Create dataset with metric-specific color
        let color = metric_color(metric);
        let dataset = Dataset::default()
            .name(metric.short_label())
            .marker(symbols::Marker::Dot)
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(color))
            .data(&data);

        // Create axis labels
        let x_labels = vec![
            Span::raw(format!("{:.0}", x_min)),
            Span::raw(format!("{:.0}", (x_min + x_max) / 2.0)),
            Span::raw(format!("{:.0}", x_max)),
        ];

        let y_labels = if *metric == AnalysisMetricData::SuccessRate {
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
            .title(results.param1_label().to_string().dark_gray())
            .bounds([x_min - x_padding, x_max + x_padding])
            .labels(x_labels);

        let y_axis = Axis::default()
            .title(metric.short_label().dark_gray())
            .bounds([y_min, y_max])
            .labels(y_labels);

        // Create chart with a bordered block showing the metric name
        let border_color = if is_selected {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        let chart_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                format!(" {} ", metric.short_label()),
                Style::default().fg(color),
            ));

        let chart = Chart::new(vec![dataset])
            .block(chart_block)
            .x_axis(x_axis)
            .y_axis(y_axis)
            .legend_position(Some(LegendPosition::BottomRight));

        frame.render_widget(chart, area);
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
        if KeybindingsConfig::matches(&key, &kb.tabs.analyze.run) {
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
        if KeybindingsConfig::matches(&key, &kb.tabs.analyze.settings) {
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

        // List navigation (j/k or Up/Down in Parameters or Metrics panel)
        if KeybindingsConfig::matches(&key, &kb.navigation.down) {
            match panel {
                AnalysisPanel::Parameters => {
                    let param_count = state.analysis_state.sweep_parameters.len();
                    if param_count > 0 {
                        state.analysis_state.selected_param_index =
                            (state.analysis_state.selected_param_index + 1) % param_count;
                    }
                }
                AnalysisPanel::Metrics => {
                    let metric_count = AVAILABLE_METRICS.len();
                    state.analysis_state.selected_metric_index =
                        (state.analysis_state.selected_metric_index + 1) % metric_count;
                }
                _ => {}
            }
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.navigation.up) {
            match panel {
                AnalysisPanel::Parameters => {
                    let param_count = state.analysis_state.sweep_parameters.len();
                    if param_count > 0 {
                        if state.analysis_state.selected_param_index == 0 {
                            state.analysis_state.selected_param_index = param_count - 1;
                        } else {
                            state.analysis_state.selected_param_index -= 1;
                        }
                    }
                }
                AnalysisPanel::Metrics => {
                    let metric_count = AVAILABLE_METRICS.len();
                    if state.analysis_state.selected_metric_index == 0 {
                        state.analysis_state.selected_metric_index = metric_count - 1;
                    } else {
                        state.analysis_state.selected_metric_index -= 1;
                    }
                }
                _ => {}
            }
            return EventResult::Handled;
        }

        // a: Add parameter
        if KeybindingsConfig::matches(&key, &kb.tabs.analyze.add_param) {
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
        if KeybindingsConfig::matches(&key, &kb.tabs.analyze.delete_param) {
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

        // t: Toggle currently selected metric (in Metrics panel)
        if KeybindingsConfig::matches(&key, &kb.tabs.analyze.toggle_metric) {
            if panel == AnalysisPanel::Metrics {
                let idx = state.analysis_state.selected_metric_index;
                if let Some(metric) = AVAILABLE_METRICS.get(idx) {
                    if state.analysis_state.selected_metrics.contains(metric) {
                        state.analysis_state.selected_metrics.remove(metric);
                    } else {
                        state.analysis_state.selected_metrics.insert(*metric);
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
                    // Toggle currently selected metric
                    let idx = state.analysis_state.selected_metric_index;
                    if let Some(metric) = AVAILABLE_METRICS.get(idx) {
                        if state.analysis_state.selected_metrics.contains(metric) {
                            state.analysis_state.selected_metrics.remove(metric);
                        } else {
                            state.analysis_state.selected_metrics.insert(*metric);
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
                    // If results exist, configure charts; otherwise run analysis
                    if state.analysis_state.results.is_some() {
                        // Configure or add chart
                        handle_chart_configure(state);
                    } else {
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
            }
            return EventResult::Handled;
        }

        // c: Configure chart (in Results panel with results)
        if KeybindingsConfig::matches(&key, &kb.tabs.analyze.configure_chart)
            && panel == AnalysisPanel::Results
            && state.analysis_state.results.is_some()
        {
            handle_chart_configure(state);
            return EventResult::Handled;
        }

        // h/l: Navigate between chart slots (in Results panel with results)
        // Navigate between all slots (configured or empty)
        if KeybindingsConfig::matches(&key, &kb.navigation.left)
            && panel == AnalysisPanel::Results
            && state.analysis_state.results.is_some()
        {
            // Always navigate across all 4 slots (max possible)
            const MAX_SLOTS: usize = 4;

            if state.analysis_state.selected_chart_index == 0 {
                state.analysis_state.selected_chart_index = MAX_SLOTS - 1;
            } else {
                state.analysis_state.selected_chart_index -= 1;
            }
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.navigation.right)
            && panel == AnalysisPanel::Results
            && state.analysis_state.results.is_some()
        {
            // Always navigate across all 4 slots (max possible)
            const MAX_SLOTS: usize = 4;

            state.analysis_state.selected_chart_index =
                (state.analysis_state.selected_chart_index + 1) % MAX_SLOTS;
            return EventResult::Handled;
        }

        // +: Add chart
        if KeybindingsConfig::matches(&key, &kb.tabs.analyze.add_chart)
            && panel == AnalysisPanel::Results
            && state.analysis_state.results.is_some()
        {
            handle_add_chart(state);
            return EventResult::Handled;
        }

        // -: Delete chart (only if there's a chart at the selected index)
        if KeybindingsConfig::matches(&key, &kb.tabs.analyze.delete_chart)
            && panel == AnalysisPanel::Results
        {
            let idx = state.analysis_state.selected_chart_index;
            if idx < state.analysis_state.chart_configs.len() {
                state.analysis_state.chart_configs.remove(idx);
                state.mark_modified();
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
