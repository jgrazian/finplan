use crate::components::{Component, EventResult};
use crate::data::portfolio_data::AccountType;
use crate::state::{AppState, MessageModal, ModalAction, ModalState, ScenarioPickerModal, TabId};
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
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(12), // Parameters
                Constraint::Length(10), // Quick actions
                Constraint::Min(6),     // Summary
            ])
            .split(area);

        self.render_parameters(frame, chunks[0], state);
        self.render_quick_actions(frame, chunks[1], state);
        self.render_summary(frame, chunks[2], state);
    }
}

impl ScenarioScreen {
    fn render_parameters(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let params = &state.data().parameters;

        let current_age_str = self
            .calculate_age(state)
            .map(|a| format!("(Current Age: {})", a))
            .unwrap_or_default();

        let end_age_str = self
            .calculate_end_age(state)
            .map(|a| format!("(End Age: {})", a))
            .unwrap_or_default();

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  Start Date:      ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(&params.start_date),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  Birth Date:      ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("{} {}", params.birth_date, current_age_str)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  Duration:        ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("{} years {}", params.duration_years, end_age_str)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  Monte Carlo:     ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw("Disabled        Iterations: 1000"),
            ]),
            Line::from(""),
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
            Line::from(Span::styled(
                "QUICK ACTIONS",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  [r] Run Single Simulation",
                Style::default().fg(Color::Green),
            )),
            Line::from(Span::styled(
                "  [m] Run Monte Carlo",
                Style::default().fg(Color::Green),
            )),
            Line::from(""),
            Line::from("  [s] Save Scenario"),
            Line::from("  [l] Load Scenario"),
        ];

        let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));

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
                    format!("  ({} total)", scenario_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
            Line::from(""),
            Line::from(format!(
                "  Accounts: {}  |  Events: {}  |  Return Profiles: {}",
                num_accounts, num_events, num_profiles
            )),
            Line::from(""),
            Line::from(format!(
                "  Est. Net Worth at Start: {}",
                crate::util::format::format_currency(net_worth)
            )),
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
