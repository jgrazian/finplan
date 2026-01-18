use std::collections::HashSet;

use crate::components::collapsible::CollapsiblePanel;
use crate::components::{Component, EventResult};
use crate::data::parameters_data::{FederalBracketsPreset, InflationData};
use crate::data::portfolio_data::{AccountData, AccountType, AssetTag};
use crate::data::profiles_data::{ProfileData, ReturnProfileData};
use crate::state::{
    AppState, ConfirmModal, FormField, FormModal, ModalAction, ModalState, PickerModal,
    PortfolioProfilesPanel,
};
use crate::util::format::{format_currency, format_percentage};
use crossterm::event::{KeyCode, KeyEvent};
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

    // ========== Left Column: Accounts List ==========

    fn render_account_list(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Accounts;

        let items: Vec<ListItem> = state
            .data()
            .portfolios
            .accounts
            .iter()
            .enumerate()
            .map(|(idx, account)| {
                let value = get_account_value(account);
                let content = format!("{:<16} {:>10}", account.name, format_currency(value));

                let style = if idx == state.portfolio_profiles_state.selected_account_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(Span::styled(content, style)))
            })
            .collect();

        let title = " ACCOUNTS ";
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

    // ========== Middle Column: Account Details & Profile Details ==========

    fn render_account_details(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Accounts;

        let content = if let Some(account) = state
            .data()
            .portfolios
            .accounts
            .get(state.portfolio_profiles_state.selected_account_index)
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

            lines.push(Line::from(""));

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
                        Span::styled("Rate: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(format!("{:.2}%", debt.interest_rate * 100.0)),
                    ]));
                }
            }

            lines
        } else {
            vec![Line::from("No account selected")]
        };

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" ACCOUNT DETAILS ")
                    .border_style(border_style),
            )
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    fn render_profile_details(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Profiles;

        let title = " PROFILE DETAILS ";

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let content = if let Some(profile_data) = state
            .data()
            .profiles
            .get(state.portfolio_profiles_state.selected_profile_index)
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
                Line::from(Self::format_profile_params(&profile_data.profile)),
            ];

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[1-4] Apply preset",
                Style::default().fg(Color::DarkGray),
            )));

            lines
        } else {
            vec![Line::from("No profile selected")]
        };

        let paragraph = Paragraph::new(content).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style),
        );

        frame.render_widget(paragraph, area);
    }

    // ========== Right Column: Profiles, Mappings, Config ==========

    fn render_profile_list(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Profiles;

        let items: Vec<ListItem> = state
            .data()
            .profiles
            .iter()
            .enumerate()
            .map(|(idx, profile_data)| {
                let profile_desc = Self::format_profile(&profile_data.profile);
                let content = format!("{}: {}", profile_data.name.0, profile_desc);

                let style = if idx == state.portfolio_profiles_state.selected_profile_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(Span::styled(content, style)))
            })
            .collect();

        let title = " RETURN PROFILES ";

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

    fn render_asset_mappings(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::AssetMappings;
        let is_collapsed = state.portfolio_profiles_state.mappings_collapsed;

        // Handle collapsed state
        if is_collapsed {
            let panel = CollapsiblePanel::new("ASSET MAPPINGS", false).focused(is_focused);
            panel.render_collapsed(frame, area);
            return;
        }

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

        let indicator = "[-]";
        let title = format!(" {} ASSET MAPPINGS ", indicator);

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
            block = block.title_bottom(Line::from(" [m] Map  [Space] Toggle ").fg(Color::DarkGray));
        }

        let list = List::new(items).block(block);

        frame.render_widget(list, area);
    }

    fn render_tax_inflation_config(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Config;
        let is_collapsed = state.portfolio_profiles_state.config_collapsed;

        // Handle collapsed state
        if is_collapsed {
            let panel = CollapsiblePanel::new("TAX & INFLATION", false).focused(is_focused);
            panel.render_collapsed(frame, area);
            return;
        }

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

        let indicator = "[-]";
        let title = format!(" {} TAX & INFLATION ", indicator);

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
            block = block.title_bottom(Line::from(" [e] Edit  [Space] Toggle ").fg(Color::DarkGray));
        }

        let paragraph = Paragraph::new(lines).block(block);

        frame.render_widget(paragraph, area);
    }

    // ========== Formatters ==========

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
            ReturnProfileData::None => "Return: 0%".to_string(),
            ReturnProfileData::Fixed { rate } => format!("Rate: {}", format_percentage(*rate)),
            ReturnProfileData::Normal { mean, std_dev } => {
                format!(
                    "μ={}, σ={}",
                    format_percentage(*mean),
                    format_percentage(*std_dev)
                )
            }
            ReturnProfileData::LogNormal { mean, std_dev } => {
                format!(
                    "μ={}, σ={}",
                    format_percentage(*mean),
                    format_percentage(*std_dev)
                )
            }
        }
    }

    // ========== Key Handlers ==========

    fn handle_accounts_keys(&self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let accounts = &state.data().portfolios.accounts;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if !accounts.is_empty() {
                    state.portfolio_profiles_state.selected_account_index =
                        (state.portfolio_profiles_state.selected_account_index + 1)
                            % accounts.len();
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !accounts.is_empty() {
                    if state.portfolio_profiles_state.selected_account_index == 0 {
                        state.portfolio_profiles_state.selected_account_index = accounts.len() - 1;
                    } else {
                        state.portfolio_profiles_state.selected_account_index -= 1;
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
                    state.modal = ModalState::Form(
                        form.with_context(
                            &state
                                .portfolio_profiles_state
                                .selected_account_index
                                .to_string(),
                        ),
                    );
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
                        .with_context(
                            &state
                                .portfolio_profiles_state
                                .selected_account_index
                                .to_string(),
                        ),
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
                            .with_context(
                                &state
                                    .portfolio_profiles_state
                                    .selected_account_index
                                    .to_string(),
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
            _ => EventResult::NotHandled,
        }
    }

    fn create_account_edit_form(account: &AccountData, _state: &AppState) -> FormModal {
        let type_name = format_account_type(&account.account_type);

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
                        FormField::text("Return Profile", &profile_str),
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
        let profiles = &state.data().profiles;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if !profiles.is_empty() {
                    state.portfolio_profiles_state.selected_profile_index =
                        (state.portfolio_profiles_state.selected_profile_index + 1)
                            % profiles.len();
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
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
                    state.modal = ModalState::Form(
                        form.with_context(
                            &state
                                .portfolio_profiles_state
                                .selected_profile_index
                                .to_string(),
                        ),
                    );
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
                        .with_context(
                            &state
                                .portfolio_profiles_state
                                .selected_profile_index
                                .to_string(),
                        ),
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
                // Toggle collapse state
                state.portfolio_profiles_state.mappings_collapsed =
                    !state.portfolio_profiles_state.mappings_collapsed;
                EventResult::Handled
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !state.portfolio_profiles_state.mappings_collapsed && !unique_assets.is_empty() {
                    state.portfolio_profiles_state.selected_mapping_index =
                        (state.portfolio_profiles_state.selected_mapping_index + 1)
                            % unique_assets.len();
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !state.portfolio_profiles_state.mappings_collapsed && !unique_assets.is_empty() {
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
                // Toggle collapse state
                state.portfolio_profiles_state.config_collapsed =
                    !state.portfolio_profiles_state.config_collapsed;
                EventResult::Handled
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !state.portfolio_profiles_state.config_collapsed {
                    state.portfolio_profiles_state.selected_config_index =
                        (state.portfolio_profiles_state.selected_config_index + 1) % CONFIG_ITEMS;
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !state.portfolio_profiles_state.config_collapsed {
                    if state.portfolio_profiles_state.selected_config_index == 0 {
                        state.portfolio_profiles_state.selected_config_index = CONFIG_ITEMS - 1;
                    } else {
                        state.portfolio_profiles_state.selected_config_index -= 1;
                    }
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
                            .with_context("state_rate"),
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
                            .with_context("cap_gains_rate"),
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
        match key.code {
            // Tab cycling through panels
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
        // Create 3-column layout: 25% | 40% | 35%
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(40),
                Constraint::Percentage(35),
            ])
            .split(area);

        // Left column: Accounts list (55%) + Profiles list (45%)
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(columns[0]);

        self.render_account_list(frame, left_chunks[0], state);
        self.render_profile_list(frame, left_chunks[1], state);

        // Middle column: Account Details (55%) + Profile Details (45%)
        let middle_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(columns[1]);

        self.render_account_details(frame, middle_chunks[0], state);
        self.render_profile_details(frame, middle_chunks[1], state);

        // Right column: Asset Mappings + Tax & Inflation Config (with collapsible support)
        let mappings_collapsed = state.portfolio_profiles_state.mappings_collapsed;
        let config_collapsed = state.portfolio_profiles_state.config_collapsed;

        let right_constraints = match (mappings_collapsed, config_collapsed) {
            (false, false) => vec![Constraint::Percentage(50), Constraint::Percentage(50)],
            (true, false) => vec![Constraint::Length(3), Constraint::Min(5)],
            (false, true) => vec![Constraint::Min(5), Constraint::Length(3)],
            (true, true) => vec![Constraint::Length(3), Constraint::Length(3)],
        };

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(right_constraints)
            .split(columns[2]);

        self.render_asset_mappings(frame, right_chunks[0], state);
        self.render_tax_inflation_config(frame, right_chunks[1], state);
    }
}

impl Screen for PortfolioProfilesScreen {
    fn title(&self) -> &str {
        "Portfolio & Profiles"
    }
}
