//! Profiles panel component extracted from PortfolioProfilesScreen.
//!
//! Renders the unified profiles panel with list, details, and distribution chart.

use std::collections::HashSet;

use crate::components::EventResult;
use crate::components::charts::render_distribution;
use crate::components::lists::calculate_centered_scroll;
use crate::data::parameters_data::ReturnsMode;
use crate::data::portfolio_data::{AccountType, AssetTag};
use crate::data::profiles_data::{ProfileData, ReturnProfileData, ReturnProfileTag};
use crate::data::ticker_profiles::{self, HISTORICAL_PRESETS};
use crate::modals::context::ModalContext;
use crate::state::{
    AppState, ConfirmModal, FormField, FormModal, ModalAction, ModalState, PickerModal,
    PortfolioProfilesPanel,
};
use crate::util::format::format_percentage;
use crate::util::styles::focused_block_with_help;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    widgets::Wrap,
};

/// Profiles panel component.
pub struct ProfilesPanel;

impl ProfilesPanel {
    /// Render the unified profiles panel (list | details, distribution chart).
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused =
            state.portfolio_profiles_state.focused_panel == PortfolioProfilesPanel::Profiles;
        let is_historical = state.data().parameters.returns_mode == ReturnsMode::Historical;

        let title = if is_historical {
            " RETURN PROFILES (Historical) "
        } else {
            " RETURN PROFILES "
        };

        let help_text = if is_historical {
            " [b]lock size [h] parametric "
        } else {
            " [a]dd [e]dit [d]el [h]istorical [Shift+J/K] Reorder "
        };

        let block = focused_block_with_help(title, is_focused, help_text);
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

        Self::render_profile_list(frame, list_area, state, is_historical);
        Self::render_vertical_separator(frame, vsep_area, top_area.height);
        Self::render_profile_details(frame, details_area, state, is_historical);
        Self::render_horizontal_separator(frame, hsep_area, inner_area.width);
        Self::render_distribution_chart(frame, bottom_area, state, is_historical);
    }

    fn render_profile_list(frame: &mut Frame, area: Rect, state: &AppState, is_historical: bool) {
        let visible_count = area.height as usize;
        let selected_idx = state.portfolio_profiles_state.selected_profile_index;
        let mut lines = Vec::new();

        if is_historical {
            let scroll_offset =
                calculate_centered_scroll(selected_idx, HISTORICAL_PRESETS.len(), visible_count);

            for (idx, (_, display_name, _)) in HISTORICAL_PRESETS
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .take(visible_count)
            {
                let prefix = if idx == selected_idx { "> " } else { "  " };
                let max_name_len = area.width.saturating_sub(4) as usize;
                let name = if display_name.len() > max_name_len && max_name_len > 3 {
                    format!("{}...", &display_name[..max_name_len.saturating_sub(3)])
                } else {
                    display_name.to_string()
                };
                let content = format!("{}{}", prefix, name);

                let style = if idx == selected_idx {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                lines.push(Line::from(Span::styled(content, style)));
            }
        } else {
            let profiles = &state.data().profiles;
            let scroll_offset =
                calculate_centered_scroll(selected_idx, profiles.len(), visible_count);

            for (idx, profile_data) in profiles
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .take(visible_count)
            {
                let prefix = if idx == selected_idx { "> " } else { "  " };
                let max_name_len = area.width.saturating_sub(4) as usize;
                let name = if profile_data.name.0.len() > max_name_len && max_name_len > 3 {
                    format!(
                        "{}...",
                        &profile_data.name.0[..max_name_len.saturating_sub(3)]
                    )
                } else {
                    profile_data.name.0.clone()
                };
                let content = format!("{}{}", prefix, name);

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
                    "  No profiles.",
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(Span::styled(
                    "  Press 'a' to add.",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        let list_para = Paragraph::new(lines);
        frame.render_widget(list_para, area);
    }

    fn render_vertical_separator(frame: &mut Frame, area: Rect, height: u16) {
        let mut vsep_lines = Vec::new();
        for _ in 0..height {
            vsep_lines.push(Line::from(Span::styled(
                "│",
                Style::default().fg(Color::DarkGray),
            )));
        }
        let vsep = Paragraph::new(vsep_lines);
        frame.render_widget(vsep, area);
    }

    fn render_profile_details(
        frame: &mut Frame,
        area: Rect,
        state: &AppState,
        is_historical: bool,
    ) {
        let selected_idx = state.portfolio_profiles_state.selected_profile_index;

        let detail_lines = if is_historical {
            Self::render_historical_details(state, selected_idx)
        } else {
            Self::render_parametric_details(state, selected_idx)
        };

        let details_para = Paragraph::new(detail_lines).wrap(Wrap { trim: true });
        frame.render_widget(details_para, area);
    }

    fn render_historical_details(state: &AppState, selected_idx: usize) -> Vec<Line<'static>> {
        if let Some((preset_key, display_name, description)) = HISTORICAL_PRESETS.get(selected_idx)
        {
            let history = ReturnProfileData::get_historical_returns(preset_key);

            let block_size_str = match state.data().parameters.historical_block_size {
                Some(bs) => format!("{} years", bs),
                None => "i.i.d.".to_string(),
            };

            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(display_name.to_string()),
                ]),
                Line::from(vec![
                    Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled("Bootstrap", Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Data: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(description.to_string()),
                ]),
            ];

            if let Some(stats) = history.statistics() {
                lines.push(Line::from(vec![
                    Span::styled("Mean: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format_percentage(stats.arithmetic_mean),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(" (arith), "),
                    Span::styled(
                        format_percentage(stats.geometric_mean),
                        Style::default().fg(Color::Green),
                    ),
                    Span::raw(" (geom)"),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Std Dev: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format_percentage(stats.std_dev)),
                ]));
            }

            lines.push(Line::from(vec![
                Span::styled("Block: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(block_size_str, Style::default().fg(Color::Magenta)),
            ]));

            lines
        } else {
            vec![Line::from(Span::styled(
                "No profile selected",
                Style::default().fg(Color::DarkGray),
            ))]
        }
    }

    fn render_parametric_details(state: &AppState, selected_idx: usize) -> Vec<Line<'static>> {
        if let Some(profile_data) = state.data().profiles.get(selected_idx) {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(profile_data.name.0.clone()),
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
                ReturnProfileData::StudentT { mean, scale, df } => {
                    lines.push(Line::from(vec![
                        Span::styled("Mean: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format_percentage(*mean), Style::default().fg(Color::Yellow)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Scale: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(format_percentage(*scale)),
                    ]));
                    let tail_desc = if *df <= 2.5 {
                        "Very fat tails"
                    } else if *df <= 4.0 {
                        "Fat tails"
                    } else {
                        "Moderate tails"
                    };
                    lines.push(Line::from(vec![
                        Span::styled("Tails: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!("{} (df={:.0})", tail_desc, df),
                            Style::default().fg(Color::Magenta),
                        ),
                    ]));
                }
                ReturnProfileData::RegimeSwitching {
                    bull_mean,
                    bull_std_dev,
                    bear_mean,
                    bear_std_dev,
                    bull_to_bear_prob,
                    bear_to_bull_prob,
                } => {
                    lines.push(Line::from(vec![
                        Span::styled("Bull: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!(
                                "{} mean, {} std",
                                format_percentage(*bull_mean),
                                format_percentage(*bull_std_dev)
                            ),
                            Style::default().fg(Color::Green),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Bear: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!(
                                "{} mean, {} std",
                                format_percentage(*bear_mean),
                                format_percentage(*bear_std_dev)
                            ),
                            Style::default().fg(Color::Red),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled(
                            "Transitions: ",
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(format!(
                            "{}->bear, {}->bull",
                            format_percentage(*bull_to_bear_prob),
                            format_percentage(*bear_to_bull_prob)
                        )),
                    ]));
                }
                ReturnProfileData::Bootstrap { preset } => {
                    lines.push(Line::from(vec![
                        Span::styled("Preset: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(preset.clone(), Style::default().fg(Color::Cyan)),
                    ]));
                }
            }

            lines
        } else {
            vec![Line::from(Span::styled(
                "No profile selected",
                Style::default().fg(Color::DarkGray),
            ))]
        }
    }

    fn render_horizontal_separator(frame: &mut Frame, area: Rect, width: u16) {
        let sep_width = width as usize;
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
        frame.render_widget(hsep, area);
    }

    fn render_distribution_chart(
        frame: &mut Frame,
        area: Rect,
        state: &AppState,
        is_historical: bool,
    ) {
        // Center chart with ~20% padding on each side
        let padding = (area.width as f32 * 0.20) as u16;
        let chart_width = area.width.saturating_sub(padding * 2);
        let chart_area = Rect::new(area.x + padding, area.y, chart_width, area.height);

        let selected_idx = state.portfolio_profiles_state.selected_profile_index;

        if is_historical {
            if let Some((preset_key, _, _)) = HISTORICAL_PRESETS.get(selected_idx) {
                let profile = ReturnProfileData::Bootstrap {
                    preset: preset_key.to_string(),
                };
                render_distribution(frame, chart_area, &profile);
            } else {
                let msg = Paragraph::new("No profile selected")
                    .style(Style::default().fg(Color::DarkGray));
                frame.render_widget(msg, chart_area);
            }
        } else if let Some(profile_data) = state.data().profiles.get(selected_idx) {
            render_distribution(frame, chart_area, &profile_data.profile);
        } else {
            let msg =
                Paragraph::new("No profile selected").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, chart_area);
        }
    }

    /// Handle key events for the profiles panel.
    pub fn handle_key(key: KeyEvent, state: &mut AppState) -> EventResult {
        let is_historical = state.data().parameters.returns_mode == ReturnsMode::Historical;
        let list_len = if is_historical {
            HISTORICAL_PRESETS.len()
        } else {
            state.data().profiles.len()
        };
        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);

        match key.code {
            // Toggle between Parametric and Historical mode
            KeyCode::Char('h') => {
                let current_mode = state.data().parameters.returns_mode;
                let new_mode = match current_mode {
                    ReturnsMode::Parametric => {
                        if state.data().historical_assets.is_empty() {
                            Self::auto_map_historical_assets(state);
                        }
                        ReturnsMode::Historical
                    }
                    ReturnsMode::Historical => ReturnsMode::Parametric,
                };
                state.data_mut().parameters.returns_mode = new_mode;
                state.portfolio_profiles_state.selected_profile_index = 0;
                state.mark_modified();
                EventResult::Handled
            }
            // Block size picker (Historical mode only)
            KeyCode::Char('b') if is_historical => {
                let options = vec![
                    "1 (i.i.d. sampling)".to_string(),
                    "3 (short-term momentum)".to_string(),
                    "5 (medium-term cycles)".to_string(),
                    "10 (long-term trends)".to_string(),
                ];
                state.modal = ModalState::Picker(PickerModal::new(
                    "Select Block Size",
                    options,
                    ModalAction::PICK_BLOCK_SIZE,
                ));
                EventResult::Handled
            }
            // Move down (Shift+J or Shift+Down) - Parametric only
            KeyCode::Char('J') if has_shift && !is_historical => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if list_len >= 2 && idx < list_len - 1 {
                    state.data_mut().profiles.swap(idx, idx + 1);
                    state.portfolio_profiles_state.selected_profile_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Down if has_shift && !is_historical => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if list_len >= 2 && idx < list_len - 1 {
                    state.data_mut().profiles.swap(idx, idx + 1);
                    state.portfolio_profiles_state.selected_profile_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            // Move up (Shift+K or Shift+Up) - Parametric only
            KeyCode::Char('K') if has_shift && !is_historical => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if list_len >= 2 && idx > 0 {
                    state.data_mut().profiles.swap(idx, idx - 1);
                    state.portfolio_profiles_state.selected_profile_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Up if has_shift && !is_historical => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if list_len >= 2 && idx > 0 {
                    state.data_mut().profiles.swap(idx, idx - 1);
                    state.portfolio_profiles_state.selected_profile_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if list_len > 0 {
                    state.portfolio_profiles_state.selected_profile_index =
                        (state.portfolio_profiles_state.selected_profile_index + 1) % list_len;
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if list_len > 0 {
                    if state.portfolio_profiles_state.selected_profile_index == 0 {
                        state.portfolio_profiles_state.selected_profile_index = list_len - 1;
                    } else {
                        state.portfolio_profiles_state.selected_profile_index -= 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Char('a') if !is_historical => {
                let types = vec![
                    "None".to_string(),
                    "Fixed Rate".to_string(),
                    "Normal Distribution".to_string(),
                    "Log-Normal Distribution".to_string(),
                    "Student's t Distribution".to_string(),
                    "Regime Switching (Normal)".to_string(),
                    "Regime Switching (Student-t)".to_string(),
                ];
                state.modal = ModalState::Picker(PickerModal::new(
                    "Select Profile Type",
                    types,
                    ModalAction::PICK_PROFILE_TYPE,
                ));
                EventResult::Handled
            }
            KeyCode::Char('e') if !is_historical => {
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
            KeyCode::Char('d') if !is_historical => {
                if let Some(profile_data) = state
                    .data()
                    .profiles
                    .get(state.portfolio_profiles_state.selected_profile_index)
                {
                    state.modal = ModalState::Confirm(
                        ConfirmModal::new(
                            "Delete Profile",
                            &format!(
                                "Delete profile '{}'?\n\nThis cannot be undone.",
                                profile_data.name.0
                            ),
                            ModalAction::DELETE_PROFILE,
                        )
                        .with_typed_context(ModalContext::profile_index(
                            state.portfolio_profiles_state.selected_profile_index,
                        )),
                    );
                }
                EventResult::Handled
            }
            // Preset shortcuts (Parametric only)
            KeyCode::Char('1') if !is_historical => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::Fixed { rate: 0.095668 };
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('2') if !is_historical => {
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
            KeyCode::Char('3') if !is_historical => {
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
            KeyCode::Char('4') if !is_historical => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::None;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('5') if !is_historical => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::StudentT {
                        mean: 0.11471,
                        scale: 0.140558,
                        df: 5.0,
                    };
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('6') if !is_historical => {
                let idx = state.portfolio_profiles_state.selected_profile_index;
                if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
                    profile_data.profile = ReturnProfileData::RegimeSwitching {
                        bull_mean: 0.12,
                        bull_std_dev: 0.12,
                        bear_mean: -0.05,
                        bear_std_dev: 0.22,
                        bull_to_bear_prob: 0.15,
                        bear_to_bull_prob: 0.40,
                    };
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char(' ') => {
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

    // ========== Helper Functions ==========

    /// Extract all unique assets from investment accounts.
    pub fn get_unique_assets(state: &AppState) -> Vec<AssetTag> {
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

    /// Auto-map assets to historical profiles based on ticker suggestions.
    pub fn auto_map_historical_assets(state: &mut AppState) {
        let unique_assets = Self::get_unique_assets(state);
        let data = state.data_mut();

        for asset in unique_assets {
            if let Some((_, display_name)) = ticker_profiles::get_historical_suggestion(&asset.0) {
                data.historical_assets
                    .insert(asset, ReturnProfileTag(display_name.to_string()));
            }
        }
    }

    /// Format profile type as a display string.
    fn format_profile_type(profile: &ReturnProfileData) -> String {
        match profile {
            ReturnProfileData::None => "None".to_string(),
            ReturnProfileData::Fixed { .. } => "Fixed Rate".to_string(),
            ReturnProfileData::Normal { .. } => "Normal Distribution".to_string(),
            ReturnProfileData::LogNormal { .. } => "Log-Normal Distribution".to_string(),
            ReturnProfileData::StudentT { .. } => "Student's t Distribution".to_string(),
            ReturnProfileData::RegimeSwitching { .. } => "Regime Switching".to_string(),
            ReturnProfileData::Bootstrap { .. } => "Bootstrap (Historical)".to_string(),
        }
    }

    /// Create profile edit form for the given profile data.
    pub fn create_profile_edit_form(profile_data: &ProfileData) -> FormModal {
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
                    FormField::percentage("Mean Return", *mean),
                    FormField::percentage("Std Deviation", *std_dev),
                ],
                ModalAction::EDIT_PROFILE,
            ),
            ReturnProfileData::StudentT { mean, scale, df } => FormModal::new(
                "Edit Profile",
                vec![
                    FormField::text("Name", &profile_data.name.0),
                    FormField::text(
                        "Description",
                        profile_data.description.as_deref().unwrap_or(""),
                    ),
                    FormField::read_only("Type", &type_name),
                    FormField::percentage("Mean Return", *mean),
                    FormField::percentage("Scale", *scale),
                    FormField::text("Degrees of Freedom", &format!("{:.1}", df)),
                ],
                ModalAction::EDIT_PROFILE,
            ),
            ReturnProfileData::RegimeSwitching {
                bull_mean,
                bull_std_dev,
                bear_mean,
                bear_std_dev,
                bull_to_bear_prob,
                bear_to_bull_prob,
            } => FormModal::new(
                "Edit Profile",
                vec![
                    FormField::text("Name", &profile_data.name.0),
                    FormField::text(
                        "Description",
                        profile_data.description.as_deref().unwrap_or(""),
                    ),
                    FormField::read_only("Type", &type_name),
                    FormField::percentage("Bull Mean", *bull_mean),
                    FormField::percentage("Bull Std Dev", *bull_std_dev),
                    FormField::percentage("Bear Mean", *bear_mean),
                    FormField::percentage("Bear Std Dev", *bear_std_dev),
                    FormField::percentage("Bull→Bear Prob", *bull_to_bear_prob),
                    FormField::percentage("Bear→Bull Prob", *bear_to_bull_prob),
                ],
                ModalAction::EDIT_PROFILE,
            ),
            ReturnProfileData::Bootstrap { preset } => FormModal::new(
                "Edit Profile",
                vec![
                    FormField::text("Name", &profile_data.name.0),
                    FormField::text(
                        "Description",
                        profile_data.description.as_deref().unwrap_or(""),
                    ),
                    FormField::read_only("Type", &type_name),
                    FormField::read_only("Preset", preset),
                ],
                ModalAction::EDIT_PROFILE,
            ),
        }
    }
}
