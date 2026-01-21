use crate::components::{Component, EventResult};
use crate::data::portfolio_data::AccountType;
use crate::state::{
    AppState, ConfirmModal, FieldType, FormField, FormModal, MessageModal, ModalAction, ModalState,
    ScenarioPanel, ScenarioPickerModal, TabId,
};
use crate::util::format::{format_currency, format_currency_short};
use crossterm::event::{KeyCode, KeyEvent};
use jiff::civil::Date;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, List, ListItem, Paragraph},
};

use super::Screen;

pub struct ScenarioScreen;

impl ScenarioScreen {
    fn parse_date(date_str: &str) -> Option<Date> {
        date_str.parse().ok()
    }

    fn calculate_age(&self, state: &AppState) -> Option<u8> {
        let birth_date = Self::parse_date(&state.data().parameters.birth_date)?;
        let start_date = Self::parse_date(&state.data().parameters.start_date)?;

        let years = start_date.year() - birth_date.year();
        let had_birthday =
            (start_date.month(), start_date.day()) >= (birth_date.month(), birth_date.day());

        if had_birthday {
            Some(years as u8)
        } else {
            Some((years - 1) as u8)
        }
    }

    fn calculate_end_age(&self, state: &AppState) -> Option<u8> {
        let start_age = self.calculate_age(state)?;
        Some(start_age + state.data().parameters.duration_years as u8)
    }

    fn calculate_net_worth(&self, state: &AppState) -> f64 {
        state
            .data()
            .portfolios
            .accounts
            .iter()
            .map(|acc| match &acc.account_type {
                AccountType::Checking(prop)
                | AccountType::Savings(prop)
                | AccountType::HSA(prop)
                | AccountType::Property(prop)
                | AccountType::Collectible(prop) => prop.value,
                AccountType::Brokerage(inv)
                | AccountType::Traditional401k(inv)
                | AccountType::Roth401k(inv)
                | AccountType::TraditionalIRA(inv)
                | AccountType::RothIRA(inv) => inv.assets.iter().map(|a| a.value).sum(),
                AccountType::Mortgage(debt)
                | AccountType::LoanDebt(debt)
                | AccountType::StudentLoanDebt(debt) => -debt.balance,
            })
            .sum()
    }

    /// Get the selected scenario name from the sorted list
    fn get_selected_scenario_name(&self, state: &AppState) -> Option<String> {
        let scenarios = state.get_scenario_list_with_summaries();
        scenarios
            .get(state.scenario_state.selected_index)
            .map(|(name, _)| name.clone())
    }
}

impl Component for ScenarioScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let panel = state.scenario_state.focused_panel;
        let scenarios = state.get_scenario_list_with_summaries();
        let num_scenarios = scenarios.len();

        match key.code {
            // Panel navigation
            KeyCode::Tab => {
                state.scenario_state.focused_panel = panel.next();
                EventResult::Handled
            }
            KeyCode::BackTab => {
                state.scenario_state.focused_panel = panel.prev();
                EventResult::Handled
            }

            // Scenario list navigation (j/k or up/down)
            KeyCode::Char('j') | KeyCode::Down => {
                if panel.is_left_panel() && num_scenarios > 0 {
                    state.scenario_state.selected_index =
                        (state.scenario_state.selected_index + 1) % num_scenarios;
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if panel.is_left_panel() && num_scenarios > 0 {
                    state.scenario_state.selected_index = state
                        .scenario_state
                        .selected_index
                        .checked_sub(1)
                        .unwrap_or(num_scenarios - 1);
                }
                EventResult::Handled
            }

            // Enter to switch to selected scenario
            KeyCode::Enter => {
                if panel.is_left_panel()
                    && let Some(selected_name) = self.get_selected_scenario_name(state)
                {
                    state.switch_scenario(&selected_name);
                }
                EventResult::Handled
            }

            // New scenario
            KeyCode::Char('n') => {
                let form = FormModal::new(
                    "New Scenario",
                    vec![FormField::new("Scenario Name", FieldType::Text, "")],
                    ModalAction::NEW_SCENARIO,
                )
                .start_editing();
                state.modal = ModalState::Form(form);
                EventResult::Handled
            }

            // Duplicate scenario
            KeyCode::Char('d') => {
                if let Some(selected_name) = self.get_selected_scenario_name(state) {
                    let new_name = format!("{} (copy)", selected_name);
                    let form = FormModal::new(
                        "Duplicate Scenario",
                        vec![FormField::new("New Name", FieldType::Text, &new_name)],
                        ModalAction::DUPLICATE_SCENARIO,
                    )
                    .start_editing();
                    state.modal = ModalState::Form(form);
                }
                EventResult::Handled
            }

            // Delete scenario
            KeyCode::Delete | KeyCode::Backspace => {
                if num_scenarios > 1 {
                    if let Some(selected_name) = self.get_selected_scenario_name(state) {
                        state.modal = ModalState::Confirm(ConfirmModal::new(
                            "Delete Scenario",
                            &format!("Are you sure you want to delete '{}'?", selected_name),
                            ModalAction::DELETE_SCENARIO,
                        ));
                    }
                } else {
                    state.set_error("Cannot delete the last scenario".to_string());
                }
                EventResult::Handled
            }

            // Run Monte Carlo on current scenario
            KeyCode::Char('m') => {
                match state.run_monte_carlo(1000) {
                    Ok(()) => {
                        // Stay on scenario tab to see updated summary
                    }
                    Err(e) => state.set_error(format!("Monte Carlo simulation failed: {}", e)),
                }
                EventResult::Handled
            }

            // Run All scenarios
            KeyCode::Char('R') => {
                match state.run_monte_carlo_all(1000) {
                    Ok(count) => {
                        state.modal = ModalState::Message(MessageModal::info(
                            "Batch Run Complete",
                            &format!("Ran Monte Carlo on {} scenarios.", count),
                        ));
                    }
                    Err(e) => state.set_error(format!("Batch Monte Carlo failed: {}", e)),
                }
                EventResult::Handled
            }

            // Run single simulation and switch to results
            KeyCode::Char('r') => {
                match state.run_simulation() {
                    Ok(()) => {
                        state.switch_tab(TabId::Results);
                        state.results_state.scroll_offset = 0;
                    }
                    Err(e) => state.set_error(format!("Simulation failed: {}", e)),
                }
                EventResult::Handled
            }

            // Preview projection
            KeyCode::Char('p') => {
                if let Err(e) = state.run_projection_preview() {
                    state.set_error(format!("Projection failed: {}", e));
                }
                EventResult::Handled
            }

            // Save scenario
            KeyCode::Char('s') => {
                let scenarios = state.scenario_names();
                state.modal = ModalState::ScenarioPicker(ScenarioPickerModal::new(
                    "Save Scenario As",
                    scenarios,
                    ModalAction::SAVE_AS,
                ));
                EventResult::Handled
            }

            // Load scenario
            KeyCode::Char('l') => {
                let scenarios = state.scenario_names();
                if scenarios.is_empty() {
                    state.modal = ModalState::Message(MessageModal::info(
                        "No Scenarios",
                        "No scenarios available to load.",
                    ));
                } else {
                    state.modal = ModalState::ScenarioPicker(ScenarioPickerModal::new(
                        "Load Scenario",
                        scenarios,
                        ModalAction::LOAD,
                    ));
                }
                EventResult::Handled
            }

            // Edit parameters
            KeyCode::Char('e') => {
                let params = &state.data().parameters;

                let start_date = if params.start_date.is_empty() {
                    jiff::Zoned::now().date().strftime("%Y-%m-%d").to_string()
                } else {
                    params.start_date.clone()
                };

                let form = FormModal::new(
                    "Edit Simulation Parameters",
                    vec![
                        FormField::new("Start Date (YYYY-MM-DD)", FieldType::Text, &start_date),
                        FormField::new(
                            "Birth Date (YYYY-MM-DD)",
                            FieldType::Text,
                            &params.birth_date,
                        ),
                        FormField::new(
                            "Duration (years)",
                            FieldType::Text,
                            &params.duration_years.to_string(),
                        ),
                    ],
                    ModalAction::EDIT_PARAMETERS,
                );
                state.modal = ModalState::Form(form);
                EventResult::Handled
            }

            // Import scenario
            KeyCode::Char('i') => {
                let default_path = dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("scenario.yaml")
                    .to_string_lossy()
                    .to_string();

                let form = FormModal::new(
                    "Import Scenario",
                    vec![FormField::new("File path", FieldType::Text, &default_path)],
                    ModalAction::IMPORT,
                )
                .start_editing();
                state.modal = ModalState::Form(form);
                EventResult::Handled
            }

            // Export scenario
            KeyCode::Char('x') => {
                let default_path = dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join(format!("{}.yaml", state.current_scenario))
                    .to_string_lossy()
                    .to_string();

                let form = FormModal::new(
                    "Export Scenario",
                    vec![FormField::new("File path", FieldType::Text, &default_path)],
                    ModalAction::EXPORT,
                )
                .start_editing();
                state.modal = ModalState::Form(form);
                EventResult::Handled
            }

            _ => EventResult::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        // Main layout: 2 columns
        let main_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // Left column: scenarios
                Constraint::Percentage(60), // Right column: comparison
            ])
            .split(area);

        // Left column: scenario list + selected details
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),   // Scenario list
                Constraint::Length(9), // Selected scenario details
            ])
            .split(main_cols[0]);

        // Right column: comparison table + overlay chart
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(12), // Comparison table with percentiles
                Constraint::Min(8),     // Overlay chart
            ])
            .split(main_cols[1]);

        let panel = state.scenario_state.focused_panel;

        self.render_scenario_list(
            frame,
            left_chunks[0],
            state,
            panel == ScenarioPanel::ScenarioList,
        );
        self.render_selected_details(
            frame,
            left_chunks[1],
            state,
            panel == ScenarioPanel::ScenarioDetails,
        );
        self.render_comparison_table(
            frame,
            right_chunks[0],
            state,
            panel == ScenarioPanel::ComparisonTable,
        );
        self.render_overlay_chart(
            frame,
            right_chunks[1],
            state,
            panel == ScenarioPanel::OverlayChart,
        );
    }
}

impl ScenarioScreen {
    fn render_scenario_list(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let scenarios = state.get_scenario_list_with_summaries();
        let selected_index = state.scenario_state.selected_index;

        let items: Vec<ListItem> = scenarios
            .iter()
            .enumerate()
            .map(|(idx, (name, summary))| {
                let is_current = name == &state.current_scenario;
                let is_selected = idx == selected_index;
                let is_dirty = state.dirty_scenarios.contains(name);

                // Format: > name*  $X.XXM  XX%
                let prefix = if is_current { ">" } else { " " };
                let dirty_marker = if is_dirty { "*" } else { "" };

                // Combine name and dirty marker, then truncate/pad to fixed width
                let name_with_marker = format!("{}{}", name, dirty_marker);
                let display_name: String = if name_with_marker.len() > 12 {
                    name_with_marker
                        .chars()
                        .take(11)
                        .chain(std::iter::once('…'))
                        .collect()
                } else {
                    name_with_marker
                };

                let (final_nw, success) = if let Some(s) = summary {
                    (
                        s.final_net_worth
                            .map(format_currency_short)
                            .unwrap_or_else(|| "--".to_string()),
                        s.success_rate
                            .map(|r| format!("{:.0}%", r * 100.0))
                            .unwrap_or_else(|| "--".to_string()),
                    )
                } else {
                    ("--".to_string(), "--".to_string())
                };

                let line_text = format!(
                    "{} {:<12} {:>12} {:>5}",
                    prefix, display_name, final_nw, success
                );

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(Span::styled(line_text, style)))
            })
            .collect();

        // Keybinds help
        let keybinds = Line::from(vec![
            Span::styled(
                "[n]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("ew "),
            Span::styled(
                "[d]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("up "),
            Span::styled(
                "[Del]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("ete "),
            Span::styled(
                "[R]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("un All "),
            Span::styled(
                "[Enter]",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" switch"),
        ]);

        let title = if focused {
            " SCENARIOS [j/k nav, Tab panels] "
        } else {
            " SCENARIOS "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        // Layout for list + keybinds
        let inner = block.inner(area);
        let inner_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);

        frame.render_widget(block, area);

        let list = List::new(items);
        frame.render_widget(list, inner_chunks[0]);

        let keybinds_para = Paragraph::new(keybinds);
        frame.render_widget(keybinds_para, inner_chunks[1]);
    }

    fn render_selected_details(
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

        let params = &state.data().parameters;
        let current_age = self.calculate_age(state);
        let end_age = self.calculate_end_age(state);

        let age_str = match (current_age, end_age) {
            (Some(start), Some(end)) => {
                format!("{} → {} ({} years)", start, end, params.duration_years)
            }
            _ => format!("{} years", params.duration_years),
        };

        let mc_status = if let Some(mc) = &state.monte_carlo_result {
            format!(
                "{} runs ({:.0}% success)",
                mc.stats.num_iterations,
                mc.stats.success_rate * 100.0
            )
        } else {
            "Not run (press [m])".to_string()
        };

        let num_accounts = state.data().portfolios.accounts.len();
        let num_events = state.data().events.len();
        let net_worth = self.calculate_net_worth(state);

        let scenario_name = if state.is_current_dirty() {
            format!("{}*", state.current_scenario)
        } else {
            state.current_scenario.clone()
        };

        let lines = vec![
            Line::from(vec![
                Span::styled("Scenario: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    &scenario_name,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Start:    ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&params.start_date),
            ]),
            Line::from(vec![
                Span::styled("Age:      ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(age_str),
            ]),
            Line::from(vec![
                Span::styled("Monte:    ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(mc_status),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw(format!(
                    "Accounts: {}  |  Events: {}  |  ",
                    num_accounts, num_events
                )),
                Span::styled("NW: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    format_currency(net_worth),
                    Style::default().fg(if net_worth >= 0.0 {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
            ]),
        ];

        let title = if focused {
            " SELECTED SCENARIO "
        } else {
            " SELECTED "
        };

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        );

        frame.render_widget(paragraph, area);
    }

    fn render_comparison_table(
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

        let scenarios = state.get_scenario_list_with_summaries();

        // Header with percentiles integrated
        let mut lines = vec![
            Line::from(vec![Span::styled(
                format!(
                    "{:<12}  {:>5}   {:>11}  {:>11}  {:>11}",
                    "Scenario", "Succ", "P5", "P50", "P95"
                ),
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(Span::styled(
                "─".repeat(58),
                Style::default().fg(Color::DarkGray),
            )),
        ];

        // Scenario rows with all data combined
        for (name, summary) in &scenarios {
            let is_current = name == &state.current_scenario;

            // Truncate name to fit
            let display_name: String = if name.len() > 12 {
                name.chars().take(11).chain(std::iter::once('…')).collect()
            } else {
                name.clone()
            };

            let (success, p5, p50, p95) = if let Some(s) = summary {
                let succ = s
                    .success_rate
                    .map(|r| format!("{:.0}%", r * 100.0))
                    .unwrap_or_else(|| "--".to_string());
                let (p5_val, p50_val, p95_val) = if let Some((p5, p50, p95)) = s.percentiles {
                    (
                        format_currency_short(p5),
                        format_currency_short(p50),
                        format_currency_short(p95),
                    )
                } else {
                    ("--".to_string(), "--".to_string(), "--".to_string())
                };
                (succ, p5_val, p50_val, p95_val)
            } else {
                (
                    "--".to_string(),
                    "--".to_string(),
                    "--".to_string(),
                    "--".to_string(),
                )
            };

            let style = if is_current {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            lines.push(Line::from(Span::styled(
                format!(
                    "{:<12}  {:>5}   {:>11}  {:>11}  {:>11}",
                    display_name, success, p5, p50, p95
                ),
                style,
            )));
        }

        let title = " COMPARISON ";

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        );

        frame.render_widget(paragraph, area);
    }

    fn render_overlay_chart(&self, frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = " NET WORTH OVERLAY ";

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let scenarios = state.get_scenario_list_with_summaries();

        // Collect all scenarios with yearly data
        let scenarios_with_data: Vec<_> = scenarios
            .iter()
            .filter_map(|(name, summary)| {
                summary.and_then(|s| {
                    s.yearly_net_worth
                        .as_ref()
                        .map(|data| (name.clone(), data.clone()))
                })
            })
            .collect();

        if scenarios_with_data.is_empty() {
            let help = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No simulation data available.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled(
                        "[R]",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" to run Monte Carlo on all scenarios."),
                ]),
            ])
            .alignment(Alignment::Center);
            frame.render_widget(help, inner);
            return;
        }

        // Define colors for different scenarios
        let colors = [
            Color::Green,
            Color::Cyan,
            Color::Magenta,
            Color::Yellow,
            Color::Blue,
            Color::Red,
        ];

        // Find the global max value and year range
        let mut min_year = i32::MAX;
        let mut max_year = i32::MIN;
        let mut max_value = 0.0f64;

        for (_, data) in &scenarios_with_data {
            for (year, value) in data {
                min_year = min_year.min(*year);
                max_year = max_year.max(*year);
                max_value = max_value.max(*value);
            }
        }

        if max_value <= 0.0 {
            max_value = 1.0; // Avoid division by zero
        }

        // Calculate inner dimensions
        let chart_width = inner.width.saturating_sub(2) as usize;
        let chart_height = inner.height.saturating_sub(2) as usize;

        if chart_width < 10 || chart_height < 3 {
            return;
        }

        // Build a simple ASCII chart
        // X axis: years, Y axis: net worth
        let _year_range = (max_year - min_year + 1) as usize;

        // Create legend at top
        let mut legend_spans: Vec<Span> = Vec::new();
        for (i, (name, _)) in scenarios_with_data.iter().enumerate() {
            let color = colors[i % colors.len()];
            if i > 0 {
                legend_spans.push(Span::raw("  "));
            }
            legend_spans.push(Span::styled("━", Style::default().fg(color)));
            legend_spans.push(Span::styled(
                format!(" {}", name),
                Style::default().fg(color),
            ));
        }
        let legend = Paragraph::new(Line::from(legend_spans));

        // Split inner for legend + chart
        let chart_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);

        frame.render_widget(legend, chart_chunks[0]);

        // Create a bar chart showing the final values for each scenario
        // (A full overlay line chart would require custom canvas rendering, so we use a bar chart comparison)
        let bars: Vec<Bar> = scenarios_with_data
            .iter()
            .enumerate()
            .map(|(i, (name, data))| {
                let final_value = data.last().map(|(_, v)| *v).unwrap_or(0.0);
                let color = colors[i % colors.len()];

                // Scale to percentage of max
                let scaled = if max_value > 0.0 {
                    ((final_value / max_value) * 100.0).max(0.0) as u64
                } else {
                    0
                };

                // Truncate name for label
                let short_name: String = name.chars().take(8).collect();

                Bar::default()
                    .value(scaled)
                    .label(Line::from(short_name))
                    .text_value(format_currency_short(final_value))
                    .style(Style::default().fg(color))
                    .value_style(Style::default().fg(color).reversed())
            })
            .collect();

        let bar_width = (chart_chunks[1].width as usize / bars.len().max(1)).clamp(3, 10) as u16;

        let chart = BarChart::default()
            .data(BarGroup::default().bars(&bars))
            .bar_width(bar_width)
            .bar_gap(1)
            .direction(Direction::Vertical);

        frame.render_widget(chart, chart_chunks[1]);
    }
}

impl Screen for ScenarioScreen {
    fn title(&self) -> &str {
        "Scenario"
    }
}
