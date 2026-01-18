use crate::components::{Component, EventResult};
use crate::data::parameters_data::{FederalBracketsPreset, InflationData};
use crate::data::profiles_data::ReturnProfileData;
use crate::state::{AppState, FocusedPanel};
use crate::util::format::format_percentage;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::Screen;

pub struct ProfilesScreen;

impl ProfilesScreen {
    pub fn new() -> Self {
        Self
    }

    fn render_profile_list(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.profiles_state.focused_panel == FocusedPanel::Left;

        let items: Vec<ListItem> = state
            .data()
            .profiles
            .iter()
            .enumerate()
            .map(|(idx, profile_data)| {
                let profile_desc = Self::format_profile(&profile_data.profile);
                let content = format!("{}: {}", profile_data.name.0, profile_desc);

                let style = if idx == state.profiles_state.selected_return_profile_index {
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
            " RETURN PROFILES [FOCUSED] "
        } else {
            " RETURN PROFILES "
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

    fn render_details(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.profiles_state.focused_panel == FocusedPanel::Right;

        // Split the right panel into sections
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),      // Profile details
                Constraint::Length(10),  // Presets
                Constraint::Length(8),   // Inflation
                Constraint::Min(6),      // Tax config
            ])
            .split(area);

        self.render_profile_details(frame, chunks[0], state, is_focused);
        self.render_presets(frame, chunks[1], state);
        self.render_inflation(frame, chunks[2], state);
        self.render_tax_config(frame, chunks[3], state);
    }

    fn render_profile_details(&self, frame: &mut Frame, area: Rect, state: &AppState, is_focused: bool) {
        let title = if is_focused {
            " PROFILE DETAILS [FOCUSED] "
        } else {
            " PROFILE DETAILS "
        };

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let content = if let Some(profile_data) = state
            .data()
            .profiles
            .get(state.profiles_state.selected_return_profile_index)
        {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&profile_data.name.0),
                ]),
                Line::from(""),
            ];

            if let Some(desc) = &profile_data.description {
                lines.push(Line::from(vec![
                    Span::styled("Description: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(desc),
                ]));
                lines.push(Line::from(""));
            }

            lines.push(Line::from(vec![
                Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(Self::format_profile_type(&profile_data.profile)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Parameters:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Self::format_profile_params(&profile_data.profile)));

            lines
        } else {
            vec![Line::from("No profile selected")]
        };

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(border_style),
            )
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    fn render_presets(&self, frame: &mut Frame, area: Rect, _state: &AppState) {
        let lines = vec![
            Line::from(Span::styled(
                "PRESETS",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  [1] S&P 500 Historical (Fixed)"),
            Line::from("  [2] S&P 500 Historical (Normal)"),
            Line::from("  [3] S&P 500 Historical (Log-Normal)"),
            Line::from("  [4] None (0% return)"),
            Line::from(""),
            Line::from(Span::styled(
                "(Press 1-4 to apply preset)",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));

        frame.render_widget(paragraph, area);
    }

    fn render_inflation(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let inflation_desc = match &state.data().parameters.inflation {
            InflationData::None => "None (0%)".to_string(),
            InflationData::Fixed { rate } => format!("Fixed: {}", format_percentage(*rate)),
            InflationData::Normal { mean, std_dev } => {
                format!(
                    "Normal: μ={}, σ={}",
                    format_percentage(*mean),
                    format_percentage(*std_dev)
                )
            }
            InflationData::LogNormal { mean, std_dev } => {
                format!(
                    "Log-Normal: μ={}, σ={}",
                    format_percentage(*mean),
                    format_percentage(*std_dev)
                )
            }
            InflationData::USHistorical { distribution } => {
                format!("US Historical ({:?})", distribution)
            }
        };

        let lines = vec![
            Line::from(vec![
                Span::styled(
                    "INFLATION PROFILE",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(inflation_desc),
            Line::from(""),
            Line::from(Span::styled(
                "[i] Edit inflation",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));

        frame.render_widget(paragraph, area);
    }

    fn render_tax_config(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let tax_config = &state.data().parameters.tax_config;
        let federal_desc = match &tax_config.federal_brackets {
            FederalBracketsPreset::Single2024 => "2024 Single".to_string(),
            FederalBracketsPreset::MarriedJoint2024 => "2024 Married Joint".to_string(),
            FederalBracketsPreset::Custom { brackets } => {
                format!("{} custom brackets", brackets.len())
            }
        };

        let lines = vec![
            Line::from(Span::styled(
                "TAX CONFIGURATION",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(format!("  Federal Brackets: {}", federal_desc)),
            Line::from(format!(
                "  State Rate: {}",
                format_percentage(tax_config.state_rate)
            )),
            Line::from(format!(
                "  Capital Gains Rate: {}",
                format_percentage(tax_config.capital_gains_rate)
            )),
            Line::from(""),
            Line::from(Span::styled(
                "[t] Edit tax config",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" TAX CONFIG "),
        );

        frame.render_widget(paragraph, area);
    }

    fn format_profile(profile: &ReturnProfileData) -> String {
        match profile {
            ReturnProfileData::None => "None".to_string(),
            ReturnProfileData::Fixed { rate } => format!("Fixed {}", format_percentage(*rate)),
            ReturnProfileData::Normal { mean, .. } => {
                format!("Normal {}", format_percentage(*mean))
            }
            ReturnProfileData::LogNormal { mean, .. } => {
                format!("LogNormal {}", format_percentage(*mean))
            }
        }
    }

    fn format_profile_type(profile: &ReturnProfileData) -> String {
        match profile {
            ReturnProfileData::None => "None".to_string(),
            ReturnProfileData::Fixed { .. } => "Fixed Rate".to_string(),
            ReturnProfileData::Normal { .. } => "Normal Distribution".to_string(),
            ReturnProfileData::LogNormal { .. } => "Log-Normal Distribution".to_string(),
        }
    }

    fn format_profile_params(profile: &ReturnProfileData) -> String {
        match profile {
            ReturnProfileData::None => "  Return: 0%".to_string(),
            ReturnProfileData::Fixed { rate } => format!("  Rate: {}", format_percentage(*rate)),
            ReturnProfileData::Normal { mean, std_dev } => {
                format!(
                    "  Mean: {}, Std Dev: {}",
                    format_percentage(*mean),
                    format_percentage(*std_dev)
                )
            }
            ReturnProfileData::LogNormal { mean, std_dev } => {
                format!(
                    "  Mean: {}, Std Dev: {}",
                    format_percentage(*mean),
                    format_percentage(*std_dev)
                )
            }
        }
    }
}

impl Component for ProfilesScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let profiles = &state.data().profiles;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if state.profiles_state.focused_panel == FocusedPanel::Left && !profiles.is_empty()
                {
                    state.profiles_state.selected_return_profile_index =
                        (state.profiles_state.selected_return_profile_index + 1) % profiles.len();
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if state.profiles_state.focused_panel == FocusedPanel::Left && !profiles.is_empty()
                {
                    if state.profiles_state.selected_return_profile_index == 0 {
                        state.profiles_state.selected_return_profile_index = profiles.len() - 1;
                    } else {
                        state.profiles_state.selected_return_profile_index -= 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Tab if key.modifiers.is_empty() => {
                state.profiles_state.focused_panel = match state.profiles_state.focused_panel {
                    FocusedPanel::Left => FocusedPanel::Right,
                    FocusedPanel::Right => FocusedPanel::Left,
                };
                EventResult::Handled
            }
            KeyCode::Char('a') => {
                state.set_error("Add return profile not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('e') => {
                state.set_error("Edit return profile not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('i') => {
                state.set_error("Edit inflation not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('t') => {
                state.set_error("Edit tax config not yet implemented".to_string());
                EventResult::Handled
            }
            // Preset shortcuts
            KeyCode::Char('1') if state.profiles_state.focused_panel == FocusedPanel::Right => {
                let idx = state.profiles_state.selected_return_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::Fixed { rate: 0.095668 };
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('2') if state.profiles_state.focused_panel == FocusedPanel::Right => {
                let idx = state.profiles_state.selected_return_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::Normal {
                        mean: 0.095668,
                        std_dev: 0.165244,
                    };
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('3') if state.profiles_state.focused_panel == FocusedPanel::Right => {
                let idx = state.profiles_state.selected_return_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::LogNormal {
                        mean: 0.095668,
                        std_dev: 0.165244,
                    };
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('4') if state.profiles_state.focused_panel == FocusedPanel::Right => {
                let idx = state.profiles_state.selected_return_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::None;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(area);

        self.render_profile_list(frame, chunks[0], state);
        self.render_details(frame, chunks[1], state);
    }
}

impl Screen for ProfilesScreen {
    fn title(&self) -> &str {
        "Profiles"
    }
}
