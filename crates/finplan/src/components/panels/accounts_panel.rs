//! Accounts panel component extracted from PortfolioProfilesScreen.
//!
//! Renders the unified accounts panel with list, details, and holdings chart.

use crate::actions::create_edit_account_form;
use crate::components::EventResult;
use crate::components::lists::calculate_centered_scroll;
use crate::data::portfolio_data::{AccountData, AccountType};
use crate::modals::context::ModalContext;
use crate::modals::{ConfirmModal, ModalAction, ModalState, PickerModal};
use crate::state::{AccountInteractionMode, AppState, PortfolioProfilesPanel};
use crate::util::format::format_currency;
use crate::util::styles::focused_block_with_help;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

/// Accounts panel component.
pub struct AccountsPanel;

impl AccountsPanel {
    /// Render the unified accounts panel (list | details, holdings chart).
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Accounts;

        let help_text = Self::get_help_text(state);
        let block = focused_block_with_help(" ACCOUNTS ", is_focused, &help_text);

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        // Split vertically: ~45% top (list|details), ~55% bottom (chart)
        let top_height = (inner_area.height as f32 * 0.45).max(5.0) as u16;
        let bottom_height = inner_area.height.saturating_sub(top_height + 1);

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
        let details_width = top_area.width.saturating_sub(list_width + 1);

        let list_area = Rect::new(top_area.x, top_area.y, list_width, top_area.height);
        let vsep_area = Rect::new(top_area.x + list_width, top_area.y, 1, top_area.height);
        let details_area = Rect::new(
            top_area.x + list_width + 1,
            top_area.y,
            details_width,
            top_area.height,
        );

        Self::render_account_list(frame, list_area, state);
        Self::render_vertical_separator(frame, vsep_area);
        Self::render_account_details(frame, details_area, state);
        Self::render_horizontal_separator(frame, hsep_area, inner_area.width, " HOLDINGS ");
        Self::render_holdings_chart(frame, bottom_area, state);
    }

    fn get_help_text(state: &AppState) -> String {
        if state
            .portfolio_profiles_state
            .account_mode
            .is_editing_holdings()
        {
            if state
                .portfolio_profiles_state
                .account_mode
                .is_editing_value()
                || state.portfolio_profiles_state.account_mode.is_adding_new()
            {
                " [Enter] Save  [Esc] Cancel ".to_string()
            } else {
                " [Enter] Edit [d] Del [Shift+J/K] Reorder [Esc] Exit ".to_string()
            }
        } else {
            " [a]dd [e]dit [d]el [Enter] Holdings [Shift+J/K] Reorder ".to_string()
        }
    }

    fn render_account_list(frame: &mut Frame, area: Rect, state: &AppState) {
        let accounts = &state.data().portfolios.accounts;
        let visible_count = area.height as usize;
        let selected_idx = state.portfolio_profiles_state.selected_account_index;
        let scroll_offset = calculate_centered_scroll(selected_idx, accounts.len(), visible_count);

        let mut lines = Vec::new();
        for (idx, account) in accounts
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_count)
        {
            let value = get_account_value(account);
            let prefix = if idx == selected_idx { "> " } else { "  " };
            let max_name_len = area.width.saturating_sub(15) as usize;
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

            let style = if idx == selected_idx {
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
        frame.render_widget(list_para, area);
    }

    fn render_vertical_separator(frame: &mut Frame, area: Rect) {
        let mut lines = Vec::new();
        for _ in 0..area.height {
            lines.push(Line::from(Span::styled(
                "│",
                Style::default().fg(Color::DarkGray),
            )));
        }
        let sep = Paragraph::new(lines);
        frame.render_widget(sep, area);
    }

    fn render_account_details(frame: &mut Frame, area: Rect, state: &AppState) {
        let accounts = &state.data().portfolios.accounts;
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
        frame.render_widget(details_para, area);
    }

    fn render_horizontal_separator(frame: &mut Frame, area: Rect, width: u16, label: &str) {
        let sep_width = width as usize;
        let label_len = label.len();
        let left_dashes = (sep_width.saturating_sub(label_len)) / 2;
        let right_dashes = sep_width.saturating_sub(label_len + left_dashes);
        let separator_text = format!(
            "{}{}{}",
            "─".repeat(left_dashes),
            label,
            "─".repeat(right_dashes)
        );
        let sep = Paragraph::new(Line::from(Span::styled(
            separator_text,
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(sep, area);
    }

    fn render_holdings_chart(frame: &mut Frame, area: Rect, state: &AppState) {
        // Center the chart with ~20% padding on each side
        let padding = (area.width as f32 * 0.20) as u16;
        let chart_width = area.width.saturating_sub(padding * 2);
        let chart_area = Rect::new(area.x + padding, area.y, chart_width, area.height);

        Self::render_asset_bars(frame, chart_area, state);
    }

    fn render_asset_bars(frame: &mut Frame, area: Rect, state: &AppState) {
        let account = match state
            .data()
            .portfolios
            .accounts
            .get(state.portfolio_profiles_state.selected_account_index)
        {
            Some(a) => a,
            None => return,
        };

        let assets = match &account.account_type {
            AccountType::Brokerage(inv)
            | AccountType::Traditional401k(inv)
            | AccountType::Roth401k(inv)
            | AccountType::TraditionalIRA(inv)
            | AccountType::RothIRA(inv) => &inv.assets,
            _ => return,
        };

        if assets.is_empty() {
            let empty_msg = Paragraph::new(Line::from(Span::styled(
                "No holdings. Press Enter to add.",
                Style::default().fg(Color::DarkGray),
            )));
            frame.render_widget(empty_msg, area);
            return;
        }

        let total: f64 = assets.iter().map(|a| a.value).sum();
        if total <= 0.0 {
            return;
        }

        let visible_count = area.height as usize;
        let editing_mode = &state.portfolio_profiles_state.account_mode;
        let (scroll_offset, selected_holding) = match editing_mode {
            AccountInteractionMode::EditingHoldings { selected_index, .. } => {
                let offset =
                    calculate_centered_scroll(*selected_index, assets.len(), visible_count);
                (offset, Some(*selected_index))
            }
            _ => (0, None),
        };

        let bar_width = area.width.saturating_sub(25) as usize; // Leave room for labels

        for (line_idx, (asset_idx, asset)) in assets
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_count)
            .enumerate()
        {
            let pct = asset.value / total;
            let filled = ((pct * bar_width as f64).round() as usize).min(bar_width);

            let is_selected = selected_holding == Some(asset_idx);
            let bar_color = get_asset_color(asset_idx);

            let prefix = if is_selected { "> " } else { "  " };
            let ticker = format!("{:>6}", &asset.asset.0);
            let bar: String = "█".repeat(filled) + &" ".repeat(bar_width - filled);
            let pct_str = format!("{:>5.1}%", pct * 100.0);
            let val_str = format!("{:>10}", format_currency(asset.value));

            let style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let line = Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(ticker, style),
                Span::raw(" "),
                Span::styled(&bar, Style::default().fg(bar_color)),
                Span::raw(" "),
                Span::styled(pct_str, style),
                Span::styled(val_str, style),
            ]);

            let line_area = Rect::new(area.x, area.y + line_idx as u16, area.width, 1);
            frame.render_widget(Paragraph::new(line), line_area);
        }
    }

    /// Handle key events for the accounts panel.
    pub fn handle_key(key: KeyEvent, state: &mut AppState) -> EventResult {
        // Delegate to holdings handler if in editing mode
        if state
            .portfolio_profiles_state
            .account_mode
            .is_editing_holdings()
        {
            return Self::handle_holdings_keys(key, state);
        }

        Self::handle_accounts_keys(key, state)
    }

    fn handle_accounts_keys(key: KeyEvent, state: &mut AppState) -> EventResult {
        let accounts_len = state.data().portfolios.accounts.len();
        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);

        match key.code {
            // Reorder down (Shift+J or Shift+Down)
            KeyCode::Char('J') | KeyCode::Down if has_shift => {
                let idx = state.portfolio_profiles_state.selected_account_index;
                if accounts_len >= 2 && idx < accounts_len - 1 {
                    state.data_mut().portfolios.accounts.swap(idx, idx + 1);
                    state.portfolio_profiles_state.selected_account_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            // Reorder up (Shift+K or Shift+Up)
            KeyCode::Char('K') | KeyCode::Up if has_shift => {
                let idx = state.portfolio_profiles_state.selected_account_index;
                if accounts_len >= 2 && idx > 0 {
                    state.data_mut().portfolios.accounts.swap(idx, idx - 1);
                    state.portfolio_profiles_state.selected_account_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            // Navigate down
            KeyCode::Char('j') | KeyCode::Down => {
                if accounts_len > 0 {
                    state.portfolio_profiles_state.selected_account_index =
                        (state.portfolio_profiles_state.selected_account_index + 1) % accounts_len;
                }
                EventResult::Handled
            }
            // Navigate up
            KeyCode::Char('k') | KeyCode::Up => {
                if accounts_len > 0 {
                    if state.portfolio_profiles_state.selected_account_index == 0 {
                        state.portfolio_profiles_state.selected_account_index = accounts_len - 1;
                    } else {
                        state.portfolio_profiles_state.selected_account_index -= 1;
                    }
                }
                EventResult::Handled
            }
            // Enter holdings editing mode
            KeyCode::Enter => {
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
                            state.portfolio_profiles_state.account_mode =
                                AccountInteractionMode::enter_editing(0);
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
            // Add account - show category picker
            KeyCode::Char('a') => {
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
            // Edit account
            KeyCode::Char('e') => {
                if let Some(account) = state
                    .data()
                    .portfolios
                    .accounts
                    .get(state.portfolio_profiles_state.selected_account_index)
                {
                    let form = create_edit_account_form(account);
                    state.modal =
                        ModalState::Form(form.with_typed_context(ModalContext::account_index(
                            state.portfolio_profiles_state.selected_account_index,
                        )));
                }
                EventResult::Handled
            }
            // Delete account
            KeyCode::Char('d') => {
                if let Some(account) = state
                    .data()
                    .portfolios
                    .accounts
                    .get(state.portfolio_profiles_state.selected_account_index)
                {
                    state.modal = ModalState::Confirm(
                        ConfirmModal::new(
                            "Delete Account",
                            &format!(
                                "Delete account '{}'?\n\nThis cannot be undone.",
                                account.name
                            ),
                            ModalAction::DELETE_ACCOUNT,
                        )
                        .with_typed_context(ModalContext::account_index(
                            state.portfolio_profiles_state.selected_account_index,
                        )),
                    );
                }
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    fn handle_holdings_keys(key: KeyEvent, state: &mut AppState) -> EventResult {
        // Placeholder - full implementation will be moved from portfolio_profiles.rs
        match key.code {
            KeyCode::Esc => {
                state.portfolio_profiles_state.account_mode = AccountInteractionMode::Browsing;
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }
}

// Helper functions

fn get_account_value(account: &AccountData) -> f64 {
    match &account.account_type {
        AccountType::Checking(p)
        | AccountType::Savings(p)
        | AccountType::HSA(p)
        | AccountType::Property(p)
        | AccountType::Collectible(p) => p.value,
        AccountType::Brokerage(inv)
        | AccountType::Traditional401k(inv)
        | AccountType::Roth401k(inv)
        | AccountType::TraditionalIRA(inv)
        | AccountType::RothIRA(inv) => inv.assets.iter().map(|a| a.value).sum(),
        AccountType::Mortgage(d) | AccountType::LoanDebt(d) | AccountType::StudentLoanDebt(d) => {
            -d.balance
        }
    }
}

fn format_account_type(account_type: &AccountType) -> &'static str {
    match account_type {
        AccountType::Checking(_) => "Checking",
        AccountType::Savings(_) => "Savings",
        AccountType::HSA(_) => "HSA",
        AccountType::Property(_) => "Property",
        AccountType::Collectible(_) => "Collectible",
        AccountType::Brokerage(_) => "Brokerage",
        AccountType::Traditional401k(_) => "401(k)",
        AccountType::Roth401k(_) => "Roth 401(k)",
        AccountType::TraditionalIRA(_) => "Traditional IRA",
        AccountType::RothIRA(_) => "Roth IRA",
        AccountType::Mortgage(_) => "Mortgage",
        AccountType::LoanDebt(_) => "Loan",
        AccountType::StudentLoanDebt(_) => "Student Loan",
    }
}

fn get_asset_color(idx: usize) -> Color {
    const COLORS: [Color; 8] = [
        Color::Blue,
        Color::Green,
        Color::Cyan,
        Color::Magenta,
        Color::Yellow,
        Color::Red,
        Color::LightBlue,
        Color::LightGreen,
    ];
    COLORS[idx % COLORS.len()]
}
