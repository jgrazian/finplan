use crate::components::{Component, EventResult};
use crate::data::portfolio_data::AccountType;
use crate::state::{
    AppState, FieldType, FormField, FormModal, MessageModal, ModalAction, ModalState,
    ScenarioPickerModal, TabId,
};
use crate::util::format::format_currency;
use crossterm::event::{KeyCode, KeyEvent};
use jiff::civil::Date;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph},
};

use super::Screen;

pub struct ScenarioScreen;

impl ScenarioScreen {
    pub fn new() -> Self {
        Self
    }

    fn parse_date(date_str: &str) -> Option<Date> {
        // Parse YYYY-MM-DD format using jiff
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
}

impl Component for ScenarioScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        match key.code {
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
            KeyCode::Char('p') => {
                // Run projection preview
                if let Err(e) = state.run_projection_preview() {
                    state.set_error(format!("Projection failed: {}", e));
                }
                EventResult::Handled
            }
            KeyCode::Char('m') => {
                match state.run_monte_carlo(1000) {
                    Ok(()) => {
                        state.switch_tab(TabId::Results);
                        state.results_state.scroll_offset = 0;
                    }
                    Err(e) => state.set_error(format!("Monte Carlo simulation failed: {}", e)),
                }
                EventResult::Handled
            }
            KeyCode::Char('s') => {
                // Open scenario picker for save (allows saving to existing or new scenario)
                let scenarios = state.scenario_names();
                state.modal = ModalState::ScenarioPicker(ScenarioPickerModal::new(
                    "Save Scenario As",
                    scenarios,
                    ModalAction::SAVE_AS,
                ));
                EventResult::Handled
            }
            KeyCode::Char('l') => {
                // Open scenario picker for load
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
            KeyCode::Char('e') => {
                // Edit simulation parameters
                let params = &state.data().parameters;

                // Get today's date as default for start_date if it's empty
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
            _ => EventResult::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        // New layout: 2 columns at top + summary bar at bottom
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(12),   // Top section (2 columns)
                Constraint::Length(4), // Summary bar
            ])
            .split(area);

        // Top section: 2 columns (50/50)
        let top_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[0]);

        // Left column: Parameters + Quick Actions (stacked)
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7), // Parameters (compact)
                Constraint::Min(5),    // Quick Actions
            ])
            .split(top_columns[0]);

        self.render_parameters(frame, left_chunks[0], state);
        self.render_quick_actions(frame, left_chunks[1], state);

        // Right column: Projection Preview
        self.render_projection_preview(frame, top_columns[1], state);

        // Bottom: Summary bar
        self.render_summary(frame, main_chunks[1], state);
    }
}

impl ScenarioScreen {
    fn render_parameters(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let params = &state.data().parameters;

        let current_age = self.calculate_age(state);
        let end_age = self.calculate_end_age(state);
        let age_str = match (current_age, end_age) {
            (Some(start), Some(end)) => {
                format!("{} -> {} ({} years)", start, end, params.duration_years)
            }
            _ => format!("{} years", params.duration_years),
        };

        // Check if Monte Carlo results exist
        let monte_carlo_status = if let Some(mc_result) = &state.monte_carlo_result {
            Line::from(vec![
                Span::styled("Monte: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{} runs", mc_result.stats.num_iterations),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!(" ({:.0}% success)", mc_result.stats.success_rate * 100.0),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled("Monte: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled("Not run", Style::default().fg(Color::DarkGray)),
                Span::raw(" (press [m])"),
            ])
        };

        let lines = vec![
            Line::from(vec![
                Span::styled("Start: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&params.start_date),
            ]),
            Line::from(vec![
                Span::styled("Age:   ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(age_str),
            ]),
            monte_carlo_status,
        ];

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" SIMULATION PARAMETERS "),
        );

        frame.render_widget(paragraph, area);
    }

    fn render_quick_actions(&self, frame: &mut Frame, area: Rect, _state: &AppState) {
        let lines = vec![
            Line::from(vec![
                Span::styled(
                    "[r]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Run  "),
                Span::styled(
                    "[p]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Preview  "),
                Span::styled(
                    "[m]",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Monte"),
            ]),
            Line::from(vec![
                Span::styled(
                    "[s]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Save  "),
                Span::styled(
                    "[l]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Load  "),
                Span::styled(
                    "[e]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Edit Params"),
            ]),
        ];

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" QUICK ACTIONS "),
        );

        frame.render_widget(paragraph, area);
    }

    fn render_projection_preview(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        if let Some(preview) = &state.scenario_state.projection_preview {
            // Split area: top for text, bottom for bar chart
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(8),    // Text section
                    Constraint::Length(6), // Bar chart
                ])
                .split(area);

            // Build text content
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(
                        "Final Net Worth: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format_currency(preview.final_net_worth),
                        Style::default().fg(if preview.final_net_worth >= 0.0 {
                            Color::Green
                        } else {
                            Color::Red
                        }),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Total Income:    ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format_currency(preview.total_income)),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Total Expenses:  ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format_currency(preview.total_expenses)),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Total Taxes:     ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format_currency(preview.total_taxes)),
                ]),
            ];

            // Add Monte Carlo summary if available
            if let Some(mc) = &preview.mc_summary {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(
                        "Monte Carlo: ",
                        Style::default()
                            .add_modifier(Modifier::BOLD)
                            .fg(Color::Magenta),
                    ),
                    Span::styled(
                        format!("{:.0}% success", mc.success_rate * 100.0),
                        Style::default().fg(if mc.success_rate >= 0.9 {
                            Color::Green
                        } else if mc.success_rate >= 0.7 {
                            Color::Yellow
                        } else {
                            Color::Red
                        }),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("  P5:  ", Style::default().fg(Color::DarkGray)),
                    Span::raw(format_currency(mc.p5_final)),
                    Span::styled("  P50: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(format_currency(mc.p50_final)),
                    Span::styled("  P95: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(format_currency(mc.p95_final)),
                ]));
            } else {
                // Show milestones if no MC summary
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Key Milestones:",
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::Cyan),
                )));
                for (year, desc) in preview.milestones.iter().take(3) {
                    lines.push(Line::from(format!("  {} - {}", year, desc)));
                }
            }

            let paragraph = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default())
                    .title(" PROJECTION PREVIEW "),
            );

            frame.render_widget(paragraph, chunks[0]);

            // Render bar chart of yearly net worth
            if !preview.yearly_net_worth.is_empty() {
                self.render_yearly_bar_chart(frame, chunks[1], &preview.yearly_net_worth);
            }
        } else {
            let lines = vec![
                Line::from(Span::styled(
                    "No projection data",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Press [p] to run preview",
                    Style::default().fg(Color::Cyan),
                )),
                Line::from(Span::styled(
                    "or [r] to run full simulation",
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            let paragraph = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" PROJECTION PREVIEW "),
            );

            frame.render_widget(paragraph, area);
        }
    }

    fn render_yearly_bar_chart(&self, frame: &mut Frame, area: Rect, yearly_data: &[(i32, f64)]) {
        let num_years = yearly_data.len();
        if num_years == 0 {
            return;
        }

        // Calculate how many bars we can fit
        let inner_width = area.width.saturating_sub(2) as usize;
        let bar_width = 1u16;
        let bar_gap = 0u16;
        let max_bars = inner_width / (bar_width as usize + bar_gap as usize).max(1);

        // Sample if needed to fit
        let step = if num_years > max_bars {
            (num_years as f64 / max_bars as f64).ceil() as usize
        } else {
            1
        };

        let max_value = yearly_data.iter().map(|(_, v)| *v).fold(0.0f64, f64::max);

        let bars: Vec<Bar> = yearly_data
            .iter()
            .step_by(step.max(1))
            .map(|(year, value)| {
                let scaled = if max_value > 0.0 {
                    ((value / max_value) * 100.0).max(0.0) as u64
                } else {
                    0
                };
                let style = if *value >= 0.0 {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                };

                Bar::default()
                    .value(scaled)
                    .label(Line::from(format!("{}", year % 100)))
                    .style(style)
            })
            .collect();

        let chart = BarChart::default()
            .block(Block::default().borders(Borders::TOP).title(" Net Worth "))
            .data(BarGroup::default().bars(&bars))
            .bar_width(bar_width)
            .bar_gap(bar_gap)
            .direction(Direction::Vertical);

        frame.render_widget(chart, area);
    }

    fn render_summary(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let num_accounts = state.data().portfolios.accounts.len();
        let num_events = state.data().events.len();
        let num_profiles = state.data().profiles.len();
        let net_worth = self.calculate_net_worth(state);
        let scenario_count = state.app_data.simulations.len();

        let lines = vec![Line::from(vec![
            Span::styled("SCENARIO: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                &state.current_scenario,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({} total)", scenario_count),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  |  "),
            Span::raw(format!(
                "{} Accounts  |  {} Events  |  {} Profiles  |  ",
                num_accounts, num_events, num_profiles
            )),
            Span::styled("Net Worth: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format_currency(net_worth)),
        ])];

        let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));

        frame.render_widget(paragraph, area);
    }
}

impl Screen for ScenarioScreen {
    fn title(&self) -> &str {
        "Scenario"
    }
}
