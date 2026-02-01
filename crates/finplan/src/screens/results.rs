use std::collections::HashMap;

use crate::components::panels::LedgerPanel;
use crate::components::portfolio_overview::{AccountBar, PortfolioOverviewChart};
use crate::components::{Component, EventResult};
use crate::data::keybindings_data::KeybindingsConfig;
use crate::event::{AppKeyEvent, KeyCode};
use crate::state::{AppState, PercentileView, ResultsPanel, SimulationResult, ValueDisplayMode};
use crate::util::format::{format_currency, format_currency_short};
use finplan_core::model::{AccountId, AccountSnapshotFlavor, WealthSnapshot};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, List, ListItem, Paragraph},
};

use super::Screen;

pub struct ResultsScreen;

impl ResultsScreen {
    /// Build a map of AccountId to account names from the current simulation data
    fn build_account_name_map(state: &AppState) -> HashMap<AccountId, String> {
        let mut map = HashMap::new();
        for (idx, account) in state.data().portfolios.accounts.iter().enumerate() {
            let id = AccountId((idx + 1) as u16);
            map.insert(id, account.name.clone());
        }
        map
    }

    /// Get the current TUI result based on viewing mode (Monte Carlo percentile or single run)
    fn get_current_tui_result(state: &AppState) -> Option<&SimulationResult> {
        if state.results_state.viewing_monte_carlo {
            if let Some(mc) = &state.monte_carlo_result {
                match state.results_state.percentile_view {
                    PercentileView::P5 => mc.get_percentile_tui(0.05),
                    PercentileView::P50 => mc.get_percentile_tui(0.50),
                    PercentileView::P95 => mc.get_percentile_tui(0.95),
                    PercentileView::Mean => mc.mean_tui_result.as_ref(),
                }
            } else {
                state.simulation_result.as_ref()
            }
        } else {
            state.simulation_result.as_ref()
        }
    }

    /// Get the current core result based on viewing mode (Monte Carlo percentile or single run)
    fn get_current_core_result(state: &AppState) -> Option<&finplan_core::model::SimulationResult> {
        if state.results_state.viewing_monte_carlo {
            if let Some(mc) = &state.monte_carlo_result {
                match state.results_state.percentile_view {
                    PercentileView::P5 => mc.get_percentile_core(0.05),
                    PercentileView::P50 => mc.get_percentile_core(0.50),
                    PercentileView::P95 => mc.get_percentile_core(0.95),
                    PercentileView::Mean => mc.mean_core_result.as_ref(),
                }
            } else {
                state.core_simulation_result.as_ref()
            }
        } else {
            state.core_simulation_result.as_ref()
        }
    }

    /// Get the wealth snapshot for the selected year using current result
    fn get_wealth_snapshot_for_year_current(
        state: &AppState,
        year_index: usize,
    ) -> Option<&WealthSnapshot> {
        let core_result = Self::get_current_core_result(state)?;
        let tui_result = Self::get_current_tui_result(state)?;

        if year_index >= tui_result.years.len() {
            return None;
        }

        let target_year = tui_result.years[year_index].year as i16;

        // Find the last snapshot for this year (any month)
        core_result
            .wealth_snapshots
            .iter()
            .rfind(|snap| snap.date.year() == target_year)
    }

    /// Get the list of unique years from the current simulation result
    fn get_years_current(state: &AppState) -> Vec<i16> {
        Self::get_current_tui_result(state)
            .map(|result| result.years.iter().map(|y| y.year as i16).collect())
            .unwrap_or_default()
    }

    /// Calculate optimal bar width and gap for net worth chart
    /// Returns (bar_width, bar_gap, total_width_needed)
    fn calculate_chart_sizing(num_years: usize, available_width: usize) -> (u16, u16, usize) {
        if num_years == 0 {
            return (3, 1, 0);
        }

        // Try widths 3, 2, 1 with gap of 1, then gap of 0
        for &bw in &[3u16, 2, 1] {
            for &bg in &[1u16, 0] {
                let total = num_years * (bw as usize) + num_years.saturating_sub(1) * (bg as usize);
                if total <= available_width {
                    return (bw, bg, total);
                }
            }
        }

        // Minimum case: width 1, gap 0
        let total = num_years;
        (1, 0, total)
    }

    fn render_chart(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        // Get selected year for highlighting
        let years = Self::get_years_current(state);
        let year_index = state
            .results_state
            .selected_year_index
            .min(years.len().saturating_sub(1));
        let selected_year = years.get(year_index).copied().unwrap_or(0) as i32;

        // Check display mode
        let display_mode = state.results_state.value_display_mode;
        let mode_label = display_mode.short_label();

        // Build title with percentile and display mode indicators
        let title = if state.results_state.viewing_monte_carlo {
            let pct = state.results_state.percentile_view.short_label();
            if focused {
                format!(
                    " NET WORTH PROJECTION ({}) ({}) ({}) [h/l year, v view, $ toggle] ",
                    selected_year, pct, mode_label
                )
            } else {
                format!(
                    " NET WORTH PROJECTION ({}) ({}) ({}) ",
                    selected_year, pct, mode_label
                )
            }
        } else if focused {
            format!(
                " NET WORTH PROJECTION ({}) ({}) [h/l year, $ toggle] ",
                selected_year, mode_label
            )
        } else {
            format!(
                " NET WORTH PROJECTION ({}) ({}) ",
                selected_year, mode_label
            )
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        if let Some(result) = Self::get_current_tui_result(state) {
            if result.years.is_empty() {
                let paragraph = Paragraph::new("No data to display").block(block);
                frame.render_widget(paragraph, area);
                return;
            }

            let num_years = result.years.len();
            // Available width inside borders
            let inner_width = area.width.saturating_sub(2) as usize;

            // Calculate optimal bar sizing (prefer 3/2/1 width with gap 1)
            let (bar_width, bar_gap, _) = Self::calculate_chart_sizing(num_years, inner_width);

            // Check if we can fit all bars, otherwise sample
            let total_needed =
                num_years * (bar_width as usize) + num_years.saturating_sub(1) * (bar_gap as usize);
            let step = if total_needed > inner_width && inner_width > 0 {
                let max_bars = inner_width / (bar_width as usize + bar_gap as usize).max(1);
                (num_years as f64 / max_bars as f64).ceil() as usize
            } else {
                1
            };

            // Determine which final net worth to use for color scaling
            let final_nw_for_scale = match display_mode {
                ValueDisplayMode::Nominal => result.final_net_worth,
                ValueDisplayMode::Real => result.final_real_net_worth,
            };

            // Create bars for the chart
            let bars: Vec<Bar> = result
                .years
                .iter()
                .step_by(step.max(1))
                .map(|year| {
                    // Use real or nominal value based on display mode
                    let net_worth_value = match display_mode {
                        ValueDisplayMode::Nominal => year.net_worth,
                        ValueDisplayMode::Real => year.real_net_worth,
                    };

                    let value = (net_worth_value / 1000.0).max(0.0) as u64;
                    let is_selected = year.year == selected_year;

                    // Highlight selected year with white/bright style
                    let style = if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        self.net_worth_style(net_worth_value, final_nw_for_scale)
                    };

                    let label_style = if is_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };

                    // Show age as label
                    let label = if bar_width >= 2 {
                        Line::from(Span::styled(format!("{}", year.age), label_style))
                    } else {
                        Line::from("")
                    };

                    Bar::default()
                        .value(value)
                        .label(label)
                        .text_value(format_currency(net_worth_value))
                        .style(style)
                        .value_style(style.reversed())
                })
                .collect();

            // Render legend in top-left corner (inside the block area)
            let inner_area = block.inner(area);
            frame.render_widget(block.clone(), area);

            // Legend text
            let legend = vec![Line::from(vec![
                Span::styled("\u{2588}", Style::default().fg(Color::Red)),
                Span::raw(" <0  "),
                Span::styled("\u{2588}", Style::default().fg(Color::Yellow)),
                Span::raw(" <25%  "),
                Span::styled("\u{2588}", Style::default().fg(Color::LightYellow)),
                Span::raw(" <50%  "),
                Span::styled("\u{2588}", Style::default().fg(Color::LightGreen)),
                Span::raw(" <75%  "),
                Span::styled("\u{2588}", Style::default().fg(Color::Green)),
                Span::raw(" >75%"),
            ])];
            let legend_paragraph =
                Paragraph::new(legend).style(Style::default().fg(Color::DarkGray));

            // Split inner area: legend row at top, chart in middle, label at bottom
            let chart_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .split(inner_area);

            frame.render_widget(legend_paragraph, chart_layout[0]);

            let chart = BarChart::default()
                .data(BarGroup::default().bars(&bars))
                .bar_width(bar_width)
                .bar_gap(bar_gap)
                .direction(Direction::Vertical);

            frame.render_widget(chart, chart_layout[1]);

            // Bottom label
            let bottom_label = Paragraph::new("Net worth by age")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(bottom_label, chart_layout[2]);
        } else {
            let content = vec![
                Line::from(""),
                Line::from("No simulation results available."),
                Line::from(""),
                Line::from("Run a simulation from the Scenario screen to see results here."),
            ];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        }
    }

    fn net_worth_style(&self, value: f64, final_value: f64) -> Style {
        let ratio = if final_value > 0.0 {
            (value / final_value).clamp(0.0, 1.5)
        } else {
            0.0
        };

        if value < 0.0 {
            Style::default().fg(Color::Red)
        } else if ratio < 0.25 {
            Style::default().fg(Color::Yellow)
        } else if ratio < 0.5 {
            Style::default().fg(Color::LightYellow)
        } else if ratio < 0.75 {
            Style::default().fg(Color::LightGreen)
        } else {
            Style::default().fg(Color::Green)
        }
    }

    fn render_yearly_breakdown(
        &self,
        frame: &mut Frame,
        area: Rect,
        state: &AppState,
        focused: bool,
    ) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        // Get selected year for highlighting
        let years = Self::get_years_current(state);
        let year_index = state
            .results_state
            .selected_year_index
            .min(years.len().saturating_sub(1));
        let selected_year = years.get(year_index).copied().unwrap_or(0) as i32;

        // Check display mode
        let display_mode = state.results_state.value_display_mode;
        let mode_label = display_mode.short_label();

        let items: Vec<ListItem> = if let Some(result) = Self::get_current_tui_result(state) {
            // Calculate visible rows (account for borders, header, summary)
            let visible_count = (area.height as usize).saturating_sub(5);
            let total_years = result.years.len();

            // Center-based scrolling: keep selection in the middle when possible
            let center = visible_count / 2;

            // Calculate scroll offset to center the selected year
            let start_idx = if year_index <= center {
                // Near the top: selection moves down from top, no scroll needed
                0
            } else if year_index >= total_years.saturating_sub(visible_count.saturating_sub(center))
            {
                // Near the bottom: keep at least half the visible rows showing
                // This ensures context is visible even at the end
                total_years.saturating_sub(visible_count)
            } else {
                // Middle: center the selection
                year_index.saturating_sub(center)
            };

            // Determine which final net worth to display
            let final_nw_display = match display_mode {
                ValueDisplayMode::Nominal => result.final_net_worth,
                ValueDisplayMode::Real => result.final_real_net_worth,
            };

            // Summary section
            let mut items = vec![
                ListItem::new(Line::from(vec![Span::styled(
                    format!(
                        "Final: {} ({})  Years: {}",
                        format_currency(final_nw_display),
                        mode_label,
                        result.years.len()
                    ),
                    Style::default().add_modifier(Modifier::BOLD),
                )])),
                ListItem::new(Line::from("")),
                ListItem::new(Line::from(vec![Span::styled(
                    format!(
                        "{:>5} {:>4} {:>10} {:>10} {:>10} {:>10} {:>10} {:>12}",
                        "Year",
                        "Age",
                        "Income",
                        "Withdraw",
                        "Contrib",
                        "Expense",
                        "Taxes",
                        "Net Worth"
                    ),
                    Style::default().add_modifier(Modifier::BOLD),
                )])),
            ];

            // Data rows with highlighting for selected year
            for year in result.years.iter().skip(start_idx).take(visible_count) {
                let is_selected = year.year == selected_year;

                // Use real or nominal values based on display mode
                let (income, expenses, net_worth) = match display_mode {
                    ValueDisplayMode::Nominal => (year.income, year.expenses, year.net_worth),
                    ValueDisplayMode::Real => {
                        (year.real_income, year.real_expenses, year.real_net_worth)
                    }
                };

                let row_text = format!(
                    "{:>5} {:>4} {:>10} {:>10} {:>10} {:>10} {:>10} {:>12}",
                    year.year,
                    year.age,
                    format_currency_short(income),
                    format_currency_short(year.withdrawals),
                    format_currency_short(year.contributions),
                    format_currency_short(expenses),
                    format_currency_short(year.taxes),
                    format_currency_short(net_worth)
                );

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                items.push(ListItem::new(Line::from(Span::styled(row_text, style))));
            }

            items
        } else {
            vec![ListItem::new(Line::from("No data"))]
        };

        // Build title with percentile and display mode indicators
        let title = if state.results_state.viewing_monte_carlo {
            let pct = state.results_state.percentile_view.short_label();
            if focused {
                format!(
                    " YEARLY BREAKDOWN ({}) ({}) ({}) [j/k scroll, v view, $ toggle] ",
                    selected_year, pct, mode_label
                )
            } else {
                format!(
                    " YEARLY BREAKDOWN ({}) ({}) ({}) ",
                    selected_year, pct, mode_label
                )
            }
        } else if focused {
            format!(
                " YEARLY BREAKDOWN ({}) ({}) [j/k scroll, $ toggle] ",
                selected_year, mode_label
            )
        } else {
            format!(" YEARLY BREAKDOWN ({}) ({}) ", selected_year, mode_label)
        };

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        );

        frame.render_widget(list, area);
    }

    fn render_account_chart(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let years = Self::get_years_current(state);
        let year_index = state
            .results_state
            .selected_year_index
            .min(years.len().saturating_sub(1));
        let selected_year = years.get(year_index).copied().unwrap_or(0);

        // Check display mode
        let display_mode = state.results_state.value_display_mode;
        let mode_label = display_mode.short_label();

        // Get inflation factor for the selected year (for real value calculation)
        // Use the first year in results as the base year for inflation indexing
        let first_year = years.first().copied().unwrap_or(selected_year);
        let inflation_index = (selected_year - first_year).max(0) as usize;
        let inflation_factor = Self::get_current_core_result(state)
            .and_then(|core| core.cumulative_inflation.get(inflation_index).copied())
            .unwrap_or(1.0);

        // Build title with percentile and display mode indicators
        let title = if state.results_state.viewing_monte_carlo {
            let pct = state.results_state.percentile_view.short_label();
            if focused {
                format!(
                    " ACCOUNT BREAKDOWN ({}) ({}) ({}) [h/l year, v view, $ toggle] ",
                    selected_year, pct, mode_label
                )
            } else {
                format!(
                    " ACCOUNT BREAKDOWN ({}) ({}) ({}) ",
                    selected_year, pct, mode_label
                )
            }
        } else if focused {
            format!(
                " ACCOUNT BREAKDOWN ({}) ({}) [h/l year, $ toggle] ",
                selected_year, mode_label
            )
        } else {
            format!(" ACCOUNT BREAKDOWN ({}) ({}) ", selected_year, mode_label)
        };

        if let Some(snapshot) = Self::get_wealth_snapshot_for_year_current(state, year_index) {
            let account_names = Self::build_account_name_map(state);

            // Convert snapshot accounts to AccountBar instances
            let accounts: Vec<AccountBar> = snapshot
                .accounts
                .iter()
                .map(|acc| {
                    let name = account_names
                        .get(&acc.account_id)
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown");

                    let nominal_value = acc.total_value();

                    // Apply inflation adjustment if in Real mode
                    let value = match display_mode {
                        ValueDisplayMode::Nominal => nominal_value,
                        ValueDisplayMode::Real => {
                            if inflation_factor > 0.0 {
                                nominal_value / inflation_factor
                            } else {
                                nominal_value
                            }
                        }
                    };

                    let label = match &acc.flavor {
                        AccountSnapshotFlavor::Investment { assets, .. } => {
                            format!("{} ({} assets)", name, assets.len())
                        }
                        _ => name.to_string(),
                    };

                    // Color based on account type: Gold=Yellow, negative=Red, otherwise Green
                    let color = if value < 0.0 {
                        Color::Red
                    } else {
                        Color::Green
                    };

                    AccountBar::new(label, value, color)
                })
                .collect();

            PortfolioOverviewChart::new(&accounts)
                .title(title)
                .focused(focused)
                .value_overlay(true)
                .line_spacing(1)
                .render(frame, area);
        } else {
            let border_style = if focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title);

            let content = vec![
                Line::from(""),
                Line::from("No account data for selected year."),
            ];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        }
    }
}

impl Component for ResultsScreen {
    fn handle_key(&mut self, key: AppKeyEvent, state: &mut AppState) -> EventResult {
        let panel = state.results_state.focused_panel;
        let kb = &state.keybindings;

        // Panel navigation
        if KeybindingsConfig::matches(&key, &kb.navigation.next_panel) {
            state.results_state.focused_panel = panel.next();
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.navigation.prev_panel) {
            state.results_state.focused_panel = panel.prev();
            return EventResult::Handled;
        }

        // j/k (down/up) scrolling for YearlyBreakdown and Ledger
        if KeybindingsConfig::matches(&key, &kb.navigation.down) {
            match panel {
                ResultsPanel::YearlyBreakdown => {
                    let years = Self::get_years_current(state);
                    if state.results_state.selected_year_index + 1 < years.len() {
                        state.results_state.selected_year_index += 1;
                        // Scroll offset is calculated in render, not here
                    }
                }
                ResultsPanel::Ledger => {
                    let filtered_count = LedgerPanel::get_filtered_count(state);
                    if state.results_state.ledger_scroll_offset + 1 < filtered_count {
                        state.results_state.ledger_scroll_offset += 1;
                    }
                }
                _ => {}
            }
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.navigation.up) {
            match panel {
                ResultsPanel::YearlyBreakdown => {
                    if state.results_state.selected_year_index > 0 {
                        state.results_state.selected_year_index -= 1;
                        // Scroll offset is calculated in render, not here
                    }
                }
                ResultsPanel::Ledger => {
                    if state.results_state.ledger_scroll_offset > 0 {
                        state.results_state.ledger_scroll_offset -= 1;
                    }
                }
                _ => {}
            }
            return EventResult::Handled;
        }

        // h/l (prev/next year) for year selection (works in NetWorthChart, AccountChart, YearlyBreakdown)
        if KeybindingsConfig::matches(&key, &kb.tabs.results.prev_year) {
            match panel {
                ResultsPanel::NetWorthChart
                | ResultsPanel::AccountChart
                | ResultsPanel::YearlyBreakdown => {
                    if state.results_state.selected_year_index > 0 {
                        state.results_state.selected_year_index -= 1;
                        // Scroll offset is calculated in render, not here
                    }
                }
                _ => {}
            }
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.tabs.results.next_year) {
            match panel {
                ResultsPanel::NetWorthChart
                | ResultsPanel::AccountChart
                | ResultsPanel::YearlyBreakdown => {
                    let years = Self::get_years_current(state);
                    if state.results_state.selected_year_index + 1 < years.len() {
                        state.results_state.selected_year_index += 1;
                        // Scroll offset is calculated in render, not here
                    }
                }
                _ => {}
            }
            return EventResult::Handled;
        }

        // Home/End for first/last year (works in NetWorthChart, AccountChart, YearlyBreakdown)
        if KeybindingsConfig::matches(&key, &kb.tabs.results.first_year) {
            match panel {
                ResultsPanel::NetWorthChart
                | ResultsPanel::AccountChart
                | ResultsPanel::YearlyBreakdown => {
                    state.results_state.selected_year_index = 0;
                    // Scroll offset is calculated in render, not here
                }
                _ => {}
            }
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &kb.tabs.results.last_year) {
            match panel {
                ResultsPanel::NetWorthChart
                | ResultsPanel::AccountChart
                | ResultsPanel::YearlyBreakdown => {
                    let years = Self::get_years_current(state);
                    state.results_state.selected_year_index = years.len().saturating_sub(1);
                    // Scroll offset is calculated in render, not here
                }
                _ => {}
            }
            return EventResult::Handled;
        }

        // PageUp/PageDown for fast ledger scrolling
        if key.code == KeyCode::PageDown {
            if panel == ResultsPanel::Ledger {
                let filtered_count = LedgerPanel::get_filtered_count(state);
                let new_offset = state.results_state.ledger_scroll_offset + 10;
                state.results_state.ledger_scroll_offset =
                    new_offset.min(filtered_count.saturating_sub(1));
            }
            return EventResult::Handled;
        }
        if key.code == KeyCode::PageUp {
            if panel == ResultsPanel::Ledger {
                state.results_state.ledger_scroll_offset =
                    state.results_state.ledger_scroll_offset.saturating_sub(10);
            }
            return EventResult::Handled;
        }

        // f for cycling ledger filter
        if KeybindingsConfig::matches(&key, &kb.tabs.results.cycle_filter) {
            if panel == ResultsPanel::Ledger {
                state.results_state.ledger_filter = state.results_state.ledger_filter.next();
                state.results_state.ledger_scroll_offset = 0; // Reset scroll when filter changes
            }
            return EventResult::Handled;
        }

        // v for cycling percentile view (Monte Carlo only)
        if KeybindingsConfig::matches(&key, &kb.tabs.results.cycle_percentile) {
            if state.results_state.viewing_monte_carlo {
                state.results_state.percentile_view = state.results_state.percentile_view.next();
            }
            return EventResult::Handled;
        }

        // $ for toggling between nominal and real (inflation-adjusted) values
        if KeybindingsConfig::matches(&key, &kb.tabs.results.toggle_real) {
            state.results_state.value_display_mode =
                state.results_state.value_display_mode.toggle();
            return EventResult::Handled;
        }

        // Legacy keys for export (not yet implemented)
        if key.code == KeyCode::Char('e') {
            state.set_error("Export CSV not yet implemented".to_string());
            return EventResult::Handled;
        }
        if key.code == KeyCode::Char('p') {
            state.set_error("PDF report not yet implemented".to_string());
            return EventResult::Handled;
        }

        EventResult::NotHandled
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let panel = state.results_state.focused_panel;

        // 2x2 grid layout
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50), // Top row
                Constraint::Percentage(50), // Bottom row
            ])
            .split(area);

        // Calculate exact width needed for net worth chart
        let num_years = Self::get_current_tui_result(state)
            .map(|r| r.years.len())
            .unwrap_or(0);

        // Available width for top row
        let top_row_width = rows[0].width as usize;

        // Minimum width for account breakdown to show names + values properly
        const MIN_ACCOUNT_WIDTH: usize = 35;

        // Calculate maximum available space for chart content (inside borders)
        // Must leave room for MIN_ACCOUNT_WIDTH for the account panel
        let max_chart_content_width = top_row_width
            .saturating_sub(MIN_ACCOUNT_WIDTH)
            .saturating_sub(2); // 2 for chart borders

        // Calculate bar sizing that fits within the available space
        let (_, _, chart_content_width) =
            Self::calculate_chart_sizing(num_years, max_chart_content_width);

        // Chart width = content + borders
        let chart_width = if num_years > 0 {
            (chart_content_width + 2) as u16
        } else {
            // No data, use reasonable default (leave room for account view)
            top_row_width.saturating_sub(MIN_ACCOUNT_WIDTH) as u16
        };

        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(chart_width), // Net Worth Chart - exact fit
                Constraint::Min(MIN_ACCOUNT_WIDTH as u16), // Account Breakdown - minimum width
            ])
            .split(rows[0]);

        let bottom_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50), // Yearly Breakdown
                Constraint::Percentage(50), // Ledger
            ])
            .split(rows[1]);

        // Render all 4 panels
        self.render_chart(
            frame,
            top_cols[0],
            state,
            panel == ResultsPanel::NetWorthChart,
        );
        self.render_account_chart(
            frame,
            top_cols[1],
            state,
            panel == ResultsPanel::AccountChart,
        );
        self.render_yearly_breakdown(
            frame,
            bottom_cols[0],
            state,
            panel == ResultsPanel::YearlyBreakdown,
        );
        LedgerPanel::render(frame, bottom_cols[1], state, panel == ResultsPanel::Ledger);
    }
}

impl Screen for ResultsScreen {
    fn title(&self) -> &str {
        "Results"
    }
}
