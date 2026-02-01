use std::collections::HashSet;

use crate::actions::{self, ActionContext, ActionResult};
use crate::components::lists::calculate_centered_scroll;
use crate::components::panels::{AccountsPanel, ProfilesPanel};
use crate::components::portfolio_overview::{AccountBar, PortfolioOverviewChart};
use crate::components::{Component, EventResult};
use crate::data::keybindings_data::KeybindingsConfig;
use crate::data::parameters_data::{FederalBracketsPreset, InflationData, ReturnsMode};
use crate::data::portfolio_data::{AccountData, AccountType, AssetTag};
use crate::data::profiles_data::{ProfileData, ReturnProfileTag};
use crate::data::ticker_profiles;
use crate::data::ticker_profiles::HISTORICAL_PRESETS;
use crate::event::{AppKeyEvent, KeyCode};
use crate::modals::context::{ConfigContext, ModalContext, TaxConfigContext};
use crate::modals::{AccountAction, ConfigAction, HoldingAction, ProfileAction};
use crate::modals::{FormField, FormModal, MessageModal, ModalAction, ModalState, PickerModal};
use crate::state::{AppState, PortfolioProfilesPanel};
use crate::util::format::format_percentage;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::Screen;

pub struct PortfolioProfilesScreen;

impl PortfolioProfilesScreen {
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
        let is_historical = state.data().parameters.returns_mode == ReturnsMode::Historical;

        let tax_config = &state.data().parameters.tax_config;
        let federal_short = match &tax_config.federal_brackets {
            FederalBracketsPreset::Single2024 => "2024 Single",
            FederalBracketsPreset::MarriedJoint2024 => "2024 MJ",
            FederalBracketsPreset::Custom { .. } => "Custom",
        };

        // In Historical mode, inflation is always US Historical Bootstrap
        let inflation_short = if is_historical {
            "US Hist Bootstrap"
        } else {
            match &state.data().parameters.inflation {
                InflationData::None => "None",
                InflationData::Fixed { .. } => "Fixed",
                InflationData::Normal { .. } => "Normal",
                InflationData::LogNormal { .. } => "LogN",
                InflationData::USHistorical { .. } => "US Hist",
            }
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
        let is_historical = state.data().parameters.returns_mode == ReturnsMode::Historical;

        let unique_assets = Self::get_unique_assets(state);
        // Use mode-specific mappings
        let mappings = if is_historical {
            &state.data().historical_assets
        } else {
            &state.data().assets
        };

        // Calculate scrolling
        let visible_count = area.height.saturating_sub(2) as usize; // Account for borders
        let selected_idx = state.portfolio_profiles_state.selected_mapping_index;
        let scroll_offset =
            calculate_centered_scroll(selected_idx, unique_assets.len(), visible_count);

        let items: Vec<ListItem> = unique_assets
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_count)
            .map(|(idx, asset)| {
                let mapping = mappings.get(asset);
                let is_unmapped = mapping.is_none();
                let has_suggestion = is_unmapped && ticker_profiles::is_known_ticker(&asset.0);

                let mapping_str = if is_unmapped {
                    if has_suggestion {
                        "(unmapped) [?]" // Indicates suggestion available
                    } else {
                        "(unmapped)"
                    }
                } else {
                    mapping.map(|p| p.0.as_str()).unwrap_or("")
                };

                let style = if idx == selected_idx {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if has_suggestion {
                    // Unmapped with suggestion available - highlight in cyan
                    Style::default().fg(Color::Cyan)
                } else if is_unmapped {
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
            block = block.title_bottom(
                Line::from(" [m] Map [a] Suggest [A] All [Space] Collapse ").fg(Color::DarkGray),
            );
        }

        let list = List::new(items).block(block);

        frame.render_widget(list, area);
    }

    /// Render expanded tax & inflation config panel
    fn render_tax_inflation_config(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Config;
        let is_historical = state.data().parameters.returns_mode == ReturnsMode::Historical;

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

        // In Historical mode, inflation is always US Historical Bootstrap with same block size
        let inflation_desc = if is_historical {
            let block_size = state.data().parameters.historical_block_size;
            match block_size {
                Some(bs) => format!("US Historical Bootstrap (block={})", bs),
                None => "US Historical Bootstrap (i.i.d.)".to_string(),
            }
        } else {
            match &state.data().parameters.inflation {
                InflationData::None => "None (0%)".to_string(),
                InflationData::Fixed { rate } => format!("Fixed {}", format_percentage(*rate)),
                InflationData::Normal { mean, .. } => {
                    format!("Normal μ={}", format_percentage(*mean))
                }
                InflationData::LogNormal { mean, .. } => {
                    format!("LogN μ={}", format_percentage(*mean))
                }
                InflationData::USHistorical { distribution } => {
                    format!("US Historical ({:?})", distribution)
                }
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

    // ========== Key Handlers ==========

    fn handle_mappings_keys(&self, key: AppKeyEvent, state: &mut AppState) -> EventResult {
        let unique_assets = Self::get_unique_assets(state);
        let is_historical = state.data().parameters.returns_mode == ReturnsMode::Historical;
        let kb = &state.keybindings;

        // Space toggle for collapse - keep hardcoded as space bar is intuitive
        if key.code == KeyCode::Char(' ') {
            // Collapse both secondary panels and return to main panel
            state.portfolio_profiles_state.mappings_collapsed =
                !state.portfolio_profiles_state.mappings_collapsed;
            state.portfolio_profiles_state.config_collapsed =
                !state.portfolio_profiles_state.config_collapsed;
            return EventResult::Handled;
        }

        // Navigation: down
        if KeybindingsConfig::matches(&key, &kb.navigation.down) {
            if !unique_assets.is_empty() {
                state.portfolio_profiles_state.selected_mapping_index =
                    (state.portfolio_profiles_state.selected_mapping_index + 1)
                        % unique_assets.len();
            }
            return EventResult::Handled;
        }

        // Navigation: up
        if KeybindingsConfig::matches(&key, &kb.navigation.up) {
            if !unique_assets.is_empty() {
                if state.portfolio_profiles_state.selected_mapping_index == 0 {
                    state.portfolio_profiles_state.selected_mapping_index = unique_assets.len() - 1;
                } else {
                    state.portfolio_profiles_state.selected_mapping_index -= 1;
                }
            }
            return EventResult::Handled;
        }

        // Map action (m key or confirm/enter)
        if KeybindingsConfig::matches(&key, &kb.tabs.portfolio.map)
            || KeybindingsConfig::matches(&key, &kb.navigation.confirm)
        {
            // Map the selected asset to a profile
            if let Some(asset) =
                unique_assets.get(state.portfolio_profiles_state.selected_mapping_index)
            {
                if is_historical {
                    // Historical mode: cycle through preset profiles
                    let mappings = &state.data().historical_assets;
                    let current_mapping = mappings.get(asset);
                    let current_idx = current_mapping
                        .and_then(|tag| {
                            HISTORICAL_PRESETS
                                .iter()
                                .position(|(_, name, _)| *name == tag.0)
                        })
                        .unwrap_or(HISTORICAL_PRESETS.len());

                    let next_idx = if current_idx >= HISTORICAL_PRESETS.len() - 1 {
                        0
                    } else {
                        current_idx + 1
                    };

                    let (_, display_name, _) = HISTORICAL_PRESETS[next_idx];
                    let new_profile = ReturnProfileTag(display_name.to_string());
                    let asset_clone = asset.clone();
                    state
                        .data_mut()
                        .historical_assets
                        .insert(asset_clone, new_profile);
                    state.mark_modified();
                } else {
                    // Parametric mode: cycle through user-defined profiles
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
            }
            return EventResult::Handled;
        }

        // Suggest profile for selected asset (a key - uses portfolio.suggest)
        if KeybindingsConfig::matches(&key, &kb.tabs.portfolio.suggest) {
            if let Some(asset) =
                unique_assets.get(state.portfolio_profiles_state.selected_mapping_index)
            {
                if is_historical {
                    // Historical mode: check historical_assets
                    if state.data().historical_assets.contains_key(asset) {
                        state.set_error(format!("{} is already mapped", asset.0));
                        return EventResult::Handled;
                    }

                    // Look up historical suggestion
                    if let Some((_, display_name)) =
                        ticker_profiles::get_historical_suggestion(&asset.0)
                    {
                        let profile_tag = ReturnProfileTag(display_name.to_string());
                        state
                            .data_mut()
                            .historical_assets
                            .insert(asset.clone(), profile_tag);
                        state.mark_modified();
                    } else {
                        state.set_error(format!("No historical suggestion for {}", asset.0));
                    }
                } else {
                    // Parametric mode: check regular assets
                    if state.data().assets.contains_key(asset) {
                        state.set_error(format!("{} is already mapped", asset.0));
                        return EventResult::Handled;
                    }

                    // Look up suggestion
                    if let Some(suggestion) = ticker_profiles::get_suggestion(&asset.0) {
                        // Create profile if it doesn't exist
                        let profile_tag = ReturnProfileTag(suggestion.profile_name.to_string());
                        if !state.data().profiles.iter().any(|p| p.name == profile_tag) {
                            state.data_mut().profiles.push(ProfileData {
                                name: profile_tag.clone(),
                                description: Some(format!(
                                    "Auto-generated from ticker {}",
                                    asset.0
                                )),
                                profile: suggestion.profile_data.clone(),
                            });
                        }
                        // Add the mapping
                        state.data_mut().assets.insert(asset.clone(), profile_tag);
                        state.mark_modified();
                    } else {
                        state.set_error(format!("No suggestion available for {}", asset.0));
                    }
                }
            }
            return EventResult::Handled;
        }

        // Suggest profiles for ALL unmapped known tickers (Shift+A)
        if KeybindingsConfig::matches(&key, &kb.tabs.portfolio.suggest_all) {
            let mut suggestions_applied = 0;
            for asset in &unique_assets {
                if is_historical {
                    // Historical mode
                    if state.data().historical_assets.contains_key(asset) {
                        continue;
                    }
                    if let Some((_, display_name)) =
                        ticker_profiles::get_historical_suggestion(&asset.0)
                    {
                        let profile_tag = ReturnProfileTag(display_name.to_string());
                        state
                            .data_mut()
                            .historical_assets
                            .insert(asset.clone(), profile_tag);
                        suggestions_applied += 1;
                    }
                } else {
                    // Parametric mode
                    if state.data().assets.contains_key(asset) {
                        continue;
                    }
                    if let Some(suggestion) = ticker_profiles::get_suggestion(&asset.0) {
                        let profile_tag = ReturnProfileTag(suggestion.profile_name.to_string());
                        if !state.data().profiles.iter().any(|p| p.name == profile_tag) {
                            state.data_mut().profiles.push(ProfileData {
                                name: profile_tag.clone(),
                                description: Some(
                                    "Auto-generated from ticker suggestion".to_string(),
                                ),
                                profile: suggestion.profile_data.clone(),
                            });
                        }
                        state.data_mut().assets.insert(asset.clone(), profile_tag);
                        suggestions_applied += 1;
                    }
                }
            }
            if suggestions_applied > 0 {
                state.mark_modified();
                // Success - mappings will be visible immediately in the UI
            } else {
                state.set_error("No suggestions available for unmapped assets".to_string());
            }
            return EventResult::Handled;
        }

        EventResult::NotHandled
    }

    fn handle_config_keys(&self, key: AppKeyEvent, state: &mut AppState) -> EventResult {
        const CONFIG_ITEMS: usize = 4; // Federal, State, Cap Gains, Inflation
        let kb = &state.keybindings;

        // Space toggle for collapse - keep hardcoded as space bar is intuitive
        if key.code == KeyCode::Char(' ') {
            // Collapse both secondary panels and return to main panel
            state.portfolio_profiles_state.mappings_collapsed =
                !state.portfolio_profiles_state.mappings_collapsed;
            state.portfolio_profiles_state.config_collapsed =
                !state.portfolio_profiles_state.config_collapsed;
            return EventResult::Handled;
        }

        // Navigation: down
        if KeybindingsConfig::matches(&key, &kb.navigation.down) {
            state.portfolio_profiles_state.selected_config_index =
                (state.portfolio_profiles_state.selected_config_index + 1) % CONFIG_ITEMS;
            return EventResult::Handled;
        }

        // Navigation: up
        if KeybindingsConfig::matches(&key, &kb.navigation.up) {
            if state.portfolio_profiles_state.selected_config_index == 0 {
                state.portfolio_profiles_state.selected_config_index = CONFIG_ITEMS - 1;
            } else {
                state.portfolio_profiles_state.selected_config_index -= 1;
            }
            return EventResult::Handled;
        }

        // Edit action (e key or confirm/enter)
        if KeybindingsConfig::matches(&key, &kb.tabs.portfolio.edit)
            || KeybindingsConfig::matches(&key, &kb.navigation.confirm)
        {
            // Edit the selected config item
            match state.portfolio_profiles_state.selected_config_index {
                0 => {
                    // Federal Brackets - picker
                    let options = vec!["2024 Single".to_string(), "2024 Married Joint".to_string()];
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
                    // Inflation - picker for type (disabled in Historical mode)
                    let is_historical =
                        state.data().parameters.returns_mode == ReturnsMode::Historical;
                    if is_historical {
                        // In Historical mode, inflation is auto-set to US Historical Bootstrap
                        // Show info message instead of edit picker
                        state.modal = ModalState::Message(MessageModal::info(
                            "Inflation (Historical Mode)",
                            "In Historical mode, inflation is automatically set to US Historical Bootstrap sampling with the same block size as returns.\n\nSwitch to Parametric mode to customize inflation settings.",
                        ));
                    } else {
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
                }
                _ => {}
            }
            return EventResult::Handled;
        }

        EventResult::NotHandled
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

impl Component for PortfolioProfilesScreen {
    fn handle_key(&mut self, key: AppKeyEvent, state: &mut AppState) -> EventResult {
        // Holdings editing is now handled by AccountsPanel directly
        let kb = &state.keybindings;

        // Tab cycling through all panels: next_panel
        if KeybindingsConfig::matches(&key, &kb.navigation.next_panel) {
            state.portfolio_profiles_state.focused_panel =
                state.portfolio_profiles_state.focused_panel.next();
            return EventResult::Handled;
        }

        // Tab cycling through all panels: prev_panel
        if KeybindingsConfig::matches(&key, &kb.navigation.prev_panel) {
            state.portfolio_profiles_state.focused_panel =
                state.portfolio_profiles_state.focused_panel.prev();
            return EventResult::Handled;
        }

        // Tab-global: Toggle between Parametric and Historical mode
        if KeybindingsConfig::matches(&key, &kb.tabs.portfolio.history_mode) {
            let current_mode = state.data().parameters.returns_mode;
            let new_mode = match current_mode {
                ReturnsMode::Parametric => {
                    // Auto-map historical assets if switching to Historical for the first time
                    if state.data().historical_assets.is_empty() {
                        ProfilesPanel::auto_map_historical_assets(state);
                    }
                    ReturnsMode::Historical
                }
                ReturnsMode::Historical => ReturnsMode::Parametric,
            };
            state.data_mut().parameters.returns_mode = new_mode;
            state.portfolio_profiles_state.selected_profile_index = 0;
            state.mark_modified();
            return EventResult::Handled;
        }

        // Delegate to focused panel handler
        match state.portfolio_profiles_state.focused_panel {
            PortfolioProfilesPanel::Accounts => AccountsPanel::handle_key(key, state),
            PortfolioProfilesPanel::Profiles => ProfilesPanel::handle_key(key, state),
            PortfolioProfilesPanel::AssetMappings => self.handle_mappings_keys(key, state),
            PortfolioProfilesPanel::Config => self.handle_config_keys(key, state),
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

        AccountsPanel::render(frame, content_cols[0], state);
        ProfilesPanel::render(frame, content_cols[1], state);

        // Secondary panels at bottom
        self.render_secondary_panels(frame, main_layout[2], state);
    }
}

impl Screen for PortfolioProfilesScreen {
    fn title(&self) -> &str {
        "Portfolio & Profiles"
    }
}

impl super::ModalHandler for PortfolioProfilesScreen {
    fn handles(&self, action: &ModalAction) -> bool {
        matches!(
            action,
            ModalAction::Account(_)
                | ModalAction::Profile(_)
                | ModalAction::Holding(_)
                | ModalAction::Config(_)
        )
    }

    fn handle_modal_result(
        &self,
        state: &mut AppState,
        action: ModalAction,
        value: &crate::modals::ConfirmedValue,
    ) -> crate::actions::ActionResult {
        // Extract modal context FIRST (clone to break the borrow)
        let modal_context = match &state.modal {
            ModalState::Form(form) => form.context.clone(),
            ModalState::Confirm(confirm) => confirm.context.clone(),
            ModalState::Picker(picker) => picker.context.clone(),
            _ => None,
        };

        let ctx = ActionContext::new(modal_context.as_ref(), value);

        match action {
            // Account actions
            ModalAction::Account(AccountAction::PickCategory) => {
                actions::handle_category_pick(value.as_str().unwrap_or_default())
            }
            ModalAction::Account(AccountAction::PickType) => {
                actions::handle_type_pick(value.as_str().unwrap_or_default(), state)
            }
            ModalAction::Account(AccountAction::Create) => {
                actions::handle_create_account(state, ctx)
            }
            ModalAction::Account(AccountAction::Edit) => actions::handle_edit_account(state, ctx),
            ModalAction::Account(AccountAction::Delete) => {
                actions::handle_delete_account(state, ctx)
            }

            // Profile actions
            ModalAction::Profile(ProfileAction::PickType) => {
                actions::handle_profile_type_pick(value.as_str().unwrap_or_default())
            }
            ModalAction::Profile(ProfileAction::Create) => {
                actions::handle_create_profile(state, ctx)
            }
            ModalAction::Profile(ProfileAction::Edit) => actions::handle_edit_profile(state, ctx),
            ModalAction::Profile(ProfileAction::Delete) => {
                actions::handle_delete_profile(state, ctx)
            }
            ModalAction::Profile(ProfileAction::PickBlockSize) => {
                // Parse block size from picker selection
                let block_size = match value.as_str().unwrap_or_default() {
                    "1 (i.i.d. sampling)" => None,
                    "3 (short-term momentum)" => Some(3),
                    "5 (medium-term cycles)" => Some(5),
                    "10 (long-term trends)" => Some(10),
                    _ => None,
                };
                state.data_mut().parameters.historical_block_size = block_size;
                state.mark_modified();
                ActionResult::close()
            }

            // Holding actions
            ModalAction::Holding(HoldingAction::PickReturnProfile) => ActionResult::close(),
            ModalAction::Holding(HoldingAction::Add) => actions::handle_add_holding(state, ctx),
            ModalAction::Holding(HoldingAction::Edit) => actions::handle_edit_holding(state, ctx),
            ModalAction::Holding(HoldingAction::Delete) => {
                actions::handle_delete_holding(state, ctx)
            }

            // Config actions
            ModalAction::Config(ConfigAction::PickFederalBrackets) => {
                actions::handle_federal_brackets_pick(state, value.as_str().unwrap_or_default())
            }
            ModalAction::Config(ConfigAction::EditTax) => {
                actions::handle_edit_tax_config(state, ctx)
            }
            ModalAction::Config(ConfigAction::PickInflationType) => {
                actions::handle_inflation_type_pick(state, value.as_str().unwrap_or_default())
            }
            ModalAction::Config(ConfigAction::EditInflation) => {
                actions::handle_edit_inflation(state, ctx)
            }

            // This shouldn't happen if handles() is correct
            _ => ActionResult::close(),
        }
    }
}
