use crate::components::{Component, EventResult};
use crate::data::portfolio_data::AccountType;
use crate::state::{AppState, MessageModal, ModalAction, ModalState, ScenarioPickerModal, TabId};
use crate::util::format::format_currency;
use crossterm::event::{KeyCode, KeyEvent};
use jiff::civil::Date;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
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
                state.set_error("Monte Carlo simulation not yet implemented".to_string());
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
            _ => EventResult::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        // New layout: 2 columns at top + summary bar at bottom
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(12),    // Top section (2 columns)
                Constraint::Length(4),  // Summary bar
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
                Constraint::Length(7),  // Parameters (compact)
                Constraint::Min(5),     // Quick Actions
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
            (Some(start), Some(end)) => format!("{} -> {} ({} years)", start, end, params.duration_years),
            _ => format!("{} years", params.duration_years),
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
            Line::from(vec![
                Span::styled("Monte: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled("Disabled", Style::default().fg(Color::DarkGray)),
                Span::raw(" (1000 iterations)"),
            ]),
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
                Span::styled("[r]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Run  "),
                Span::styled("[p]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" Preview  "),
                Span::styled("[m]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Monte"),
            ]),
            Line::from(vec![
                Span::styled("[s]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" Save  "),
                Span::styled("[l]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" Load"),
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
        let lines = if let Some(preview) = &state.scenario_state.projection_preview {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Final Net Worth: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format_currency(preview.final_net_worth),
                        Style::default().fg(if preview.final_net_worth >= 0.0 { Color::Green } else { Color::Red }),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Total Income:    ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format_currency(preview.total_income)),
                ]),
                Line::from(vec![
                    Span::styled("Total Expenses:  ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format_currency(preview.total_expenses)),
                ]),
                Line::from(vec![
                    Span::styled("Total Taxes:     ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format_currency(preview.total_taxes)),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Key Milestones:",
                    Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                )),
            ];

            for (year, desc) in &preview.milestones {
                lines.push(Line::from(format!("  {} - {}", year, desc)));
            }

            lines
        } else {
            vec![
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
            ]
        };

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" PROJECTION PREVIEW "),
        );

        frame.render_widget(paragraph, area);
    }

    fn render_summary(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let num_accounts = state.data().portfolios.accounts.len();
        let num_events = state.data().events.len();
        let num_profiles = state.data().profiles.len();
        let net_worth = self.calculate_net_worth(state);
        let scenario_count = state.app_data.simulations.len();

        let lines = vec![
            Line::from(vec![
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
                Span::raw(format!("{} Accounts  |  {} Events  |  {} Profiles  |  ", num_accounts, num_events, num_profiles)),
                Span::styled("Net Worth: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format_currency(net_worth)),
            ]),
        ];

        let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));

        frame.render_widget(paragraph, area);
    }
}

impl Screen for ScenarioScreen {
    fn title(&self) -> &str {
        "Scenario"
    }
}
