use std::collections::HashMap;

use crate::components::{Component, EventResult};
use crate::state::{AppState, LedgerFilter, PercentileView, ResultsPanel, SimulationResult};
use crate::util::format::format_currency;
use crossterm::event::{KeyCode, KeyEvent};
use finplan_core::model::{
    AccountId, AccountSnapshotFlavor, LedgerEntry, StateEvent, WealthSnapshot,
};
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
    pub fn new() -> Self {
        Self
    }

    /// Build a map of AccountId to account names from the current simulation data
    fn build_account_name_map(state: &AppState) -> HashMap<AccountId, String> {
        let mut map = HashMap::new();
        for (idx, account) in state.data().portfolios.accounts.iter().enumerate() {
            let id = AccountId((idx + 1) as u16);
            map.insert(id, account.name.clone());
        }
        map
    }

    /// Format a StateEvent for display in the ledger
    fn format_state_event(
        event: &StateEvent,
        account_names: &HashMap<AccountId, String>,
    ) -> String {
        match event {
            StateEvent::TimeAdvance {
                from_date,
                to_date,
                days_elapsed,
            } => {
                format!("Time: {} -> {} ({} days)", from_date, to_date, days_elapsed)
            }
            StateEvent::CreateAccount(account) => {
                let name = account_names
                    .get(&account.account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Create account: {}", name)
            }
            StateEvent::DeleteAccount(id) => {
                let name = account_names
                    .get(id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Delete account: {}", name)
            }
            StateEvent::CashCredit { to, amount } => {
                let name = account_names
                    .get(to)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Credit {} to {}", format_currency(*amount), name)
            }
            StateEvent::CashDebit { from, amount } => {
                let name = account_names
                    .get(from)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Debit {} from {}", format_currency(*amount), name)
            }
            StateEvent::CashAppreciation {
                account_id,
                previous_value,
                new_value,
                return_rate,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let gain = new_value - previous_value;
                format!(
                    "{}: {} appreciation ({:.2}%)",
                    name,
                    format_currency(gain),
                    return_rate * 100.0
                )
            }
            StateEvent::LiabilityInterestAccrual {
                account_id,
                previous_principal,
                new_principal,
                interest_rate,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let interest = new_principal - previous_principal;
                format!(
                    "{}: {} interest accrued ({:.2}%)",
                    name,
                    format_currency(interest),
                    interest_rate * 100.0
                )
            }
            StateEvent::AssetPurchase {
                account_id,
                units,
                cost_basis,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!(
                    "{}: Buy {:.2} units for {}",
                    name,
                    units,
                    format_currency(*cost_basis)
                )
            }
            StateEvent::AssetSale {
                account_id,
                units,
                proceeds,
                short_term_gain,
                long_term_gain,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let total_gain = short_term_gain + long_term_gain;
                format!(
                    "{}: Sell {:.2} units for {} (gain: {})",
                    name,
                    units,
                    format_currency(*proceeds),
                    format_currency(total_gain)
                )
            }
            StateEvent::IncomeTax {
                gross_amount,
                federal_tax,
                state_tax,
            } => {
                let total = federal_tax + state_tax;
                format!(
                    "Income tax on {}: {} (Fed: {}, State: {})",
                    format_currency(*gross_amount),
                    format_currency(total),
                    format_currency(*federal_tax),
                    format_currency(*state_tax)
                )
            }
            StateEvent::ShortTermCapitalGainsTax {
                gross_gain,
                federal_tax,
                state_tax,
            } => {
                let total = federal_tax + state_tax;
                format!(
                    "ST Cap Gains tax on {}: {}",
                    format_currency(*gross_gain),
                    format_currency(total)
                )
            }
            StateEvent::LongTermCapitalGainsTax {
                gross_gain,
                federal_tax,
                state_tax,
            } => {
                let total = federal_tax + state_tax;
                format!(
                    "LT Cap Gains tax on {}: {}",
                    format_currency(*gross_gain),
                    format_currency(total)
                )
            }
            StateEvent::EventTriggered { event_id } => {
                format!("Event triggered: #{}", event_id.0)
            }
            StateEvent::EventPaused { event_id } => {
                format!("Event paused: #{}", event_id.0)
            }
            StateEvent::EventResumed { event_id } => {
                format!("Event resumed: #{}", event_id.0)
            }
            StateEvent::EventTerminated { event_id } => {
                format!("Event terminated: #{}", event_id.0)
            }
            StateEvent::YearRollover { from_year, to_year } => {
                format!("Year rollover: {} -> {}", from_year, to_year)
            }
            StateEvent::RmdWithdrawal {
                account_id,
                age,
                required_amount,
                actual_amount,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!(
                    "{}: RMD at age {} - required {}, withdrew {}",
                    name,
                    age,
                    format_currency(*required_amount),
                    format_currency(*actual_amount)
                )
            }
        }
    }

    /// Check if a ledger entry matches the current filter
    fn matches_ledger_filter(entry: &LedgerEntry, filter: LedgerFilter) -> bool {
        match filter {
            LedgerFilter::All => true,
            LedgerFilter::CashOnly => entry.event.is_cash_event(),
            LedgerFilter::AssetsOnly => entry.event.is_asset_event(),
            LedgerFilter::TaxesOnly => entry.event.is_tax_event(),
            LedgerFilter::EventsOnly => entry.event.is_event_management(),
        }
    }

    /// Get the color for a ledger entry based on its type
    fn get_event_color(event: &StateEvent) -> Color {
        if event.is_cash_event() {
            Color::Cyan
        } else if event.is_asset_event() {
            Color::Magenta
        } else if event.is_tax_event() {
            Color::Red
        } else if event.is_event_management() {
            Color::Yellow
        } else {
            Color::Gray
        }
    }

    /// Get the current TUI result based on viewing mode (Monte Carlo percentile or single run)
    fn get_current_tui_result(state: &AppState) -> Option<&SimulationResult> {
        if state.results_state.viewing_monte_carlo {
            if let Some(mc) = &state.monte_carlo_result {
                match state.results_state.percentile_view {
                    PercentileView::P5 => Some(&mc.p5_result),
                    PercentileView::P50 => Some(&mc.p50_result),
                    PercentileView::P95 => Some(&mc.p95_result),
                    PercentileView::Mean => Some(&mc.mean_result),
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
                    PercentileView::P5 => Some(&mc.p5_core),
                    PercentileView::P50 => Some(&mc.p50_core),
                    PercentileView::P95 => Some(&mc.p95_core),
                    PercentileView::Mean => Some(&mc.mean_core),
                }
            } else {
                state.core_simulation_result.as_ref()
            }
        } else {
            state.core_simulation_result.as_ref()
        }
    }

    /// Get the wealth snapshot for the selected year using current result
    fn get_wealth_snapshot_for_year_current<'a>(
        state: &'a AppState,
        year_index: usize,
    ) -> Option<&'a WealthSnapshot> {
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
            .filter(|snap| snap.date.year() == target_year)
            .next_back()
    }

    /// Get the list of unique years from the current simulation result
    fn get_years_current(state: &AppState) -> Vec<i16> {
        Self::get_current_tui_result(state)
            .map(|result| result.years.iter().map(|y| y.year as i16).collect())
            .unwrap_or_default()
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

        // Build title with percentile indicator if viewing Monte Carlo
        let title = if state.results_state.viewing_monte_carlo {
            let pct = state.results_state.percentile_view.short_label();
            if focused {
                format!(" NET WORTH PROJECTION ({}) ({}) [h/l year, v view] ", selected_year, pct)
            } else {
                format!(" NET WORTH PROJECTION ({}) ({}) ", selected_year, pct)
            }
        } else {
            if focused {
                format!(" NET WORTH PROJECTION ({}) [h/l year] ", selected_year)
            } else {
                format!(" NET WORTH PROJECTION ({}) ", selected_year)
            }
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

            // Calculate optimal bar_width and bar_gap to fit all years
            // Formula: num_years * bar_width + (num_years - 1) * bar_gap <= inner_width
            // We want bar_width >= 1 and bar_gap >= 0
            let (bar_width, bar_gap) = if num_years == 0 {
                (1, 1)
            } else {
                // Try different combinations to fit all bars
                // Start with preferred widths and reduce if needed
                let mut bw = 4u16;
                let mut bg = 1u16;

                loop {
                    let total_width =
                        num_years * (bw as usize) + (num_years.saturating_sub(1)) * (bg as usize);
                    if total_width <= inner_width {
                        break;
                    }
                    // Reduce gap first
                    if bg > 0 {
                        bg = 0;
                    } else if bw > 1 {
                        // Then reduce bar width
                        bw -= 1;
                        bg = 0; // Reset gap when reducing width
                    } else {
                        // Can't fit, will need to sample
                        break;
                    }
                }
                (bw, bg)
            };

            // Check if we can fit all bars, otherwise sample
            let total_needed = num_years * (bar_width as usize)
                + (num_years.saturating_sub(1)) * (bar_gap as usize);
            let step = if total_needed > inner_width && inner_width > 0 {
                // Calculate step to sample years
                let max_bars = inner_width / (bar_width as usize + bar_gap as usize).max(1);
                (num_years as f64 / max_bars as f64).ceil() as usize
            } else {
                1
            };

            // Create bars for the chart
            let bars: Vec<Bar> = result
                .years
                .iter()
                .step_by(step.max(1))
                .map(|year| {
                    let value = (year.net_worth / 1000.0).max(0.0) as u64;
                    let is_selected = year.year == selected_year;

                    // Highlight selected year with white/bright style
                    let style = if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        self.net_worth_style(year.net_worth, result.final_net_worth)
                    };

                    let label_style = if is_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };

                    // Only show year label if bar is wide enough
                    let label = if bar_width >= 4 {
                        Line::from(Span::styled(format!("{}", year.year), label_style))
                    } else if bar_width >= 2 {
                        // Show short year (last 2 digits)
                        Line::from(Span::styled(format!("{:02}", year.year % 100), label_style))
                    } else {
                        Line::from("")
                    };

                    Bar::default()
                        .value(value)
                        .label(label)
                        .text_value(format_currency(year.net_worth))
                        .style(style)
                        .value_style(style.reversed())
                })
                .collect();

            let chart = BarChart::default()
                .block(block)
                .data(BarGroup::default().bars(&bars))
                .bar_width(bar_width)
                .bar_gap(bar_gap)
                .direction(Direction::Vertical);

            frame.render_widget(chart, area);
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

        let items: Vec<ListItem> = if let Some(result) = Self::get_current_tui_result(state) {
            // Auto-scroll to keep selected year visible
            let visible_count = (area.height as usize).saturating_sub(5); // Account for borders, header, summary
            let start_idx = state.results_state.scroll_offset;

            // Summary section
            let mut items = vec![
                ListItem::new(Line::from(vec![Span::styled(
                    format!(
                        "Final: {}  Years: {}",
                        format_currency(result.final_net_worth),
                        result.years.len()
                    ),
                    Style::default().add_modifier(Modifier::BOLD),
                )])),
                ListItem::new(Line::from("")),
                ListItem::new(Line::from(vec![Span::styled(
                    format!(
                        "{:>6} {:>5} {:>12} {:>12} {:>12} {:>12}",
                        "Year", "Age", "Income", "Expense", "Taxes", "Net Worth"
                    ),
                    Style::default().add_modifier(Modifier::BOLD),
                )])),
            ];

            // Data rows with highlighting for selected year
            for year in result.years.iter().skip(start_idx).take(visible_count) {
                let is_selected = year.year == selected_year;
                let row_text = format!(
                    "{:>6} {:>5} {:>12} {:>12} {:>12} {:>12}",
                    year.year,
                    year.age,
                    format_currency(year.income),
                    format_currency(year.expenses),
                    format_currency(year.taxes),
                    format_currency(year.net_worth)
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

        // Build title with percentile indicator if viewing Monte Carlo
        let title = if state.results_state.viewing_monte_carlo {
            let pct = state.results_state.percentile_view.short_label();
            if focused {
                format!(" YEARLY BREAKDOWN ({}) ({}) [j/k scroll, v view] ", selected_year, pct)
            } else {
                format!(" YEARLY BREAKDOWN ({}) ({}) ", selected_year, pct)
            }
        } else {
            if focused {
                format!(" YEARLY BREAKDOWN ({}) [j/k scroll] ", selected_year)
            } else {
                format!(" YEARLY BREAKDOWN ({}) ", selected_year)
            }
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
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let years = Self::get_years_current(state);
        let year_index = state
            .results_state
            .selected_year_index
            .min(years.len().saturating_sub(1));
        let selected_year = years.get(year_index).copied().unwrap_or(0);

        // Build title with percentile indicator if viewing Monte Carlo
        let title = if state.results_state.viewing_monte_carlo {
            let pct = state.results_state.percentile_view.short_label();
            if focused {
                format!(" ACCOUNT BREAKDOWN ({}) ({}) [h/l year, v view] ", selected_year, pct)
            } else {
                format!(" ACCOUNT BREAKDOWN ({}) ({}) ", selected_year, pct)
            }
        } else {
            if focused {
                format!(" ACCOUNT BREAKDOWN ({}) [h/l year] ", selected_year)
            } else {
                format!(" ACCOUNT BREAKDOWN ({}) ", selected_year)
            }
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        if let Some(snapshot) = Self::get_wealth_snapshot_for_year_current(state, year_index) {
            let account_names = Self::build_account_name_map(state);

            if snapshot.accounts.is_empty() {
                let paragraph = Paragraph::new("No accounts").block(block);
                frame.render_widget(paragraph, area);
                return;
            }

            // Create horizontal bars
            let bars: Vec<Bar> = snapshot
                .accounts
                .iter()
                .map(|acc| {
                    let name = account_names
                        .get(&acc.account_id)
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown");

                    let value = acc.total_value();
                    let label = match &acc.flavor {
                        AccountSnapshotFlavor::Investment { assets, .. } => {
                            format!("{} ({} assets)", name, assets.len())
                        }
                        _ => name.to_string(),
                    };

                    // Scale to u64 for bar chart (in thousands)
                    let scaled = (value.abs() / 1000.0) as u64;
                    let style = if value >= 0.0 {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Red)
                    };

                    Bar::default()
                        .value(scaled)
                        .label(Line::from(label))
                        .text_value(format_currency(value))
                        .style(style)
                        .value_style(style.reversed())
                })
                .collect();

            let chart = BarChart::default()
                .block(block)
                .data(BarGroup::default().bars(&bars))
                .bar_width(3)
                .bar_gap(1)
                .direction(Direction::Horizontal);

            frame.render_widget(chart, area);
        } else {
            let content = vec![
                Line::from(""),
                Line::from("No account data for selected year."),
            ];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        }
    }

    fn render_ledger(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let filter = state.results_state.ledger_filter;

        // Build title with percentile indicator if viewing Monte Carlo
        let title = if state.results_state.viewing_monte_carlo {
            let pct = state.results_state.percentile_view.short_label();
            if focused {
                format!(" LEDGER [{}] ({}) [j/k scroll, f filter, v view] ", filter.label(), pct)
            } else {
                format!(" LEDGER [{}] ({}) ", filter.label(), pct)
            }
        } else {
            if focused {
                format!(" LEDGER [{}] [j/k scroll, f filter] ", filter.label())
            } else {
                format!(" LEDGER [{}] ", filter.label())
            }
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        if let Some(core_result) = Self::get_current_core_result(state) {
            let account_names = Self::build_account_name_map(state);

            // Filter entries
            let filtered_entries: Vec<&LedgerEntry> = core_result
                .ledger
                .iter()
                .filter(|entry| Self::matches_ledger_filter(entry, filter))
                .collect();

            if filtered_entries.is_empty() {
                let paragraph = Paragraph::new("No matching entries").block(block);
                frame.render_widget(paragraph, area);
                return;
            }

            let visible_count = (area.height as usize).saturating_sub(2);
            let scroll_offset = state
                .results_state
                .ledger_scroll_offset
                .min(filtered_entries.len().saturating_sub(1));

            let items: Vec<ListItem> = filtered_entries
                .iter()
                .skip(scroll_offset)
                .take(visible_count)
                .map(|entry| {
                    let color = Self::get_event_color(&entry.event);
                    let text = Self::format_state_event(&entry.event, &account_names);
                    let line = Line::from(vec![
                        Span::styled(
                            format!("[{}] ", entry.date),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(text, Style::default().fg(color)),
                    ]);
                    ListItem::new(line)
                })
                .collect();

            let list = List::new(items).block(block);
            frame.render_widget(list, area);
        } else {
            let paragraph = Paragraph::new("No simulation data").block(block);
            frame.render_widget(paragraph, area);
        }
    }
}

impl Component for ResultsScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let panel = state.results_state.focused_panel;

        match key.code {
            // Panel navigation
            KeyCode::Tab => {
                state.results_state.focused_panel = panel.next();
                EventResult::Handled
            }
            KeyCode::BackTab => {
                state.results_state.focused_panel = panel.prev();
                EventResult::Handled
            }

            // j/k scrolling for YearlyBreakdown and Ledger
            KeyCode::Char('j') | KeyCode::Down => {
                match panel {
                    ResultsPanel::YearlyBreakdown => {
                        let years = Self::get_years_current(state);
                        if state.results_state.selected_year_index + 1 < years.len() {
                            state.results_state.selected_year_index += 1;
                            state.results_state.scroll_offset =
                                state.results_state.selected_year_index;
                        }
                    }
                    ResultsPanel::Ledger => {
                        if let Some(core_result) = Self::get_current_core_result(state) {
                            let filter = state.results_state.ledger_filter;
                            let filtered_count = core_result
                                .ledger
                                .iter()
                                .filter(|e| Self::matches_ledger_filter(e, filter))
                                .count();
                            if state.results_state.ledger_scroll_offset + 1 < filtered_count {
                                state.results_state.ledger_scroll_offset += 1;
                            }
                        }
                    }
                    _ => {}
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                match panel {
                    ResultsPanel::YearlyBreakdown => {
                        if state.results_state.selected_year_index > 0 {
                            state.results_state.selected_year_index -= 1;
                            state.results_state.scroll_offset =
                                state.results_state.selected_year_index;
                        }
                    }
                    ResultsPanel::Ledger => {
                        if state.results_state.ledger_scroll_offset > 0 {
                            state.results_state.ledger_scroll_offset -= 1;
                        }
                    }
                    _ => {}
                }
                EventResult::Handled
            }

            // h/l or Left/Right for year selection (works in NetWorthChart, AccountChart, YearlyBreakdown)
            KeyCode::Char('h') | KeyCode::Left => {
                match panel {
                    ResultsPanel::NetWorthChart
                    | ResultsPanel::AccountChart
                    | ResultsPanel::YearlyBreakdown => {
                        if state.results_state.selected_year_index > 0 {
                            state.results_state.selected_year_index -= 1;
                            // Sync yearly breakdown scroll to selected year
                            state.results_state.scroll_offset =
                                state.results_state.selected_year_index;
                        }
                    }
                    _ => {}
                }
                EventResult::Handled
            }
            KeyCode::Char('l') | KeyCode::Right => {
                match panel {
                    ResultsPanel::NetWorthChart
                    | ResultsPanel::AccountChart
                    | ResultsPanel::YearlyBreakdown => {
                        let years = Self::get_years_current(state);
                        if state.results_state.selected_year_index + 1 < years.len() {
                            state.results_state.selected_year_index += 1;
                            // Sync yearly breakdown scroll to selected year
                            state.results_state.scroll_offset =
                                state.results_state.selected_year_index;
                        }
                    }
                    _ => {}
                }
                EventResult::Handled
            }

            // Home/End for first/last year (works in NetWorthChart, AccountChart, YearlyBreakdown)
            KeyCode::Home => {
                match panel {
                    ResultsPanel::NetWorthChart
                    | ResultsPanel::AccountChart
                    | ResultsPanel::YearlyBreakdown => {
                        state.results_state.selected_year_index = 0;
                        state.results_state.scroll_offset = 0;
                    }
                    _ => {}
                }
                EventResult::Handled
            }
            KeyCode::End => {
                match panel {
                    ResultsPanel::NetWorthChart
                    | ResultsPanel::AccountChart
                    | ResultsPanel::YearlyBreakdown => {
                        let years = Self::get_years_current(state);
                        state.results_state.selected_year_index = years.len().saturating_sub(1);
                        state.results_state.scroll_offset = state.results_state.selected_year_index;
                    }
                    _ => {}
                }
                EventResult::Handled
            }

            // PageUp/PageDown for fast ledger scrolling
            KeyCode::PageDown => {
                if panel == ResultsPanel::Ledger
                    && let Some(core_result) = Self::get_current_core_result(state)
                {
                    let filter = state.results_state.ledger_filter;
                    let filtered_count = core_result
                        .ledger
                        .iter()
                        .filter(|e| Self::matches_ledger_filter(e, filter))
                        .count();
                    let new_offset = state.results_state.ledger_scroll_offset + 10;
                    state.results_state.ledger_scroll_offset =
                        new_offset.min(filtered_count.saturating_sub(1));
                }
                EventResult::Handled
            }
            KeyCode::PageUp => {
                if panel == ResultsPanel::Ledger {
                    state.results_state.ledger_scroll_offset =
                        state.results_state.ledger_scroll_offset.saturating_sub(10);
                }
                EventResult::Handled
            }

            // f for cycling ledger filter
            KeyCode::Char('f') => {
                if panel == ResultsPanel::Ledger {
                    state.results_state.ledger_filter = state.results_state.ledger_filter.next();
                    state.results_state.ledger_scroll_offset = 0; // Reset scroll when filter changes
                }
                EventResult::Handled
            }

            // v for cycling percentile view (Monte Carlo only)
            KeyCode::Char('v') => {
                if state.results_state.viewing_monte_carlo {
                    state.results_state.percentile_view =
                        state.results_state.percentile_view.next();
                }
                EventResult::Handled
            }

            // Legacy keys for export (not yet implemented)
            KeyCode::Char('e') => {
                state.set_error("Export CSV not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('p') => {
                state.set_error("PDF report not yet implemented".to_string());
                EventResult::Handled
            }

            _ => EventResult::NotHandled,
        }
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

        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70), // Net Worth Chart
                Constraint::Percentage(30), // Account Breakdown
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
        self.render_ledger(frame, bottom_cols[1], state, panel == ResultsPanel::Ledger);
    }
}

impl Screen for ResultsScreen {
    fn title(&self) -> &str {
        "Results"
    }
}
