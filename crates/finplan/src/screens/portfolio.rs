use crate::components::{Component, EventResult};
use crate::data::portfolio_data::{AccountData, AccountType};
use crate::state::{AppState, FocusedPanel};
use crate::util::format::format_currency;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::Screen;

pub struct PortfolioScreen;

impl PortfolioScreen {
    pub fn new() -> Self {
        Self
    }

    fn render_account_list(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.portfolio_state.focused_panel == FocusedPanel::Left;

        let items: Vec<ListItem> = state
            .data()
            .portfolios
            .accounts
            .iter()
            .enumerate()
            .map(|(idx, account)| {
                let value = get_account_value(account);
                let content = format!("{:<20} {:>12}", account.name, format_currency(value));

                let style = if idx == state.portfolio_state.selected_account_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(Span::styled(content, style)))
            })
            .collect();

        let title = if is_focused {
            " ACCOUNTS [FOCUSED] "
        } else {
            " ACCOUNTS "
        };

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style),
        );

        frame.render_widget(list, area);
    }

    fn render_account_details(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.portfolio_state.focused_panel == FocusedPanel::Right;

        let title = if is_focused {
            " ACCOUNT DETAILS [FOCUSED] "
        } else {
            " ACCOUNT DETAILS "
        };

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let content = if let Some(account) = state
            .data()
            .portfolios
            .accounts
            .get(state.portfolio_state.selected_account_index)
        {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&account.name),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format_account_type(&account.account_type)),
                ]),
                Line::from(""),
            ];

            if let Some(desc) = &account.description {
                lines.push(Line::from(vec![
                    Span::styled(
                        "Description: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(desc),
                ]));
                lines.push(Line::from(""));
            }

            match &account.account_type {
                AccountType::Checking(prop)
                | AccountType::Savings(prop)
                | AccountType::HSA(prop)
                | AccountType::Property(prop)
                | AccountType::Collectible(prop) => {
                    lines.push(Line::from(vec![
                        Span::styled("Value: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(format_currency(prop.value)),
                    ]));
                    if let Some(profile) = &prop.return_profile {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "Return Profile: ",
                                Style::default().add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(&profile.0),
                        ]));
                    }
                }
                AccountType::Brokerage(inv)
                | AccountType::Traditional401k(inv)
                | AccountType::Roth401k(inv)
                | AccountType::TraditionalIRA(inv)
                | AccountType::RothIRA(inv) => {
                    lines.push(Line::from(Span::styled(
                        "Holdings:",
                        Style::default().add_modifier(Modifier::BOLD),
                    )));
                    if inv.assets.is_empty() {
                        lines.push(Line::from("  (none)"));
                    } else {
                        for asset_val in &inv.assets {
                            lines.push(Line::from(format!(
                                "  {}: {}",
                                asset_val.asset.0,
                                format_currency(asset_val.value)
                            )));
                        }
                    }
                }
                AccountType::Mortgage(debt)
                | AccountType::LoanDebt(debt)
                | AccountType::StudentLoanDebt(debt) => {
                    lines.push(Line::from(vec![
                        Span::styled("Balance: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(format_currency(debt.balance)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled(
                            "Interest Rate: ",
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(format!("{:.2}%", debt.interest_rate * 100.0)),
                    ]));
                }
            }

            lines
        } else {
            vec![Line::from("No account selected")]
        };

        let paragraph = Paragraph::new(content).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style),
        );

        frame.render_widget(paragraph, area);
    }
}

fn get_account_value(account: &AccountData) -> f64 {
    match &account.account_type {
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
    }
}

fn format_account_type(account_type: &AccountType) -> &'static str {
    match account_type {
        AccountType::Brokerage(_) => "Brokerage",
        AccountType::Traditional401k(_) => "401(k)",
        AccountType::Roth401k(_) => "Roth 401(k)",
        AccountType::TraditionalIRA(_) => "Traditional IRA",
        AccountType::RothIRA(_) => "Roth IRA",
        AccountType::Checking(_) => "Checking",
        AccountType::Savings(_) => "Savings",
        AccountType::HSA(_) => "HSA",
        AccountType::Property(_) => "Property",
        AccountType::Collectible(_) => "Collectible",
        AccountType::Mortgage(_) => "Mortgage",
        AccountType::LoanDebt(_) => "Loan",
        AccountType::StudentLoanDebt(_) => "Student Loan",
    }
}

impl Component for PortfolioScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let accounts = &state.data().portfolios.accounts;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if state.portfolio_state.focused_panel == FocusedPanel::Left && !accounts.is_empty()
                {
                    state.portfolio_state.selected_account_index =
                        (state.portfolio_state.selected_account_index + 1) % accounts.len();
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if state.portfolio_state.focused_panel == FocusedPanel::Left && !accounts.is_empty()
                {
                    if state.portfolio_state.selected_account_index == 0 {
                        state.portfolio_state.selected_account_index = accounts.len() - 1;
                    } else {
                        state.portfolio_state.selected_account_index -= 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Tab if key.modifiers.is_empty() => {
                state.portfolio_state.focused_panel = match state.portfolio_state.focused_panel {
                    FocusedPanel::Left => FocusedPanel::Right,
                    FocusedPanel::Right => FocusedPanel::Left,
                };
                EventResult::Handled
            }
            KeyCode::Char('a') => {
                // TODO: Open add account modal
                state.set_error("Add account not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('d') => {
                // TODO: Delete selected account
                state.set_error("Delete account not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('e') => {
                // TODO: Open edit account modal
                state.set_error("Edit account not yet implemented".to_string());
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        self.render_account_list(frame, chunks[0], state);
        self.render_account_details(frame, chunks[1], state);
    }
}

impl Screen for PortfolioScreen {
    fn title(&self) -> &str {
        "Portfolio"
    }
}
