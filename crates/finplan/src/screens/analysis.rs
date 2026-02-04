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
use crate::data::keybindings_data::KeybindingsConfig;
use crate::modals::{AnalysisAction, ConfirmedValue, ModalAction, ModalState};
use crate::state::{AnalysisPanel, AnalysisResults, AppState};
use crate::util::format::{format_compact_currency, format_currency_short};
use crate::{
    actions::ActionResult,
    data::analysis_data::{AnalysisMetricData, ColorScheme},
};
use crate::{actions::analysis::handle_analysis_action, util::styles::focused_block_with_help};
use crate::{
    components::{Component, EventResult},
    util::styles::focused_block,
};

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
        let block =
            focused_block_with_help(" SWEEP PARAMETERS ", focused, "[a]dd [d]el [Enter] edit");

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
                    let bounds = if param.sweep_type.is_currency() {
                        format!(
                            "[{}-{}, {} steps]",
                            format_currency_short(param.min_value),
                            format_currency_short(param.max_value),
                            param.step_count
                        )
                    } else {
                        format!(
                            "[{:.0}-{:.0}, {} steps]",
                            param.min_value, param.max_value, param.step_count
                        )
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

    /// Render the Metrics panel (right-top) - static list of computed metrics
    fn render_metrics(&self, frame: &mut Frame, area: Rect, _state: &AppState, focused: bool) {
        let block = focused_block(" METRICS ", focused);

        let items: Vec<ListItem> = AVAILABLE_METRICS
            .iter()
            .map(|metric| {
                let color = metric_color(metric);
                ListItem::new(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled("●", Style::default().fg(color)),
                    Span::raw(" "),
                    Span::styled(metric.label(), Style::default().fg(Color::White)),
                ]))
            })
            .collect();

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }

    /// Render the Config panel (right-middle)
    fn render_config(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let block = focused_block(" CONFIGURATION ", focused);

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
        ];

        let paragraph = Paragraph::new(content).block(block);
        frame.render_widget(paragraph, area);
    }

    /// Render the Results panel (bottom) with 1D line chart(s) or 2D heatmap
    fn render_results(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
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

        let block =
            focused_block_with_help(title.as_str(), focused, "[h/l] select [c]configure chart");

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
                self.render_single_1d_chart(frame, area, results, config, is_selected);
            }
            ChartType::Heatmap2D => {
                self.render_mini_2d_heatmap(frame, area, results, config, is_selected);
            }
        }
    }

    /// Get 7-color heatmap gradient for a given color scheme (viridis family)
    fn heatmap_gradient(scheme: ColorScheme) -> Vec<Color> {
        match scheme {
            // Viridis: purple -> teal -> green -> yellow (perceptually uniform)
            ColorScheme::Viridis => vec![
                Color::Rgb(68, 1, 84),    // Deep purple
                Color::Rgb(59, 82, 139),  // Blue
                Color::Rgb(33, 145, 140), // Teal
                Color::Rgb(42, 121, 142), // Teal-blue
                Color::Rgb(92, 200, 99),  // Green
                Color::Rgb(180, 222, 44), // Yellow-green
                Color::Rgb(253, 231, 37), // Yellow
            ],
            // Magma: dark purple -> magenta -> pink -> light yellow
            ColorScheme::Magma => vec![
                Color::Rgb(0, 0, 4),       // Near black
                Color::Rgb(46, 15, 94),    // Deep purple
                Color::Rgb(135, 38, 129),  // Magenta
                Color::Rgb(205, 64, 113),  // Pink-red
                Color::Rgb(242, 100, 159), // Pink
                Color::Rgb(253, 138, 189), // Light pink
                Color::Rgb(252, 253, 191), // Light yellow
            ],
            // Inferno: dark purple -> red/orange -> yellow
            ColorScheme::Inferno => vec![
                Color::Rgb(0, 0, 4),       // Near black
                Color::Rgb(52, 10, 95),    // Deep purple
                Color::Rgb(131, 31, 105),  // Purple-magenta
                Color::Rgb(205, 72, 60),   // Orange-red
                Color::Rgb(245, 132, 15),  // Orange
                Color::Rgb(251, 194, 80),  // Yellow-orange
                Color::Rgb(252, 255, 164), // Light yellow
            ],
            // Plasma: blue -> purple -> orange -> yellow
            ColorScheme::Plasma => vec![
                Color::Rgb(13, 8, 135),   // Deep blue
                Color::Rgb(97, 5, 135),   // Purple
                Color::Rgb(163, 22, 114), // Magenta
                Color::Rgb(212, 76, 85),  // Red-pink
                Color::Rgb(241, 129, 48), // Orange
                Color::Rgb(250, 194, 40), // Yellow-orange
                Color::Rgb(240, 249, 33), // Yellow
            ],
            // Cividis: dark blue -> gray/tan -> yellow (colorblind-friendly)
            ColorScheme::Cividis => vec![
                Color::Rgb(0, 32, 77),     // Dark blue
                Color::Rgb(35, 62, 108),   // Blue
                Color::Rgb(84, 90, 108),   // Gray-blue
                Color::Rgb(138, 135, 121), // Gray-tan
                Color::Rgb(191, 176, 110), // Tan
                Color::Rgb(233, 211, 88),  // Yellow-tan
                Color::Rgb(255, 234, 70),  // Yellow
            ],
            // Rocket: dark blue -> magenta -> pink/cream
            ColorScheme::Rocket => vec![
                Color::Rgb(3, 5, 26),      // Near black
                Color::Rgb(104, 31, 85),   // Deep magenta
                Color::Rgb(188, 22, 86),   // Magenta-red
                Color::Rgb(241, 100, 69),  // Orange-red
                Color::Rgb(246, 176, 137), // Peach
                Color::Rgb(250, 229, 212), // Cream
                Color::Rgb(250, 235, 221), // Light cream
            ],
            // Mako: dark -> purple -> teal/cyan -> light
            ColorScheme::Mako => vec![
                Color::Rgb(11, 4, 5),      // Near black
                Color::Rgb(55, 40, 83),    // Purple
                Color::Rgb(103, 150, 168), // Teal
                Color::Rgb(167, 207, 195), // Light teal
                Color::Rgb(214, 233, 217), // Pale green
                Color::Rgb(248, 245, 229), // Cream
                Color::Rgb(222, 245, 229), // Light
            ],
            // Turbo: purple -> blue -> cyan -> green -> yellow -> orange -> red
            ColorScheme::Turbo => vec![
                Color::Rgb(48, 18, 59),   // Deep purple
                Color::Rgb(71, 115, 235), // Blue
                Color::Rgb(51, 173, 247), // Cyan
                Color::Rgb(113, 254, 95), // Green
                Color::Rgb(202, 42, 3),   // Orange
                Color::Rgb(149, 13, 1),   // Red-orange
                Color::Rgb(122, 4, 3),    // Dark red
            ],
        }
    }

    /// Render a 2D heatmap in a chart slot with axes and legend
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

        // Get 2D data using config's x/y parameters
        let x_dim = config.x_param_index;
        let y_dim = config.y_param_index.unwrap_or(1);
        let Some((matrix, min_val, max_val)) = results.get_2d_metric_matrix_for_config(
            &config.metric,
            x_dim,
            y_dim,
            &config.fixed_values,
        ) else {
            let placeholder =
                Paragraph::new("No data").alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(placeholder, inner);
            return;
        };

        if matrix.is_empty() || matrix[0].is_empty() {
            return;
        }

        let data_rows = matrix.len();
        let data_cols = matrix[0].len();

        // Get parameter ranges for axis labels
        let x_values = results.param_values(x_dim);
        let y_values = results.param_values(y_dim);
        let x_label = results.param_label(x_dim);
        let y_label = results.param_label(y_dim);

        let (x_min, x_max) = if x_values.is_empty() {
            (0.0, 1.0)
        } else {
            (
                x_values.first().copied().unwrap_or(0.0),
                x_values.last().copied().unwrap_or(1.0),
            )
        };
        let (y_min, y_max) = if y_values.is_empty() {
            (0.0, 1.0)
        } else {
            (
                y_values.first().copied().unwrap_or(0.0),
                y_values.last().copied().unwrap_or(1.0),
            )
        };

        // Value scale for colors - always use actual data range for better contrast
        let scale_min = min_val;
        let scale_max = max_val;
        let range = (scale_max - scale_min).max(0.0001);

        // Get color gradient from config
        let colors = Self::heatmap_gradient(config.color_scheme);

        // Layout: Y-axis labels | heatmap | legend
        // Top row: Y-axis title
        // Below heatmap: X-axis labels + title
        let y_label_width: u16 = 6; // Space for Y-axis labels
        let legend_width: u16 = 8; // Space for legend
        let top_padding: u16 = 1; // Space for Y-axis title
        let x_label_height: u16 = 2; // Space for X-axis labels + title (no extra row)

        // Calculate available space for heatmap
        let heatmap_width = inner.width.saturating_sub(y_label_width + legend_width + 1);
        let heatmap_height = inner.height.saturating_sub(x_label_height + top_padding);

        if heatmap_width < 4 || heatmap_height < 2 {
            return; // Too small to render
        }

        // Calculate cell sizes with remainder distribution for even fill
        // Base sizes via integer division
        let base_cell_width = (heatmap_width as usize / data_cols).max(1);
        let base_cell_height = (heatmap_height as usize / data_rows).max(1);
        // Remainder to distribute among first N cells
        let extra_cols = heatmap_width as usize % data_cols;
        let extra_rows = heatmap_height as usize % data_rows;

        // Use full available space
        let actual_heatmap_width = heatmap_width as usize;
        let actual_heatmap_height = heatmap_height as usize;

        // Heatmap area (offset by top_padding for Y-axis title)
        let heatmap_x = inner.x + y_label_width;
        let heatmap_y = inner.y + top_padding;

        // Render Y-axis labels (standard orientation: y_max at top, y_min at bottom)
        // Check if axis represents currency (label ends with "Amount")
        let y_is_currency = y_label.ends_with("Amount");
        let format_y = |v: f64| {
            if y_is_currency {
                format_compact_currency(v).replace('$', "")
            } else {
                format!("{:.0}", v)
            }
        };
        let y_mid = (y_min + y_max) / 2.0;
        let y_labels = [
            (0, format_y(y_max)),
            (actual_heatmap_height / 2, format_y(y_mid)),
            (actual_heatmap_height.saturating_sub(1), format_y(y_min)),
        ];

        for (row_offset, label) in y_labels {
            if row_offset < actual_heatmap_height {
                let label_area = Rect::new(
                    inner.x,
                    heatmap_y + row_offset as u16,
                    y_label_width.saturating_sub(1),
                    1,
                );
                let label_text = Paragraph::new(label).alignment(ratatui::layout::Alignment::Right);
                frame.render_widget(label_text, label_area);
            }
        }

        // Render Y-axis title on the top padding row (extend over heatmap area for longer labels)
        let y_title_width = y_label_width + (actual_heatmap_width as u16 / 2);
        let title_area = Rect::new(inner.x, inner.y, y_title_width, 1);
        let title_text = Paragraph::new(Span::styled(
            y_label.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(title_text, title_area);

        // Helper to calculate cumulative position with distributed remainder
        let row_start = |row: usize| -> usize {
            // First 'extra_rows' rows get an extra pixel
            let extra = row.min(extra_rows);
            row * base_cell_height + extra
        };
        let col_start = |col: usize| -> usize {
            let extra = col.min(extra_cols);
            col * base_cell_width + extra
        };

        // Render heatmap cells
        for (data_row, row_data) in matrix.iter().enumerate().take(data_rows) {
            for (data_col, &val) in row_data.iter().enumerate().take(data_cols) {
                let normalized = ((val - scale_min) / range).clamp(0.0, 1.0);
                let color_idx = (normalized * (colors.len() - 1) as f64).round() as usize;
                let color = colors[color_idx.min(colors.len() - 1)];

                // Calculate screen position for this cell using distributed sizing
                // Matrix row 0 = top of heatmap (high Y value)
                let screen_row = row_start(data_row);
                let screen_col = col_start(data_col);
                let cell_height = row_start(data_row + 1) - screen_row;
                let cell_width = col_start(data_col + 1) - screen_col;

                // Render the cell as filled blocks
                for dy in 0..cell_height {
                    let y_pos = heatmap_y + screen_row as u16 + dy as u16;
                    if y_pos >= heatmap_y + actual_heatmap_height as u16 {
                        break;
                    }

                    let cell_str: String = "█".repeat(cell_width);
                    let cell_area =
                        Rect::new(heatmap_x + screen_col as u16, y_pos, cell_width as u16, 1);
                    let cell_widget =
                        Paragraph::new(Span::styled(cell_str, Style::default().fg(color)));
                    frame.render_widget(cell_widget, cell_area);
                }
            }
        }

        // Render X-axis labels (low, mid, high)
        let x_axis_y = heatmap_y + actual_heatmap_height as u16;
        let x_is_currency = x_label.ends_with("Amount");
        let format_x = |v: f64| {
            if x_is_currency {
                format_compact_currency(v).replace('$', "")
            } else {
                format!("{:.0}", v)
            }
        };
        let x_mid = (x_min + x_max) / 2.0;
        let x_labels = [
            (0usize, format_x(x_min)),
            (actual_heatmap_width / 2, format_x(x_mid)),
            (actual_heatmap_width.saturating_sub(4), format_x(x_max)),
        ];

        for (col_offset, label) in x_labels {
            let label_area = Rect::new(
                heatmap_x + col_offset as u16,
                x_axis_y,
                label.len() as u16 + 1,
                1,
            );
            let label_text = Paragraph::new(label);
            frame.render_widget(label_text, label_area);
        }

        // Render X-axis title (centered under heatmap)
        if x_axis_y + 1 < inner.y + inner.height {
            // Center the title properly: start position = heatmap_x + (heatmap_width - title_len) / 2
            let title_len = x_label.len() as u16;
            let title_x = heatmap_x + (actual_heatmap_width as u16).saturating_sub(title_len) / 2;
            let title_area = Rect::new(title_x, x_axis_y + 1, title_len, 1);
            let title_text = Paragraph::new(Span::styled(
                x_label.to_string(),
                Style::default().fg(Color::DarkGray),
            ));
            frame.render_widget(title_text, title_area);
        }

        // Render legend (aligned with heatmap, accounting for top_padding)
        let legend_x = heatmap_x + actual_heatmap_width as u16 + 1;
        let legend_height = colors.len().min(actual_heatmap_height);

        // Legend title on top padding row
        let legend_title_area = Rect::new(legend_x, inner.y, legend_width, 1);
        let legend_title = if config.metric == AnalysisMetricData::SuccessRate {
            "%"
        } else {
            "$"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                legend_title,
                Style::default().fg(Color::DarkGray),
            )),
            legend_title_area,
        );

        // Legend color bars with values (start at heatmap_y, aligned with heatmap)
        for (i, color) in colors.iter().enumerate().take(legend_height) {
            let legend_row = heatmap_y + i as u16;
            if legend_row >= inner.y + inner.height {
                break;
            }

            // Color block
            let block_area = Rect::new(legend_x, legend_row, 2, 1);
            frame.render_widget(
                Paragraph::new(Span::styled("██", Style::default().fg(*color))),
                block_area,
            );

            // Value label (show for first, middle, and last)
            // colors[0] = purple = low value, colors[last] = yellow = high value
            if i == 0 || i == colors.len() - 1 || i == colors.len() / 2 {
                let val =
                    scale_min + (scale_max - scale_min) * i as f64 / (colors.len() - 1) as f64;
                let val_str = if config.metric == AnalysisMetricData::SuccessRate {
                    format!("{:.0}%", val)
                } else {
                    format_compact_currency(val).replace("$", "")
                };
                let val_area = Rect::new(legend_x + 2, legend_row, legend_width - 2, 1);
                frame.render_widget(
                    Paragraph::new(Span::styled(val_str, Style::default().fg(Color::DarkGray))),
                    val_area,
                );
            }
        }
    }

    /// Render a single 1D chart based on config
    fn render_single_1d_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        results: &AnalysisResults,
        config: &crate::data::analysis_data::ChartConfigData,
        is_selected: bool,
    ) {
        let metric = &config.metric;
        let x_dim = config.x_param_index;

        // Get data for the configured dimension
        let (param_values, values) =
            results.get_1d_metric_data_for_config(metric, x_dim, &config.fixed_values);

        if values.is_empty() || param_values.is_empty() {
            return;
        }

        // Convert to chart data points: (x, y) tuples
        let data: Vec<(f64, f64)> = param_values
            .iter()
            .zip(values.iter())
            .map(|(&x, &y)| (x, y))
            .collect();

        // If there are multiple sweep dimensions, show min/max spread of this metric
        // across the other (non-X-axis) dimensions to indicate sensitivity
        let (min_data, max_data) = if results.ndim() > 1 {
            let (spread_params, min_values, max_values) =
                results.get_1d_metric_spread_across_other_dims(metric, x_dim);

            // Only show if there's actual variation
            let has_variation = min_values
                .iter()
                .zip(max_values.iter())
                .any(|(min, max)| (max - min).abs() > 0.01);

            if has_variation {
                let min_pts: Vec<(f64, f64)> = spread_params
                    .iter()
                    .zip(min_values.iter())
                    .map(|(&x, &y)| (x, y))
                    .collect();
                let max_pts: Vec<(f64, f64)> = spread_params
                    .iter()
                    .zip(max_values.iter())
                    .map(|(&x, &y)| (x, y))
                    .collect();
                (Some(min_pts), Some(max_pts))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        // Calculate bounds with padding
        let x_min = param_values.first().copied().unwrap_or(0.0);
        let x_max = param_values.last().copied().unwrap_or(1.0);
        let x_padding = (x_max - x_min).abs() * 0.02;

        // Calculate y bounds - include P10/P90 data if present for proper scaling
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
            // Include min/max spread in bounds calculation if present
            let mut all_values: Vec<f64> = values.clone();
            if let Some(ref min_pts) = min_data {
                all_values.extend(min_pts.iter().map(|(_, y)| *y));
            }
            if let Some(ref max_pts) = max_data {
                all_values.extend(max_pts.iter().map(|(_, y)| *y));
            }

            let min = all_values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = all_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let padding = (max - min).abs().max(1.0) * 0.1;
            (min - padding, max + padding)
        };

        // Create dataset with metric-specific color
        let color = metric_color(metric);

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
                Span::raw(format_compact_currency(y_min)),
                Span::raw(format_compact_currency((y_min + y_max) / 2.0)),
                Span::raw(format_compact_currency(y_max)),
            ]
        };

        // Use the correct parameter label for the x-axis
        let x_label = results.param_label(x_dim);
        let x_axis = Axis::default()
            .title(x_label.to_string().dark_gray())
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

        // Build datasets - boundary lines first (so main data renders on top)
        let mut datasets = Vec::new();

        // Add spread boundary lines showing min/max of this metric across other sweep params
        if let Some(ref min_pts) = min_data {
            datasets.push(
                Dataset::default()
                    .name("Min")
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::DarkGray))
                    .data(min_pts),
            );
        }

        if let Some(ref max_pts) = max_data {
            datasets.push(
                Dataset::default()
                    .name("Max")
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::DarkGray))
                    .data(max_pts),
            );
        }

        // Add main dataset on top
        datasets.push(
            Dataset::default()
                .name(metric.short_label())
                .marker(symbols::Marker::Dot)
                .graph_type(GraphType::Scatter)
                .style(Style::default().fg(color))
                .data(&data),
        );

        let chart = Chart::new(datasets)
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

        // List navigation (j/k or Up/Down in Parameters panel)
        if KeybindingsConfig::matches(&key, &kb.navigation.down)
            && panel == AnalysisPanel::Parameters
        {
            let param_count = state.analysis_state.sweep_parameters.len();
            if param_count > 0 {
                state.analysis_state.selected_param_index =
                    (state.analysis_state.selected_param_index + 1) % param_count;
            }
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.navigation.up) && panel == AnalysisPanel::Parameters
        {
            let param_count = state.analysis_state.sweep_parameters.len();
            if param_count > 0 {
                if state.analysis_state.selected_param_index == 0 {
                    state.analysis_state.selected_param_index = param_count - 1;
                } else {
                    state.analysis_state.selected_param_index -= 1;
                }
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
                    // Metrics panel is static - no action on Enter
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
