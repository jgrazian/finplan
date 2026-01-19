use std::collections::HashSet;

use crate::components::portfolio_overview::{AccountBar, PortfolioOverviewChart};
use crate::components::{Component, EventResult};
use crate::data::parameters_data::{FederalBracketsPreset, InflationData};
use crate::data::portfolio_data::{AccountData, AccountType, AssetTag};
use crate::data::profiles_data::{ProfileData, ReturnProfileData};
use crate::state::context::{ConfigContext, ModalContext, TaxConfigContext};
use crate::state::{
    AppState, ConfirmModal, FormField, FormModal, ModalAction, ModalState, PickerModal,
    PortfolioProfilesPanel,
};
use crate::util::format::{format_currency, format_percentage};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::Screen;

pub struct PortfolioProfilesScreen;

impl PortfolioProfilesScreen {
    pub fn new() -> Self {
        Self
    }

    /// Extract all unique assets from investment accounts
    fn get_unique_assets(state: &AppState) -> Vec<AssetTag> {
        let mut assets = HashSet::new();
        for account in &state.data().portfolios.accounts {
            match &account.account_type {
                AccountType::Brokerage(inv)
                | AccountType::Traditional401k(inv)
                | AccountType::Roth401k(inv)
                | AccountType::TraditionalIRA(inv)
                | AccountType::RothIRA(inv) => {
                    for asset_val in &inv.assets {
                        assets.insert(asset_val.asset.clone());
                    }
                }
                _ => {}
            }
        }
        let mut sorted: Vec<_> = assets.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        sorted
    }

    // ========== Unified Panel Renderers ==========

    /// Render portfolio overview (always visible at top)
    fn render_portfolio_overview(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let accounts: Vec<AccountBar> = state
            .data()
            .portfolios
            .accounts
            .iter()
            .map(|account| {
                AccountBar::new(
                    account.name.clone(),
                    get_account_value(account),
                    Self::account_type_color(&account.account_type),
                )
            })
            .collect();

        PortfolioOverviewChart::new(&accounts).render(frame, area);
    }

    /// Render unified accounts panel (top: list | details, bottom: centered holdings chart)
    fn render_unified_accounts(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Accounts;

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(" ACCOUNTS ")
            .border_style(border_style);

        if is_focused {
            let help_text = if state.portfolio_profiles_state.editing_holdings {
                if state.portfolio_profiles_state.editing_holding_value
                    || state.portfolio_profiles_state.adding_new_holding
                {
                    " [Enter] Save  [Esc] Cancel "
                } else {
                    " [j/k] Nav [Shift+J/K] Reorder [Enter] Edit [d] Del [Esc] Exit "
                }
            } else {
                " [a]dd [e]dit [d]el [Shift+J/K] Reorder [Enter] Holdings "
            };
            block = block.title_bottom(Line::from(help_text).fg(Color::DarkGray));
        }

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        // Split vertically: ~50% top (list|details), ~50% bottom (chart)
        let top_height = (inner_area.height as f32 * 0.45).max(5.0) as u16;
        let bottom_height = inner_area.height.saturating_sub(top_height + 1); // 1 for separator

        let top_area = Rect::new(inner_area.x, inner_area.y, inner_area.width, top_height);
        let hsep_area = Rect::new(inner_area.x, inner_area.y + top_height, inner_area.width, 1);
        let bottom_area = Rect::new(
            inner_area.x,
            inner_area.y + top_height + 1,
            inner_area.width,
            bottom_height,
        );

        // Top section: split horizontally into list (40%) | details (60%)
        let list_width = (top_area.width as f32 * 0.40) as u16;
        let details_width = top_area.width.saturating_sub(list_width + 1); // 1 for separator

        let list_area = Rect::new(top_area.x, top_area.y, list_width, top_area.height);
        let vsep_area = Rect::new(top_area.x + list_width, top_area.y, 1, top_area.height);
        let details_area = Rect::new(
            top_area.x + list_width + 1,
            top_area.y,
            details_width,
            top_area.height,
        );

        // Render account list
        let accounts = &state.data().portfolios.accounts;
        let mut lines = Vec::new();
        for (idx, account) in accounts.iter().enumerate() {
            let value = get_account_value(account);
            let prefix = if idx == state.portfolio_profiles_state.selected_account_index {
                "> "
            } else {
                "  "
            };
            // Truncate name if needed to fit
            let max_name_len = list_width.saturating_sub(15) as usize;
            let name = if account.name.len() > max_name_len && max_name_len > 3 {
                format!("{}...", &account.name[..max_name_len.saturating_sub(3)])
            } else {
                account.name.clone()
            };
            let content = format!(
                "{}{:<width$} {:>10}",
                prefix,
                name,
                format_currency(value),
                width = max_name_len.max(1)
            );

            let style = if idx == state.portfolio_profiles_state.selected_account_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            lines.push(Line::from(Span::styled(content, style)));
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No accounts.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "  Press 'a' to add.",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let list_para = Paragraph::new(lines);
        frame.render_widget(list_para, list_area);

        // Render vertical separator
        let mut vsep_lines = Vec::new();
        for _ in 0..top_area.height {
            vsep_lines.push(Line::from(Span::styled(
                "│",
                Style::default().fg(Color::DarkGray),
            )));
        }
        let vsep = Paragraph::new(vsep_lines);
        frame.render_widget(vsep, vsep_area);

        // Render account details
        let detail_lines = if let Some(account) =
            accounts.get(state.portfolio_profiles_state.selected_account_index)
        {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&account.name),
                ]),
                Line::from(vec![
                    Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format_account_type(&account.account_type)),
                ]),
            ];

            if let Some(desc) = &account.description {
                lines.push(Line::from(vec![
                    Span::styled("Desc: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(desc),
                ]));
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
                                "Profile: ",
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
                    let total: f64 = inv.assets.iter().map(|a| a.value).sum();
                    lines.push(Line::from(vec![
                        Span::styled("Total: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(format_currency(total)),
                    ]));
                }
                AccountType::Mortgage(debt)
                | AccountType::LoanDebt(debt)
                | AccountType::StudentLoanDebt(debt) => {
                    lines.push(Line::from(vec![
                        Span::styled("Balance: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(format_currency(debt.balance)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Rate: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(format!("{:.2}%", debt.interest_rate * 100.0)),
                    ]));
                }
            }

            lines
        } else {
            vec![Line::from(Span::styled(
                "No account selected",
                Style::default().fg(Color::DarkGray),
            ))]
        };

        let details_para = Paragraph::new(detail_lines).wrap(Wrap { trim: true });
        frame.render_widget(details_para, details_area);

        // Render horizontal separator with "HOLDINGS" label
        let sep_width = inner_area.width as usize;
        let label = " HOLDINGS ";
        let label_len = label.len();
        let left_dashes = (sep_width.saturating_sub(label_len)) / 2;
        let right_dashes = sep_width.saturating_sub(label_len + left_dashes);
        let separator_text = format!(
            "{}{}{}",
            "─".repeat(left_dashes),
            label,
            "─".repeat(right_dashes)
        );
        let hsep = Paragraph::new(Line::from(Span::styled(
            separator_text,
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(hsep, hsep_area);

        // Bottom section: centered chart with ~20% padding on each side
        let padding = (bottom_area.width as f32 * 0.20) as u16;
        let chart_width = bottom_area.width.saturating_sub(padding * 2);
        let chart_area = Rect::new(
            bottom_area.x + padding,
            bottom_area.y,
            chart_width,
            bottom_area.height,
        );

        // Render asset allocation chart for selected account
        self.render_account_asset_chart(frame, chart_area, state);
    }

    /// Render asset allocation chart for the selected account
    fn render_account_asset_chart(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let account = match state
            .data()
            .portfolios
            .accounts
            .get(state.portfolio_profiles_state.selected_account_index)
        {
            Some(acc) => acc,
            None => {
                let msg = Paragraph::new("No account selected")
                    .style(Style::default().fg(Color::DarkGray));
                frame.render_widget(msg, area);
                return;
            }
        };

        // Check if it's an investment account
        let assets = match &account.account_type {
            AccountType::Brokerage(inv)
            | AccountType::Traditional401k(inv)
            | AccountType::Roth401k(inv)
            | AccountType::TraditionalIRA(inv)
            | AccountType::RothIRA(inv) => &inv.assets,
            AccountType::Checking(prop)
            | AccountType::Savings(prop)
            | AccountType::HSA(prop)
            | AccountType::Property(prop)
            | AccountType::Collectible(prop) => {
                let profile_str = prop
                    .return_profile
                    .as_ref()
                    .map(|p| format!("Profile: {}", p.0))
                    .unwrap_or_else(|| "No return profile".to_string());
                let lines = vec![
                    Line::from(Span::styled(
                        format!("Value: {}", format_currency(prop.value)),
                        Style::default().fg(Color::Cyan),
                    )),
                    Line::from(Span::styled(
                        profile_str,
                        Style::default().fg(Color::DarkGray),
                    )),
                ];
                let msg = Paragraph::new(lines);
                frame.render_widget(msg, area);
                return;
            }
            AccountType::Mortgage(debt)
            | AccountType::LoanDebt(debt)
            | AccountType::StudentLoanDebt(debt) => {
                let lines = vec![
                    Line::from(Span::styled(
                        format!("Balance: {}", format_currency(debt.balance)),
                        Style::default().fg(Color::Red),
                    )),
                    Line::from(Span::styled(
                        format!("Interest: {:.2}%", debt.interest_rate * 100.0),
                        Style::default().fg(Color::DarkGray),
                    )),
                ];
                let msg = Paragraph::new(lines);
                frame.render_widget(msg, area);
                return;
            }
        };

        let editing_mode = state.portfolio_profiles_state.editing_holdings;
        let selected_idx = state.portfolio_profiles_state.selected_holding_index;
        let editing_value = state.portfolio_profiles_state.editing_holding_value;
        let adding_new = state.portfolio_profiles_state.adding_new_holding;

        if assets.is_empty() && !editing_mode {
            let msg = Paragraph::new("No holdings. Press Enter to edit.")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
            return;
        }

        // Calculate total value
        let total_value: f64 = assets.iter().map(|a| a.value).sum();

        // Render horizontal bars for each asset
        let available_height = area.height as usize;
        let max_bars = available_height;

        let mut y_offset = 0;

        // Render existing assets
        for (idx, asset_val) in assets
            .iter()
            .enumerate()
            .take(max_bars.saturating_sub(if editing_mode { 1 } else { 0 }))
        {
            let is_selected = editing_mode && idx == selected_idx;
            let color = Self::asset_color_from_name(&asset_val.asset.0);

            // Truncate asset name if needed
            let max_name = if editing_mode { 8 } else { 10 };
            let name = if asset_val.asset.0.len() > max_name {
                format!("{}...", &asset_val.asset.0[..max_name - 3])
            } else {
                asset_val.asset.0.clone()
            };

            // Create bar line
            let prefix = if editing_mode {
                if is_selected { "> " } else { "  " }
            } else {
                ""
            };

            // Reserve space for: prefix(2) + name(9) + " XXX%"(5) + " $X,XXX,XXX.XX"(15) = 31
            let bar_width =
                area.width
                    .saturating_sub(if editing_mode { 36 } else { 34 }) as usize;
            let filled = if total_value > 0.0 {
                (bar_width as f64 * asset_val.value / total_value).round() as usize
            } else {
                0
            };
            let empty = bar_width.saturating_sub(filled);

            let bar_filled: String = "█".repeat(filled);
            let bar_empty: String = "░".repeat(empty);

            let percentage = if total_value > 0.0 {
                (asset_val.value / total_value * 100.0).round() as u16
            } else {
                0
            };

            // Determine value display
            let value_display = if is_selected && editing_value {
                format!(" ${}_", state.portfolio_profiles_state.holding_edit_buffer)
            } else {
                format!(" {}", format_currency(asset_val.value))
            };

            let name_style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };

            let bar_color = if is_selected { Color::Yellow } else { color };
            let value_style = if is_selected && editing_value {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let line = Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Yellow)),
                Span::styled(format!("{:<width$} ", name, width = max_name), name_style),
                Span::styled(bar_filled, Style::default().fg(bar_color)),
                Span::styled(bar_empty, Style::default().fg(Color::DarkGray)),
                Span::raw(format!(" {:>3}%", percentage)),
                Span::styled(value_display, value_style),
            ]);

            let bar_area = Rect::new(area.x, area.y + y_offset as u16, area.width, 1);
            frame.render_widget(Paragraph::new(line), bar_area);
            y_offset += 1;
        }

        // Render "Add new" option in editing mode
        if editing_mode && y_offset < max_bars {
            let is_add_selected = selected_idx == assets.len();

            let line = if adding_new {
                // Show name input buffer
                Line::from(vec![
                    Span::styled("> ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        format!(
                            "Name: {}_",
                            state.portfolio_profiles_state.new_holding_name_buffer
                        ),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ])
            } else {
                let prefix = if is_add_selected { "> " } else { "  " };
                let style = if is_add_selected {
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default().fg(if is_add_selected {
                            Color::Yellow
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::styled("+ Add new holding...", style),
                ])
            };

            let add_area = Rect::new(area.x, area.y + y_offset as u16, area.width, 1);
            frame.render_widget(Paragraph::new(line), add_area);
        }
    }

    /// Get a consistent color for an asset based on its name
    fn asset_color_from_name(name: &str) -> Color {
        let hash = name.bytes().fold(0u8, |acc, b| acc.wrapping_add(b));
        match hash % 6 {
            0 => Color::Cyan,
            1 => Color::Magenta,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            _ => Color::LightRed,
        }
    }

    /// Render unified profiles panel (top: list | details, bottom: centered distribution chart)
    fn render_unified_profiles(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Profiles;

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(" RETURN PROFILES ")
            .border_style(border_style);

        if is_focused {
            block = block.title_bottom(
                Line::from(" [a]dd [e]dit [d]el [Shift+J/K] Reorder [1-4] Preset ")
                    .fg(Color::DarkGray),
            );
        }

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        // Split vertically: ~45% top (list|details), ~55% bottom (chart)
        let top_height = (inner_area.height as f32 * 0.45).max(5.0) as u16;
        let bottom_height = inner_area.height.saturating_sub(top_height + 1); // 1 for separator

        let top_area = Rect::new(inner_area.x, inner_area.y, inner_area.width, top_height);
        let hsep_area = Rect::new(inner_area.x, inner_area.y + top_height, inner_area.width, 1);
        let bottom_area = Rect::new(
            inner_area.x,
            inner_area.y + top_height + 1,
            inner_area.width,
            bottom_height,
        );

        // Top section: split horizontally into list (40%) | details (60%)
        let list_width = (top_area.width as f32 * 0.40) as u16;
        let details_width = top_area.width.saturating_sub(list_width + 1); // 1 for separator

        let list_area = Rect::new(top_area.x, top_area.y, list_width, top_area.height);
        let vsep_area = Rect::new(top_area.x + list_width, top_area.y, 1, top_area.height);
        let details_area = Rect::new(
            top_area.x + list_width + 1,
            top_area.y,
            details_width,
            top_area.height,
        );

        // Render profile list
        let profiles = &state.data().profiles;
        let mut lines = Vec::new();
        for (idx, profile_data) in profiles.iter().enumerate() {
            let prefix = if idx == state.portfolio_profiles_state.selected_profile_index {
                "> "
            } else {
                "  "
            };
            // Truncate name if needed
            let max_name_len = list_width.saturating_sub(4) as usize;
            let name = if profile_data.name.0.len() > max_name_len && max_name_len > 3 {
                format!(
                    "{}...",
                    &profile_data.name.0[..max_name_len.saturating_sub(3)]
                )
            } else {
                profile_data.name.0.clone()
            };
            let content = format!("{}{}", prefix, name);

            let style = if idx == state.portfolio_profiles_state.selected_profile_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            lines.push(Line::from(Span::styled(content, style)));
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No profiles.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "  Press 'a' to add.",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let list_para = Paragraph::new(lines);
        frame.render_widget(list_para, list_area);

        // Render vertical separator
        let mut vsep_lines = Vec::new();
        for _ in 0..top_area.height {
            vsep_lines.push(Line::from(Span::styled(
                "│",
                Style::default().fg(Color::DarkGray),
            )));
        }
        let vsep = Paragraph::new(vsep_lines);
        frame.render_widget(vsep, vsep_area);

        // Render profile details
        let detail_lines = if let Some(profile_data) =
            profiles.get(state.portfolio_profiles_state.selected_profile_index)
        {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&profile_data.name.0),
                ]),
                Line::from(vec![
                    Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(Self::format_profile_type(&profile_data.profile)),
                ]),
            ];

            match &profile_data.profile {
                ReturnProfileData::None => {
                    lines.push(Line::from(vec![
                        Span::styled("Return: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw("0%"),
                    ]));
                }
                ReturnProfileData::Fixed { rate } => {
                    lines.push(Line::from(vec![
                        Span::styled("Rate: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format_percentage(*rate), Style::default().fg(Color::Cyan)),
                    ]));
                }
                ReturnProfileData::Normal { mean, std_dev }
                | ReturnProfileData::LogNormal { mean, std_dev } => {
                    lines.push(Line::from(vec![
                        Span::styled("Mean: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format_percentage(*mean), Style::default().fg(Color::Yellow)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Std Dev: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(format_percentage(*std_dev)),
                    ]));
                }
            }

            lines
        } else {
            vec![Line::from(Span::styled(
                "No profile selected",
                Style::default().fg(Color::DarkGray),
            ))]
        };

        let details_para = Paragraph::new(detail_lines).wrap(Wrap { trim: true });
        frame.render_widget(details_para, details_area);

        // Render horizontal separator with "DISTRIBUTION" label
        let sep_width = inner_area.width as usize;
        let label = " DISTRIBUTION ";
        let label_len = label.len();
        let left_dashes = (sep_width.saturating_sub(label_len)) / 2;
        let right_dashes = sep_width.saturating_sub(label_len + left_dashes);
        let separator_text = format!(
            "{}{}{}",
            "─".repeat(left_dashes),
            label,
            "─".repeat(right_dashes)
        );
        let hsep = Paragraph::new(Line::from(Span::styled(
            separator_text,
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(hsep, hsep_area);

        // Bottom section: centered chart with ~20% padding on each side
        let padding = (bottom_area.width as f32 * 0.20) as u16;
        let chart_width = bottom_area.width.saturating_sub(padding * 2);
        let chart_area = Rect::new(
            bottom_area.x + padding,
            bottom_area.y,
            chart_width,
            bottom_area.height,
        );

        // Render distribution chart for selected profile
        if let Some(profile_data) =
            profiles.get(state.portfolio_profiles_state.selected_profile_index)
        {
            self.render_distribution_inline(frame, chart_area, &profile_data.profile);
        } else {
            let msg =
                Paragraph::new("No profile selected").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, chart_area);
        }
    }

    /// Render distribution chart inline (without border)
    fn render_distribution_inline(
        &self,
        frame: &mut Frame,
        area: Rect,
        profile: &ReturnProfileData,
    ) {
        match profile {
            ReturnProfileData::None => {
                let msg =
                    Paragraph::new("No return (0%)").style(Style::default().fg(Color::DarkGray));
                frame.render_widget(msg, area);
            }
            ReturnProfileData::Fixed { rate } => {
                let lines = vec![
                    Line::from(vec![
                        Span::styled(
                            "Fixed Rate: ",
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format_percentage(*rate), Style::default().fg(Color::Cyan)),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled("━━━━━━━━━━━━━━━━━━━━", Style::default().fg(Color::Cyan)),
                        Span::styled(" ▲", Style::default().fg(Color::Yellow)),
                    ]),
                ];
                let paragraph = Paragraph::new(lines);
                frame.render_widget(paragraph, area);
            }
            ReturnProfileData::Normal { mean, std_dev } => {
                self.render_normal_distribution(frame, area, *mean, *std_dev, false);
            }
            ReturnProfileData::LogNormal { mean, std_dev } => {
                self.render_normal_distribution(frame, area, *mean, *std_dev, true);
            }
        }
    }

    /// Render secondary panels (Asset Mappings and Tax & Inflation) at the bottom
    fn render_secondary_panels(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let mappings_collapsed = state.portfolio_profiles_state.mappings_collapsed;
        let config_collapsed = state.portfolio_profiles_state.config_collapsed;

        // Horizontal layout for secondary panels
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Render Asset Mappings (collapsed or expanded)
        if mappings_collapsed {
            self.render_mappings_collapsed(frame, cols[0], state);
        } else {
            self.render_asset_mappings(frame, cols[0], state);
        }

        // Render Tax & Inflation Config (collapsed or expanded)
        if config_collapsed {
            self.render_config_collapsed(frame, cols[1], state);
        } else {
            self.render_tax_inflation_config(frame, cols[1], state);
        }
    }

    /// Render collapsed asset mappings summary
    fn render_mappings_collapsed(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::AssetMappings;

        let unique_assets = Self::get_unique_assets(state);
        let mappings = &state.data().assets;

        // Build inline summary: "VFIAX->S&P 500, VTSAX->S&P 500"
        let mut summary_parts: Vec<String> = Vec::new();
        for asset in unique_assets.iter().take(3) {
            let mapping = mappings.get(asset);
            let mapping_str = mapping.map(|p| p.0.as_str()).unwrap_or("?");
            summary_parts.push(format!("{}->{}", asset.0, mapping_str));
        }
        if unique_assets.len() > 3 {
            summary_parts.push(format!("+{}", unique_assets.len() - 3));
        }
        let summary = if summary_parts.is_empty() {
            "No assets".to_string()
        } else {
            summary_parts.join(", ")
        };

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = format!(" [+] ASSET MAPPINGS  {} ", summary);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);

        frame.render_widget(block, area);
    }

    /// Render collapsed tax & inflation config summary
    fn render_config_collapsed(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Config;

        let tax_config = &state.data().parameters.tax_config;
        let federal_short = match &tax_config.federal_brackets {
            FederalBracketsPreset::Single2024 => "2024 Single",
            FederalBracketsPreset::MarriedJoint2024 => "2024 MJ",
            FederalBracketsPreset::Custom { .. } => "Custom",
        };

        let inflation_short = match &state.data().parameters.inflation {
            InflationData::None => "None",
            InflationData::Fixed { .. } => "Fixed",
            InflationData::Normal { .. } => "Normal",
            InflationData::LogNormal { .. } => "LogN",
            InflationData::USHistorical { .. } => "US Hist",
        };

        let state_str = format_percentage(tax_config.state_rate);
        let summary = format!(
            "Federal: {} | State: {} | Inf: {}",
            federal_short, state_str, inflation_short
        );

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = format!(" [+] TAX & INFLATION  {} ", summary);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);

        frame.render_widget(block, area);
    }

    /// Render expanded asset mappings panel
    fn render_asset_mappings(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::AssetMappings;

        let unique_assets = Self::get_unique_assets(state);
        let mappings = &state.data().assets;

        let items: Vec<ListItem> = unique_assets
            .iter()
            .enumerate()
            .map(|(idx, asset)| {
                let mapping = mappings.get(asset);
                let mapping_str = mapping.map(|p| p.0.as_str()).unwrap_or("(unmapped)");

                let style = if idx == state.portfolio_profiles_state.selected_mapping_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if mapping.is_none() {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default()
                };

                let content = format!("{} -> {}", asset.0, mapping_str);
                ListItem::new(Line::from(Span::styled(content, style)))
            })
            .collect();

        let title = " [-] ASSET MAPPINGS ";

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);

        if is_focused && !unique_assets.is_empty() {
            block =
                block.title_bottom(Line::from(" [m] Map  [Space] Collapse ").fg(Color::DarkGray));
        }

        let list = List::new(items).block(block);

        frame.render_widget(list, area);
    }

    /// Render expanded tax & inflation config panel
    fn render_tax_inflation_config(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Config;

        let selected_idx = state.portfolio_profiles_state.selected_config_index;

        let tax_config = &state.data().parameters.tax_config;
        let federal_desc = match &tax_config.federal_brackets {
            FederalBracketsPreset::Single2024 => "2024 Single".to_string(),
            FederalBracketsPreset::MarriedJoint2024 => "2024 Married Joint".to_string(),
            FederalBracketsPreset::Custom { brackets } => {
                if brackets.is_empty() {
                    "Custom (empty)".to_string()
                } else {
                    "Custom".to_string()
                }
            }
        };

        let inflation_desc = match &state.data().parameters.inflation {
            InflationData::None => "None (0%)".to_string(),
            InflationData::Fixed { rate } => format!("Fixed {}", format_percentage(*rate)),
            InflationData::Normal { mean, .. } => format!("Normal μ={}", format_percentage(*mean)),
            InflationData::LogNormal { mean, .. } => format!("LogN μ={}", format_percentage(*mean)),
            InflationData::USHistorical { distribution } => {
                format!("US Historical ({:?})", distribution)
            }
        };

        let state_rate_str = format_percentage(tax_config.state_rate);
        let cap_gains_str = format_percentage(tax_config.capital_gains_rate);

        // Helper to create styled config lines
        fn style_config_line<'a>(
            is_focused: bool,
            selected_idx: usize,
            idx: usize,
            label: &'a str,
            value: String,
        ) -> Line<'a> {
            let is_selected = is_focused && selected_idx == idx;
            let label_style = if is_selected {
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Yellow)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            };
            let value_style = if is_selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            let prefix = if is_selected { "> " } else { "  " };
            Line::from(vec![
                Span::raw(prefix),
                Span::styled(label, label_style),
                Span::styled(value, value_style),
            ])
        }

        let lines = vec![
            style_config_line(is_focused, selected_idx, 0, "Federal: ", federal_desc),
            style_config_line(is_focused, selected_idx, 1, "State: ", state_rate_str),
            style_config_line(is_focused, selected_idx, 2, "Cap Gains: ", cap_gains_str),
            style_config_line(is_focused, selected_idx, 3, "Inflation: ", inflation_desc),
        ];

        let title = " [-] TAX & INFLATION ";

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);

        if is_focused {
            block =
                block.title_bottom(Line::from(" [e] Edit  [Space] Collapse ").fg(Color::DarkGray));
        }

        let paragraph = Paragraph::new(lines).block(block);

        frame.render_widget(paragraph, area);
    }

    // ========== Formatters ==========

    fn format_profile_type(profile: &ReturnProfileData) -> String {
        match profile {
            ReturnProfileData::None => "None".to_string(),
            ReturnProfileData::Fixed { .. } => "Fixed Rate".to_string(),
            ReturnProfileData::Normal { .. } => "Normal Distribution".to_string(),
            ReturnProfileData::LogNormal { .. } => "Log-Normal Distribution".to_string(),
        }
    }

    // ========== Chart Rendering ==========

    /// Color based on account type
    fn account_type_color(account_type: &AccountType) -> Color {
        match account_type {
            // Investment types
            AccountType::Brokerage(_)
            | AccountType::Traditional401k(_)
            | AccountType::Roth401k(_)
            | AccountType::TraditionalIRA(_)
            | AccountType::RothIRA(_) => Color::Green,
            // Cash types
            AccountType::Checking(_) | AccountType::Savings(_) | AccountType::HSA(_) => Color::Cyan,
            // Property
            AccountType::Property(_) | AccountType::Collectible(_) => Color::Yellow,
            // Debt
            AccountType::Mortgage(_)
            | AccountType::LoanDebt(_)
            | AccountType::StudentLoanDebt(_) => Color::Red,
        }
    }

    /// Render a normal or lognormal distribution histogram
    fn render_normal_distribution(
        &self,
        frame: &mut Frame,
        area: Rect,
        mean: f64,
        std_dev: f64,
        is_lognormal: bool,
    ) {
        let bar_width = 2; // Fixed bar width of 2 characters
        let num_bins = ((area.width as usize).saturating_sub(4) / bar_width).clamp(10, 35);
        let height = area.height.saturating_sub(2) as usize; // Leave room for labels

        if height < 3 || area.width < 20 {
            let msg = Paragraph::new("Area too small").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
            return;
        }

        // Calculate bin boundaries
        let (min_val, max_val) = if is_lognormal {
            // For lognormal, use appropriate range
            let log_mean = (1.0 + mean).ln() - std_dev * std_dev / 2.0;
            let log_std = std_dev;
            let lower = (log_mean - 3.0 * log_std).exp() - 1.0;
            let upper = (log_mean + 3.0 * log_std).exp() - 1.0;
            (lower.max(-0.5), upper.min(1.0))
        } else {
            // For normal, use ±3σ range
            (mean - 3.0 * std_dev, mean + 3.0 * std_dev)
        };

        let bin_size = (max_val - min_val) / num_bins as f64;

        // Calculate PDF values for each bin
        let pi = std::f64::consts::PI;
        let mut pdf_values = Vec::with_capacity(num_bins);

        for i in 0..num_bins {
            let x = min_val + (i as f64 + 0.5) * bin_size;

            let pdf = if is_lognormal {
                // LogNormal PDF (convert return to growth factor)
                let growth = 1.0 + x;
                if growth > 0.0 {
                    let log_mean = (1.0 + mean).ln() - std_dev * std_dev / 2.0;
                    let log_x = growth.ln();
                    let exponent = -(log_x - log_mean).powi(2) / (2.0 * std_dev * std_dev);
                    (1.0 / (growth * std_dev * (2.0 * pi).sqrt())) * exponent.exp()
                } else {
                    0.0
                }
            } else {
                // Normal PDF
                let exponent = -(x - mean).powi(2) / (2.0 * std_dev * std_dev);
                (1.0 / (std_dev * (2.0 * pi).sqrt())) * exponent.exp()
            };

            pdf_values.push(pdf);
        }

        // Normalize to height
        let max_pdf = pdf_values.iter().cloned().fold(0.0_f64, f64::max);
        if max_pdf == 0.0 {
            let msg =
                Paragraph::new("Invalid distribution").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
            return;
        }

        let bar_heights: Vec<usize> = pdf_values
            .iter()
            .map(|&pdf| ((pdf / max_pdf) * height as f64).round() as usize)
            .collect();

        // Calculate centering offset
        let total_chart_width = num_bins * bar_width;
        let x_offset = (area.width as usize).saturating_sub(total_chart_width) / 2;

        // Render vertical bars from bottom up
        let bin_chars = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

        for row in 0..height {
            let y_level = height - 1 - row;
            let mut spans = Vec::new();

            // Add left padding
            if x_offset > 0 {
                spans.push(Span::raw(" ".repeat(x_offset)));
            }

            for (i, &bar_h) in bar_heights.iter().enumerate() {
                let x = min_val + (i as f64 + 0.5) * bin_size;

                // Determine color based on position relative to mean
                let color = if x < mean - std_dev {
                    Color::Red
                } else if x > mean + std_dev {
                    Color::Green
                } else {
                    Color::Yellow
                };

                let char_to_use = if bar_h > y_level {
                    "█"
                } else if bar_h == y_level && bar_h > 0 {
                    // Partial fill
                    let partial = ((pdf_values[i] / max_pdf) * height as f64).fract();
                    let idx = (partial * 7.0).round() as usize;
                    bin_chars[idx.min(7)]
                } else {
                    " "
                };

                let bar_str = char_to_use.repeat(bar_width);
                spans.push(Span::styled(bar_str, Style::default().fg(color)));
            }

            let line = Line::from(spans);
            let row_area = Rect::new(area.x, area.y + row as u16, area.width, 1);
            frame.render_widget(Paragraph::new(line), row_area);
        }

        // Render x-axis labels
        let label_y = area.y + height as u16;
        let label_line = Line::from(vec![
            Span::styled(
                format!("{:>6}", format_percentage(min_val)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(" ".repeat((area.width as usize).saturating_sub(20) / 2)),
            Span::styled(
                format!("μ={}", format_percentage(mean)),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(" ".repeat((area.width as usize).saturating_sub(20) / 2)),
            Span::styled(
                format!("{:<6}", format_percentage(max_val)),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        let label_area = Rect::new(area.x, label_y, area.width, 1);
        frame.render_widget(Paragraph::new(label_line), label_area);
    }

    // ========== Key Handlers ==========

    fn handle_accounts_keys(&self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let accounts_len = state.data().portfolios.accounts.len();
        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            // Move down (Shift+J or Shift+Down)
            KeyCode::Char('J') if has_shift => {
                let idx = state.portfolio_profiles_state.selected_account_index;
                if accounts_len >= 2 && idx < accounts_len - 1 {
                    state.data_mut().portfolios.accounts.swap(idx, idx + 1);
                    state.portfolio_profiles_state.selected_account_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Down if has_shift => {
                let idx = state.portfolio_profiles_state.selected_account_index;
                if accounts_len >= 2 && idx < accounts_len - 1 {
                    state.data_mut().portfolios.accounts.swap(idx, idx + 1);
                    state.portfolio_profiles_state.selected_account_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            // Move up (Shift+K or Shift+Up)
            KeyCode::Char('K') if has_shift => {
                let idx = state.portfolio_profiles_state.selected_account_index;
                if accounts_len >= 2 && idx > 0 {
                    state.data_mut().portfolios.accounts.swap(idx, idx - 1);
                    state.portfolio_profiles_state.selected_account_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Up if has_shift => {
                let idx = state.portfolio_profiles_state.selected_account_index;
                if accounts_len >= 2 && idx > 0 {
                    state.data_mut().portfolios.accounts.swap(idx, idx - 1);
                    state.portfolio_profiles_state.selected_account_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let accounts = &state.data().portfolios.accounts;
                if !accounts.is_empty() {
                    state.portfolio_profiles_state.selected_account_index =
                        (state.portfolio_profiles_state.selected_account_index + 1)
                            % accounts.len();
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let accounts = &state.data().portfolios.accounts;
                if !accounts.is_empty() {
                    if state.portfolio_profiles_state.selected_account_index == 0 {
                        state.portfolio_profiles_state.selected_account_index = accounts.len() - 1;
                    } else {
                        state.portfolio_profiles_state.selected_account_index -= 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Enter => {
                // Enter holdings editing mode for investment accounts
                let accounts = &state.data().portfolios.accounts;
                if let Some(account) =
                    accounts.get(state.portfolio_profiles_state.selected_account_index)
                {
                    match &account.account_type {
                        AccountType::Brokerage(_)
                        | AccountType::Traditional401k(_)
                        | AccountType::Roth401k(_)
                        | AccountType::TraditionalIRA(_)
                        | AccountType::RothIRA(_) => {
                            state.portfolio_profiles_state.editing_holdings = true;
                            state.portfolio_profiles_state.selected_holding_index = 0;
                            state.portfolio_profiles_state.editing_holding_value = false;
                            state.portfolio_profiles_state.holding_edit_buffer.clear();
                            state.portfolio_profiles_state.adding_new_holding = false;
                            state
                                .portfolio_profiles_state
                                .new_holding_name_buffer
                                .clear();
                        }
                        _ => {
                            state.set_error(
                                "Only investment accounts have editable holdings".to_string(),
                            );
                        }
                    }
                }
                EventResult::Handled
            }
            KeyCode::Char('a') => {
                // Add new account - show category picker
                let categories = vec![
                    "Investment".to_string(),
                    "Cash".to_string(),
                    "Property".to_string(),
                    "Debt".to_string(),
                ];
                state.modal = ModalState::Picker(PickerModal::new(
                    "Select Account Category",
                    categories,
                    ModalAction::PICK_ACCOUNT_CATEGORY,
                ));
                EventResult::Handled
            }
            KeyCode::Char('e') => {
                // Edit selected account
                if let Some(account) = state
                    .data()
                    .portfolios
                    .accounts
                    .get(state.portfolio_profiles_state.selected_account_index)
                {
                    let form = Self::create_account_edit_form(account, state);
                    state.modal =
                        ModalState::Form(form.with_typed_context(ModalContext::account_index(
                            state.portfolio_profiles_state.selected_account_index,
                        )));
                }
                EventResult::Handled
            }
            KeyCode::Char('d') => {
                // Delete selected account with confirmation
                if let Some(account) = state
                    .data()
                    .portfolios
                    .accounts
                    .get(state.portfolio_profiles_state.selected_account_index)
                {
                    state.modal = ModalState::Confirm(
                        ConfirmModal::new(
                            "Delete Account",
                            &format!("Delete account '{}'?", account.name),
                            ModalAction::DELETE_ACCOUNT,
                        )
                        .with_typed_context(ModalContext::account_index(
                            state.portfolio_profiles_state.selected_account_index,
                        )),
                    );
                }
                EventResult::Handled
            }
            KeyCode::Char('h') => {
                // Manage holdings for investment accounts
                if let Some(account) = state
                    .data()
                    .portfolios
                    .accounts
                    .get(state.portfolio_profiles_state.selected_account_index)
                {
                    match &account.account_type {
                        AccountType::Brokerage(_)
                        | AccountType::Traditional401k(_)
                        | AccountType::Roth401k(_)
                        | AccountType::TraditionalIRA(_)
                        | AccountType::RothIRA(_) => {
                            // Show form to add a new holding
                            let form = FormModal::new(
                                "Add Holding",
                                vec![
                                    FormField::text("Asset Name", ""),
                                    FormField::currency("Value", 0.0),
                                ],
                                ModalAction::ADD_HOLDING,
                            )
                            .with_typed_context(
                                ModalContext::account_index(
                                    state.portfolio_profiles_state.selected_account_index,
                                ),
                            );
                            state.modal = ModalState::Form(form);
                        }
                        _ => {
                            state.set_error(
                                "Holdings are only available for investment accounts".to_string(),
                            );
                        }
                    }
                }
                EventResult::Handled
            }
            KeyCode::Char(' ') => {
                // Space: toggle both secondary panels and focus Asset Mappings
                let expanding = state.portfolio_profiles_state.mappings_collapsed;
                state.portfolio_profiles_state.mappings_collapsed = !expanding;
                state.portfolio_profiles_state.config_collapsed = !expanding;
                if expanding {
                    state.portfolio_profiles_state.focused_panel =
                        PortfolioProfilesPanel::AssetMappings;
                }
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    /// Handle key events when in holdings editing mode
    fn handle_holdings_keys(&self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let account_idx = state.portfolio_profiles_state.selected_account_index;
        let adding_new = state.portfolio_profiles_state.adding_new_holding;
        let editing_value = state.portfolio_profiles_state.editing_holding_value;

        // Get the number of assets for navigation bounds
        let num_assets = {
            let accounts = &state.data().portfolios.accounts;
            if let Some(account) = accounts.get(account_idx) {
                match &account.account_type {
                    AccountType::Brokerage(inv)
                    | AccountType::Traditional401k(inv)
                    | AccountType::Roth401k(inv)
                    | AccountType::TraditionalIRA(inv)
                    | AccountType::RothIRA(inv) => inv.assets.len(),
                    _ => 0,
                }
            } else {
                0
            }
        };

        // Ensure selected index is in bounds (e.g., after a deletion)
        let num_items = num_assets + 1; // +1 for "Add new" option
        if state.portfolio_profiles_state.selected_holding_index >= num_items && num_items > 0 {
            state.portfolio_profiles_state.selected_holding_index = num_items - 1;
        }

        // If we're typing a new holding name
        if adding_new {
            match key.code {
                KeyCode::Esc => {
                    state.portfolio_profiles_state.adding_new_holding = false;
                    state
                        .portfolio_profiles_state
                        .new_holding_name_buffer
                        .clear();
                    EventResult::Handled
                }
                KeyCode::Enter => {
                    // Finish adding the name, now we need to get a value
                    let name = state
                        .portfolio_profiles_state
                        .new_holding_name_buffer
                        .clone();
                    if name.is_empty() {
                        state.set_error("Asset name cannot be empty".to_string());
                    } else {
                        // Add the new holding with value 0, then start editing the value
                        let asset_tag = AssetTag(name);
                        let new_idx = {
                            if let Some(account) =
                                state.data_mut().portfolios.accounts.get_mut(account_idx)
                            {
                                match &mut account.account_type {
                                    AccountType::Brokerage(inv)
                                    | AccountType::Traditional401k(inv)
                                    | AccountType::Roth401k(inv)
                                    | AccountType::TraditionalIRA(inv)
                                    | AccountType::RothIRA(inv) => {
                                        inv.assets.push(crate::data::portfolio_data::AssetValue {
                                            asset: asset_tag,
                                            value: 0.0,
                                        });
                                        Some(inv.assets.len() - 1)
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        };
                        if let Some(idx) = new_idx {
                            state.mark_modified();
                            state.portfolio_profiles_state.selected_holding_index = idx;
                            state.portfolio_profiles_state.adding_new_holding = false;
                            state
                                .portfolio_profiles_state
                                .new_holding_name_buffer
                                .clear();
                            state.portfolio_profiles_state.editing_holding_value = true;
                            state.portfolio_profiles_state.holding_edit_buffer.clear();
                        }
                    }
                    EventResult::Handled
                }
                KeyCode::Backspace => {
                    state.portfolio_profiles_state.new_holding_name_buffer.pop();
                    EventResult::Handled
                }
                KeyCode::Char(c) => {
                    state
                        .portfolio_profiles_state
                        .new_holding_name_buffer
                        .push(c);
                    EventResult::Handled
                }
                _ => EventResult::Handled,
            }
        } else if editing_value {
            // Editing an existing holding's value
            match key.code {
                KeyCode::Esc => {
                    state.portfolio_profiles_state.editing_holding_value = false;
                    state.portfolio_profiles_state.holding_edit_buffer.clear();
                    EventResult::Handled
                }
                KeyCode::Enter => {
                    // Parse and save the value
                    let buffer = state.portfolio_profiles_state.holding_edit_buffer.clone();
                    // Remove commas and parse
                    let clean_buffer: String = buffer.chars().filter(|c| *c != ',').collect();
                    if let Ok(value) = clean_buffer.parse::<f64>() {
                        let holding_idx = state.portfolio_profiles_state.selected_holding_index;
                        if let Some(account) =
                            state.data_mut().portfolios.accounts.get_mut(account_idx)
                        {
                            match &mut account.account_type {
                                AccountType::Brokerage(inv)
                                | AccountType::Traditional401k(inv)
                                | AccountType::Roth401k(inv)
                                | AccountType::TraditionalIRA(inv)
                                | AccountType::RothIRA(inv) => {
                                    if let Some(asset) = inv.assets.get_mut(holding_idx) {
                                        asset.value = value;
                                        state.mark_modified();
                                    }
                                }
                                _ => {}
                            }
                        }
                        state.portfolio_profiles_state.editing_holding_value = false;
                        state.portfolio_profiles_state.holding_edit_buffer.clear();
                    } else {
                        state.set_error("Invalid number format".to_string());
                    }
                    EventResult::Handled
                }
                KeyCode::Backspace => {
                    state.portfolio_profiles_state.holding_edit_buffer.pop();
                    EventResult::Handled
                }
                KeyCode::Char(c) if c.is_ascii_digit() || c == '.' || c == ',' => {
                    state.portfolio_profiles_state.holding_edit_buffer.push(c);
                    EventResult::Handled
                }
                _ => EventResult::Handled,
            }
        } else {
            // Normal navigation mode within holdings
            let num_items = num_assets + 1; // +1 for "Add new" option
            let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                KeyCode::Esc => {
                    // Exit holdings editing mode
                    state.portfolio_profiles_state.editing_holdings = false;
                    state.portfolio_profiles_state.selected_holding_index = 0;
                    EventResult::Handled
                }
                // Move down (Shift+J or Shift+Down) - only for actual holdings, not "Add new"
                KeyCode::Char('J') if has_shift => {
                    let idx = state.portfolio_profiles_state.selected_holding_index;
                    // Only reorder if we have real assets and not on "Add new" option
                    if num_assets >= 2 && idx < num_assets - 1 {
                        if let Some(account) =
                            state.data_mut().portfolios.accounts.get_mut(account_idx)
                        {
                            match &mut account.account_type {
                                AccountType::Brokerage(inv)
                                | AccountType::Traditional401k(inv)
                                | AccountType::Roth401k(inv)
                                | AccountType::TraditionalIRA(inv)
                                | AccountType::RothIRA(inv) => {
                                    inv.assets.swap(idx, idx + 1);
                                    state.portfolio_profiles_state.selected_holding_index = idx + 1;
                                    state.mark_modified();
                                }
                                _ => {}
                            }
                        }
                    }
                    EventResult::Handled
                }
                KeyCode::Down if has_shift => {
                    let idx = state.portfolio_profiles_state.selected_holding_index;
                    if num_assets >= 2 && idx < num_assets - 1 {
                        if let Some(account) =
                            state.data_mut().portfolios.accounts.get_mut(account_idx)
                        {
                            match &mut account.account_type {
                                AccountType::Brokerage(inv)
                                | AccountType::Traditional401k(inv)
                                | AccountType::Roth401k(inv)
                                | AccountType::TraditionalIRA(inv)
                                | AccountType::RothIRA(inv) => {
                                    inv.assets.swap(idx, idx + 1);
                                    state.portfolio_profiles_state.selected_holding_index = idx + 1;
                                    state.mark_modified();
                                }
                                _ => {}
                            }
                        }
                    }
                    EventResult::Handled
                }
                // Move up (Shift+K or Shift+Up)
                KeyCode::Char('K') if has_shift => {
                    let idx = state.portfolio_profiles_state.selected_holding_index;
                    if num_assets >= 2 && idx > 0 && idx < num_assets {
                        if let Some(account) =
                            state.data_mut().portfolios.accounts.get_mut(account_idx)
                        {
                            match &mut account.account_type {
                                AccountType::Brokerage(inv)
                                | AccountType::Traditional401k(inv)
                                | AccountType::Roth401k(inv)
                                | AccountType::TraditionalIRA(inv)
                                | AccountType::RothIRA(inv) => {
                                    inv.assets.swap(idx, idx - 1);
                                    state.portfolio_profiles_state.selected_holding_index = idx - 1;
                                    state.mark_modified();
                                }
                                _ => {}
                            }
                        }
                    }
                    EventResult::Handled
                }
                KeyCode::Up if has_shift => {
                    let idx = state.portfolio_profiles_state.selected_holding_index;
                    if num_assets >= 2 && idx > 0 && idx < num_assets {
                        if let Some(account) =
                            state.data_mut().portfolios.accounts.get_mut(account_idx)
                        {
                            match &mut account.account_type {
                                AccountType::Brokerage(inv)
                                | AccountType::Traditional401k(inv)
                                | AccountType::Roth401k(inv)
                                | AccountType::TraditionalIRA(inv)
                                | AccountType::RothIRA(inv) => {
                                    inv.assets.swap(idx, idx - 1);
                                    state.portfolio_profiles_state.selected_holding_index = idx - 1;
                                    state.mark_modified();
                                }
                                _ => {}
                            }
                        }
                    }
                    EventResult::Handled
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if num_items > 0 {
                        state.portfolio_profiles_state.selected_holding_index =
                            (state.portfolio_profiles_state.selected_holding_index + 1) % num_items;
                    }
                    EventResult::Handled
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if num_items > 0 {
                        if state.portfolio_profiles_state.selected_holding_index == 0 {
                            state.portfolio_profiles_state.selected_holding_index = num_items - 1;
                        } else {
                            state.portfolio_profiles_state.selected_holding_index -= 1;
                        }
                    }
                    EventResult::Handled
                }
                KeyCode::Enter => {
                    let selected = state.portfolio_profiles_state.selected_holding_index;
                    if selected == num_assets {
                        // "Add new" option selected - start adding a new holding
                        state.portfolio_profiles_state.adding_new_holding = true;
                        state
                            .portfolio_profiles_state
                            .new_holding_name_buffer
                            .clear();
                    } else if selected < num_assets {
                        // Edit existing holding value - get current value first
                        let current_value = {
                            if let Some(account) = state.data().portfolios.accounts.get(account_idx)
                            {
                                match &account.account_type {
                                    AccountType::Brokerage(inv)
                                    | AccountType::Traditional401k(inv)
                                    | AccountType::Roth401k(inv)
                                    | AccountType::TraditionalIRA(inv)
                                    | AccountType::RothIRA(inv) => {
                                        inv.assets.get(selected).map(|asset| asset.value)
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        };
                        if let Some(value) = current_value {
                            state.portfolio_profiles_state.editing_holding_value = true;
                            // Pre-populate with current value (without $ and commas)
                            state.portfolio_profiles_state.holding_edit_buffer =
                                format!("{:.0}", value);
                        }
                    }
                    EventResult::Handled
                }
                KeyCode::Char('d') => {
                    // Delete selected holding
                    let selected = state.portfolio_profiles_state.selected_holding_index;
                    if selected < num_assets {
                        // Get the asset name for confirmation
                        let asset_name = {
                            let accounts = &state.data().portfolios.accounts;
                            if let Some(account) = accounts.get(account_idx) {
                                match &account.account_type {
                                    AccountType::Brokerage(inv)
                                    | AccountType::Traditional401k(inv)
                                    | AccountType::Roth401k(inv)
                                    | AccountType::TraditionalIRA(inv)
                                    | AccountType::RothIRA(inv) => {
                                        inv.assets.get(selected).map(|a| a.asset.0.clone())
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        };

                        if let Some(name) = asset_name {
                            state.modal = ModalState::Confirm(
                                ConfirmModal::new(
                                    "Delete Holding",
                                    &format!("Delete holding '{}'?", name),
                                    ModalAction::DELETE_HOLDING,
                                )
                                .with_typed_context(
                                    ModalContext::holding_index(account_idx, selected),
                                ),
                            );
                        }
                    }
                    EventResult::Handled
                }
                _ => EventResult::NotHandled,
            }
        }
    }

    fn create_account_edit_form(account: &AccountData, state: &AppState) -> FormModal {
        let type_name = format_account_type(&account.account_type);

        // Build list of available return profiles for Select fields
        let mut profile_options: Vec<String> = vec!["".to_string()]; // Empty option for "none"
        profile_options.extend(state.data().profiles.iter().map(|p| p.name.0.clone()));

        match &account.account_type {
            AccountType::Checking(prop)
            | AccountType::Savings(prop)
            | AccountType::HSA(prop)
            | AccountType::Property(prop)
            | AccountType::Collectible(prop) => {
                let profile_str = prop
                    .return_profile
                    .as_ref()
                    .map(|p| p.0.clone())
                    .unwrap_or_default();
                FormModal::new(
                    "Edit Account",
                    vec![
                        FormField::read_only("Type", type_name),
                        FormField::text("Name", &account.name),
                        FormField::text(
                            "Description",
                            account.description.as_deref().unwrap_or(""),
                        ),
                        FormField::currency("Value", prop.value),
                        FormField::select("Return Profile", profile_options, &profile_str),
                    ],
                    ModalAction::EDIT_ACCOUNT,
                )
            }
            AccountType::Mortgage(debt)
            | AccountType::LoanDebt(debt)
            | AccountType::StudentLoanDebt(debt) => FormModal::new(
                "Edit Account",
                vec![
                    FormField::read_only("Type", type_name),
                    FormField::text("Name", &account.name),
                    FormField::text("Description", account.description.as_deref().unwrap_or("")),
                    FormField::currency("Balance", debt.balance),
                    FormField::percentage("Interest Rate", debt.interest_rate),
                ],
                ModalAction::EDIT_ACCOUNT,
            ),
            AccountType::Brokerage(_)
            | AccountType::Traditional401k(_)
            | AccountType::Roth401k(_)
            | AccountType::TraditionalIRA(_)
            | AccountType::RothIRA(_) => {
                // Investment accounts - just edit name/description
                FormModal::new(
                    "Edit Account",
                    vec![
                        FormField::read_only("Type", type_name),
                        FormField::text("Name", &account.name),
                        FormField::text(
                            "Description",
                            account.description.as_deref().unwrap_or(""),
                        ),
                    ],
                    ModalAction::EDIT_ACCOUNT,
                )
            }
        }
    }

    fn handle_profiles_keys(&self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let profiles_len = state.data().profiles.len();
        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            // Move down (Shift+J or Shift+Down)
            KeyCode::Char('J') if has_shift => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if profiles_len >= 2 && idx < profiles_len - 1 {
                    state.data_mut().profiles.swap(idx, idx + 1);
                    state.portfolio_profiles_state.selected_profile_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Down if has_shift => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if profiles_len >= 2 && idx < profiles_len - 1 {
                    state.data_mut().profiles.swap(idx, idx + 1);
                    state.portfolio_profiles_state.selected_profile_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            // Move up (Shift+K or Shift+Up)
            KeyCode::Char('K') if has_shift => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if profiles_len >= 2 && idx > 0 {
                    state.data_mut().profiles.swap(idx, idx - 1);
                    state.portfolio_profiles_state.selected_profile_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Up if has_shift => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if profiles_len >= 2 && idx > 0 {
                    state.data_mut().profiles.swap(idx, idx - 1);
                    state.portfolio_profiles_state.selected_profile_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let profiles = &state.data().profiles;
                if !profiles.is_empty() {
                    state.portfolio_profiles_state.selected_profile_index =
                        (state.portfolio_profiles_state.selected_profile_index + 1)
                            % profiles.len();
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let profiles = &state.data().profiles;
                if !profiles.is_empty() {
                    if state.portfolio_profiles_state.selected_profile_index == 0 {
                        state.portfolio_profiles_state.selected_profile_index = profiles.len() - 1;
                    } else {
                        state.portfolio_profiles_state.selected_profile_index -= 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Char('a') => {
                // Add new profile - show type picker
                let types = vec![
                    "None".to_string(),
                    "Fixed Rate".to_string(),
                    "Normal Distribution".to_string(),
                    "Log-Normal Distribution".to_string(),
                ];
                state.modal = ModalState::Picker(PickerModal::new(
                    "Select Profile Type",
                    types,
                    ModalAction::PICK_PROFILE_TYPE,
                ));
                EventResult::Handled
            }
            KeyCode::Char('e') => {
                // Edit selected profile
                if let Some(profile_data) = state
                    .data()
                    .profiles
                    .get(state.portfolio_profiles_state.selected_profile_index)
                {
                    let form = Self::create_profile_edit_form(profile_data);
                    state.modal =
                        ModalState::Form(form.with_typed_context(ModalContext::profile_index(
                            state.portfolio_profiles_state.selected_profile_index,
                        )));
                }
                EventResult::Handled
            }
            KeyCode::Char('d') => {
                // Delete selected profile with confirmation
                if let Some(profile_data) = state
                    .data()
                    .profiles
                    .get(state.portfolio_profiles_state.selected_profile_index)
                {
                    state.modal = ModalState::Confirm(
                        ConfirmModal::new(
                            "Delete Profile",
                            &format!("Delete profile '{}'?", profile_data.name.0),
                            ModalAction::DELETE_PROFILE,
                        )
                        .with_typed_context(ModalContext::profile_index(
                            state.portfolio_profiles_state.selected_profile_index,
                        )),
                    );
                }
                EventResult::Handled
            }
            // Preset shortcuts
            KeyCode::Char('1') => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::Fixed { rate: 0.095668 };
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('2') => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::Normal {
                        mean: 0.095668,
                        std_dev: 0.165244,
                    };
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('3') => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::LogNormal {
                        mean: 0.095668,
                        std_dev: 0.165244,
                    };
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('4') => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::None;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char(' ') => {
                // Space: toggle both secondary panels and focus Config
                let expanding = state.portfolio_profiles_state.config_collapsed;
                state.portfolio_profiles_state.mappings_collapsed = !expanding;
                state.portfolio_profiles_state.config_collapsed = !expanding;
                if expanding {
                    state.portfolio_profiles_state.focused_panel = PortfolioProfilesPanel::Config;
                }
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    fn create_profile_edit_form(profile_data: &ProfileData) -> FormModal {
        let type_name = Self::format_profile_type(&profile_data.profile);
        match &profile_data.profile {
            ReturnProfileData::None => FormModal::new(
                "Edit Profile",
                vec![
                    FormField::text("Name", &profile_data.name.0),
                    FormField::text(
                        "Description",
                        profile_data.description.as_deref().unwrap_or(""),
                    ),
                    FormField::read_only("Type", &type_name),
                ],
                ModalAction::EDIT_PROFILE,
            ),
            ReturnProfileData::Fixed { rate } => FormModal::new(
                "Edit Profile",
                vec![
                    FormField::text("Name", &profile_data.name.0),
                    FormField::text(
                        "Description",
                        profile_data.description.as_deref().unwrap_or(""),
                    ),
                    FormField::read_only("Type", &type_name),
                    FormField::percentage("Rate", *rate),
                ],
                ModalAction::EDIT_PROFILE,
            ),
            ReturnProfileData::Normal { mean, std_dev }
            | ReturnProfileData::LogNormal { mean, std_dev } => FormModal::new(
                "Edit Profile",
                vec![
                    FormField::text("Name", &profile_data.name.0),
                    FormField::text(
                        "Description",
                        profile_data.description.as_deref().unwrap_or(""),
                    ),
                    FormField::read_only("Type", &type_name),
                    FormField::percentage("Mean", *mean),
                    FormField::percentage("Std Dev", *std_dev),
                ],
                ModalAction::EDIT_PROFILE,
            ),
        }
    }

    fn handle_mappings_keys(&self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let unique_assets = Self::get_unique_assets(state);
        match key.code {
            KeyCode::Char(' ') => {
                // Collapse both secondary panels and return to main panel
                state.portfolio_profiles_state.mappings_collapsed = true;
                state.portfolio_profiles_state.config_collapsed = true;
                state.portfolio_profiles_state.focused_panel = PortfolioProfilesPanel::Accounts;
                EventResult::Handled
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !unique_assets.is_empty() {
                    state.portfolio_profiles_state.selected_mapping_index =
                        (state.portfolio_profiles_state.selected_mapping_index + 1)
                            % unique_assets.len();
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !unique_assets.is_empty() {
                    if state.portfolio_profiles_state.selected_mapping_index == 0 {
                        state.portfolio_profiles_state.selected_mapping_index =
                            unique_assets.len() - 1;
                    } else {
                        state.portfolio_profiles_state.selected_mapping_index -= 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Char('m') | KeyCode::Enter => {
                // Map the selected asset to a profile
                if let Some(asset) =
                    unique_assets.get(state.portfolio_profiles_state.selected_mapping_index)
                {
                    // For now, cycle through available profiles
                    let profiles = &state.data().profiles;
                    if !profiles.is_empty() {
                        let current_mapping = state.data().assets.get(asset);
                        let current_idx = current_mapping
                            .and_then(|tag| profiles.iter().position(|p| &p.name == tag))
                            .unwrap_or(profiles.len());

                        let next_idx = if current_idx >= profiles.len() - 1 {
                            0
                        } else {
                            current_idx + 1
                        };

                        let new_profile = profiles[next_idx].name.clone();
                        let asset_clone = asset.clone();
                        state.data_mut().assets.insert(asset_clone, new_profile);
                        state.mark_modified();
                    } else {
                        state.set_error(
                            "No return profiles defined. Add a profile first.".to_string(),
                        );
                    }
                }
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    fn handle_config_keys(&self, key: KeyEvent, state: &mut AppState) -> EventResult {
        const CONFIG_ITEMS: usize = 4; // Federal, State, Cap Gains, Inflation
        match key.code {
            KeyCode::Char(' ') => {
                // Collapse both secondary panels and return to main panel
                state.portfolio_profiles_state.mappings_collapsed = true;
                state.portfolio_profiles_state.config_collapsed = true;
                state.portfolio_profiles_state.focused_panel = PortfolioProfilesPanel::Profiles;
                EventResult::Handled
            }
            KeyCode::Char('j') | KeyCode::Down => {
                state.portfolio_profiles_state.selected_config_index =
                    (state.portfolio_profiles_state.selected_config_index + 1) % CONFIG_ITEMS;
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if state.portfolio_profiles_state.selected_config_index == 0 {
                    state.portfolio_profiles_state.selected_config_index = CONFIG_ITEMS - 1;
                } else {
                    state.portfolio_profiles_state.selected_config_index -= 1;
                }
                EventResult::Handled
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                // Edit the selected config item
                match state.portfolio_profiles_state.selected_config_index {
                    0 => {
                        // Federal Brackets - picker
                        let options =
                            vec!["2024 Single".to_string(), "2024 Married Joint".to_string()];
                        state.modal = ModalState::Picker(PickerModal::new(
                            "Federal Tax Brackets",
                            options,
                            ModalAction::PICK_FEDERAL_BRACKETS,
                        ));
                    }
                    1 => {
                        // State Rate - form
                        let rate = state.data().parameters.tax_config.state_rate;
                        state.modal = ModalState::Form(
                            FormModal::new(
                                "Edit State Tax Rate",
                                vec![FormField::percentage("State Rate", rate)],
                                ModalAction::EDIT_TAX_CONFIG,
                            )
                            .with_typed_context(ModalContext::Config(
                                ConfigContext::Tax(TaxConfigContext::StateRate),
                            )),
                        );
                    }
                    2 => {
                        // Capital Gains Rate - form
                        let rate = state.data().parameters.tax_config.capital_gains_rate;
                        state.modal = ModalState::Form(
                            FormModal::new(
                                "Edit Capital Gains Rate",
                                vec![FormField::percentage("Capital Gains Rate", rate)],
                                ModalAction::EDIT_TAX_CONFIG,
                            )
                            .with_typed_context(ModalContext::Config(
                                ConfigContext::Tax(TaxConfigContext::CapGainsRate),
                            )),
                        );
                    }
                    3 => {
                        // Inflation - picker for type
                        let options = vec![
                            "None".to_string(),
                            "Fixed".to_string(),
                            "Normal".to_string(),
                            "Log-Normal".to_string(),
                            "US Historical".to_string(),
                        ];
                        state.modal = ModalState::Picker(PickerModal::new(
                            "Inflation Type",
                            options,
                            ModalAction::PICK_INFLATION_TYPE,
                        ));
                    }
                    _ => {}
                }
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
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

impl Component for PortfolioProfilesScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        // If in holdings editing mode, handle all keys there first (captures Tab, etc.)
        if state.portfolio_profiles_state.editing_holdings {
            return self.handle_holdings_keys(key, state);
        }

        match key.code {
            // Tab cycling through all panels
            KeyCode::Tab if key.modifiers.is_empty() => {
                state.portfolio_profiles_state.focused_panel =
                    state.portfolio_profiles_state.focused_panel.next();
                EventResult::Handled
            }
            KeyCode::BackTab => {
                state.portfolio_profiles_state.focused_panel =
                    state.portfolio_profiles_state.focused_panel.prev();
                EventResult::Handled
            }
            _ => {
                // Delegate to focused panel handler
                match state.portfolio_profiles_state.focused_panel {
                    PortfolioProfilesPanel::Accounts => self.handle_accounts_keys(key, state),
                    PortfolioProfilesPanel::Profiles => self.handle_profiles_keys(key, state),
                    PortfolioProfilesPanel::AssetMappings => self.handle_mappings_keys(key, state),
                    PortfolioProfilesPanel::Config => self.handle_config_keys(key, state),
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        // Calculate secondary panel heights based on collapse state
        let mappings_collapsed = state.portfolio_profiles_state.mappings_collapsed;
        let config_collapsed = state.portfolio_profiles_state.config_collapsed;

        let secondary_height = match (mappings_collapsed, config_collapsed) {
            (true, true) => 2,    // Two collapsed lines (1 line each)
            (true, false) => 8,   // One collapsed, one expanded
            (false, true) => 8,   // One collapsed, one expanded
            (false, false) => 14, // Both expanded
        };

        // Main vertical layout
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10),               // Portfolio Overview (fixed)
                Constraint::Min(15),                  // Main content (flexible)
                Constraint::Length(secondary_height), // Secondary panels
            ])
            .split(area);

        // Portfolio Overview - always visible at top
        self.render_portfolio_overview(frame, main_layout[0], state);

        // Main content: 2 columns (50/50)
        let content_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_layout[1]);

        self.render_unified_accounts(frame, content_cols[0], state);
        self.render_unified_profiles(frame, content_cols[1], state);

        // Secondary panels at bottom
        self.render_secondary_panels(frame, main_layout[2], state);
    }
}

impl Screen for PortfolioProfilesScreen {
    fn title(&self) -> &str {
        "Portfolio & Profiles"
    }
}
