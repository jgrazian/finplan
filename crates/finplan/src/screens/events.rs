use crate::components::{Component, EventResult};
use crate::data::events_data::{
    AmountData, EffectData, EventData, EventTag, IntervalData, SpecialAmount, TriggerData,
};
use crate::state::{
    AppState, ConfirmModal, EventsPanel, FormField, FormModal, ModalAction, ModalState,
    PickerModal,
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::Screen;

pub struct EventsScreen;

impl EventsScreen {
    pub fn new() -> Self {
        Self
    }

    fn render_event_list(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.events_state.focused_panel == EventsPanel::EventList;

        let items: Vec<ListItem> = if state.data().events.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "(no events configured)",
                Style::default().fg(Color::DarkGray),
            )))]
        } else {
            state
                .data()
                .events
                .iter()
                .enumerate()
                .map(|(idx, event)| {
                    let enabled_prefix = if event.enabled { "[✓]" } else { "[x]" };
                    let event_desc = Self::format_event_summary(event);
                    let content = format!("{} {}: {}", enabled_prefix, event.name.0, event_desc);

                    let base_style = if !event.enabled {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default()
                    };

                    let style = if idx == state.events_state.selected_event_index {
                        base_style.fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        base_style
                    };

                    ListItem::new(Line::from(Span::styled(content, style)))
                })
                .collect()
        };

        let title = if is_focused {
            " EVENTS [FOCUSED] "
        } else {
            " EVENTS "
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

    fn render_event_details(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.events_state.focused_panel == EventsPanel::Details;

        let title = if is_focused {
            " EVENT DETAILS [FOCUSED] "
        } else {
            " EVENT DETAILS "
        };

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let content = if state.data().events.is_empty() {
            vec![
                Line::from("No events configured."),
                Line::from(""),
                Line::from("Events control simulation behavior:"),
                Line::from("  • Income and expenses"),
                Line::from("  • Asset purchases and sales"),
                Line::from("  • Account transfers"),
                Line::from("  • RMD calculations"),
                Line::from(""),
                Line::from("Press [a] to add an event."),
            ]
        } else if let Some(event) = state
            .data()
            .events
            .get(state.events_state.selected_event_index)
        {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&event.name.0),
                ]),
                Line::from(""),
            ];

            if let Some(desc) = &event.description {
                lines.push(Line::from(vec![
                    Span::styled(
                        "Description: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(desc),
                ]));
                lines.push(Line::from(""));
            }

            // Show enabled status
            let enabled_text = if event.enabled { "Yes" } else { "No" };
            let enabled_color = if event.enabled {
                Color::Green
            } else {
                Color::Red
            };
            lines.push(Line::from(vec![
                Span::styled("Enabled: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(enabled_text, Style::default().fg(enabled_color)),
            ]));

            lines.push(Line::from(vec![
                Span::styled("Once Only: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(if event.once { "Yes" } else { "No" }),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "TRIGGER",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Cyan),
            )));
            lines.push(Line::from(Self::format_trigger(&event.trigger)));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "EFFECTS",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Green),
            )));

            if event.effects.is_empty() {
                lines.push(Line::from("  (no effects)"));
            } else {
                for (i, effect) in event.effects.iter().enumerate() {
                    lines.push(Line::from(format!(
                        "  {}. {}",
                        i + 1,
                        Self::format_effect(effect)
                    )));
                }
            }

            lines
        } else {
            vec![Line::from("No event selected")]
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

    fn render_timeline(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.events_state.focused_panel == EventsPanel::Timeline;

        let title = if is_focused {
            " TIMELINE [FOCUSED] "
        } else {
            " TIMELINE "
        };

        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let events = &state.data().events;
        let birth_date = &state.data().parameters.birth_date;

        // Collect timeline entries with calculated years
        let mut timeline_entries: Vec<(Option<i32>, &EventData, bool)> = events
            .iter()
            .map(|event| {
                let year = Self::calculate_trigger_year(&event.trigger, birth_date);
                let is_repeating = matches!(event.trigger, TriggerData::Repeating { .. });
                (year, event, is_repeating)
            })
            .collect();

        // Sort by year (None/conditional events go last)
        timeline_entries.sort_by(|a, b| match (a.0, b.0) {
            (Some(y1), Some(y2)) => y1.cmp(&y2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        let mut lines: Vec<Line> = Vec::new();

        if events.is_empty() {
            lines.push(Line::from(Span::styled(
                "(no events)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            // Filter to only dated events first
            let dated_entries: Vec<_> = timeline_entries
                .iter()
                .filter(|(y, _, _)| y.is_some())
                .collect();

            let mut last_year: Option<i32> = None;

            for (i, (year_opt, event, is_repeating)) in dated_entries.iter().enumerate() {
                let is_first = i == 0;
                let is_last = i == dated_entries.len() - 1;

                let year_str = match year_opt {
                    Some(y) => format!("{:4}", y),
                    None => "    ".to_string(),
                };

                // Tree connector: ┬ for first (if more follow), ├ for middle, └ for last
                let connector = if is_first && !is_last {
                    "┬─"
                } else if is_last {
                    "└─"
                } else {
                    "├─"
                };

                // Show year only if different from previous
                let display_year = if *year_opt != last_year {
                    year_str
                } else {
                    "    ".to_string()
                };
                last_year = *year_opt;

                let repeat_symbol = if *is_repeating { " ↻" } else { " ○" };
                let enabled_indicator = if event.enabled { " [✓]" } else { " [x]" };

                let style = if !event.enabled {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default()
                };

                // Format: "2025 ├─ Event Name ○ [✓]"
                let line_content = format!(
                    "{} {} {}{}{}",
                    display_year, connector, event.name.0, repeat_symbol, enabled_indicator
                );

                lines.push(Line::from(Span::styled(line_content, style)));

                // Add vertical connector line between entries (unless last)
                if !is_last {
                    lines.push(Line::from("     │"));
                }
            }

            // Add conditional events section if any
            let conditional: Vec<_> = timeline_entries
                .iter()
                .filter(|(y, _, _)| y.is_none())
                .collect();
            if !conditional.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Conditional:",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                )));
                for (_, event, is_repeating) in conditional {
                    let repeat_symbol = if *is_repeating { "↻" } else { "○" };
                    let enabled_indicator = if event.enabled { "[✓]" } else { "[x]" };
                    let style = if !event.enabled {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default()
                    };
                    lines.push(Line::from(Span::styled(
                        format!("  {} {} {}", repeat_symbol, event.name.0, enabled_indicator),
                        style,
                    )));
                }
            }

            // Legend
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "○ One-time  ↻ Repeating",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "[✓] Enabled  [x] Disabled",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(border_style),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }

    /// Calculate the year a trigger will first fire (if determinable)
    fn calculate_trigger_year(trigger: &TriggerData, birth_date: &str) -> Option<i32> {
        match trigger {
            TriggerData::Date { date } => {
                // Parse "YYYY-MM-DD" format
                date.split('-').next()?.parse().ok()
            }
            TriggerData::Age { years, .. } => {
                // Calculate birth year + age
                let birth_year: i32 = birth_date.split('-').next()?.parse().ok()?;
                Some(birth_year + *years as i32)
            }
            TriggerData::Repeating { start, .. } => {
                // Use start condition if present
                if let Some(start_trigger) = start {
                    Self::calculate_trigger_year(start_trigger, birth_date)
                } else {
                    // Starts immediately - use current year estimation
                    Some(2025)
                }
            }
            TriggerData::RelativeToEvent { .. } => {
                // Would need to resolve reference - mark as conditional for now
                None
            }
            TriggerData::AccountBalance { .. }
            | TriggerData::AssetBalance { .. }
            | TriggerData::NetWorth { .. }
            | TriggerData::Manual
            | TriggerData::And { .. }
            | TriggerData::Or { .. } => {
                // Conditional triggers - no fixed year
                None
            }
        }
    }

    fn format_event_summary(event: &EventData) -> String {
        let trigger_type = match &event.trigger {
            TriggerData::Date { .. } => "Date",
            TriggerData::Age { .. } => "Age",
            TriggerData::RelativeToEvent { .. } => "Relative",
            TriggerData::AccountBalance { .. } => "AcctBal",
            TriggerData::AssetBalance { .. } => "AssetBal",
            TriggerData::NetWorth { .. } => "NetWorth",
            TriggerData::And { .. } => "And",
            TriggerData::Or { .. } => "Or",
            TriggerData::Repeating { .. } => "Repeating",
            TriggerData::Manual => "Manual",
        };

        let effect_count = event.effects.len();
        format!(
            "{} ({})",
            trigger_type,
            Self::plural(effect_count, "effect")
        )
    }

    fn format_trigger(trigger: &TriggerData) -> String {
        match trigger {
            TriggerData::Date { date } => format!("  Date: {}", date),
            TriggerData::Age { years, months } => {
                if let Some(m) = months {
                    format!("  Age: {} years, {} months", years, m)
                } else {
                    format!("  Age: {} years", years)
                }
            }
            TriggerData::RelativeToEvent { event, offset } => {
                format!("  Relative to \"{}\": {:?}", event.0, offset)
            }
            TriggerData::AccountBalance { account, threshold } => {
                format!("  Account \"{}\" balance {:?}", account.0, threshold)
            }
            TriggerData::AssetBalance {
                account,
                asset,
                threshold,
            } => {
                format!(
                    "  Asset \"{}\" in \"{}\" {:?}",
                    asset.0, account.0, threshold
                )
            }
            TriggerData::NetWorth { threshold } => {
                format!("  Net worth {:?}", threshold)
            }
            TriggerData::And { conditions } => {
                format!("  AND ({} conditions)", conditions.len())
            }
            TriggerData::Or { conditions } => {
                format!("  OR ({} conditions)", conditions.len())
            }
            TriggerData::Repeating {
                interval,
                start,
                end,
            } => {
                let mut desc = format!("  Repeating: {}", Self::format_interval(interval));
                if start.is_some() {
                    desc.push_str(" (with start condition)");
                }
                if end.is_some() {
                    desc.push_str(" (with end condition)");
                }
                desc
            }
            TriggerData::Manual => "  Manual (triggered by other events)".to_string(),
        }
    }

    fn format_interval(interval: &IntervalData) -> &'static str {
        match interval {
            IntervalData::Never => "Never",
            IntervalData::Weekly => "Weekly",
            IntervalData::BiWeekly => "Bi-Weekly",
            IntervalData::Monthly => "Monthly",
            IntervalData::Quarterly => "Quarterly",
            IntervalData::Yearly => "Yearly",
        }
    }

    fn format_amount(amount: &AmountData) -> String {
        match amount {
            AmountData::Fixed(val) => format!("${:.2}", val),
            AmountData::Special(special) => match special {
                SpecialAmount::SourceBalance => "Source Balance".to_string(),
                SpecialAmount::ZeroTargetBalance => "Zero Target Balance".to_string(),
                SpecialAmount::TargetToBalance { target } => format!("Target to ${:.2}", target),
                SpecialAmount::AccountBalance { account } => {
                    format!("\"{}\" Balance", account.0)
                }
                SpecialAmount::AccountCashBalance { account } => {
                    format!("\"{}\" Cash Balance", account.0)
                }
            },
        }
    }

    pub fn format_effect(effect: &EffectData) -> String {
        match effect {
            EffectData::Income {
                to,
                amount,
                gross,
                taxable,
            } => {
                let mode = if *gross { "gross" } else { "net" };
                let tax = if *taxable { "taxable" } else { "non-taxable" };
                format!(
                    "Income to \"{}\": {} ({}, {})",
                    to.0,
                    Self::format_amount(amount),
                    mode,
                    tax
                )
            }
            EffectData::Expense { from, amount } => {
                format!(
                    "Expense from \"{}\": {}",
                    from.0,
                    Self::format_amount(amount)
                )
            }
            EffectData::AssetPurchase {
                from,
                to_account,
                asset,
                amount,
            } => {
                format!(
                    "Purchase \"{}\" in \"{}\" from \"{}\": {}",
                    asset.0,
                    to_account.0,
                    from.0,
                    Self::format_amount(amount)
                )
            }
            EffectData::AssetSale {
                from,
                asset,
                amount,
                ..
            } => {
                if let Some(a) = asset {
                    format!(
                        "Sell \"{}\" from \"{}\": {}",
                        a.0,
                        from.0,
                        Self::format_amount(amount)
                    )
                } else {
                    format!(
                        "Liquidate from \"{}\": {}",
                        from.0,
                        Self::format_amount(amount)
                    )
                }
            }
            EffectData::Sweep { to, amount, .. } => {
                format!("Sweep to \"{}\": {}", to.0, Self::format_amount(amount))
            }
            EffectData::TriggerEvent { event } => format!("Trigger \"{}\"", event.0),
            EffectData::PauseEvent { event } => format!("Pause \"{}\"", event.0),
            EffectData::ResumeEvent { event } => format!("Resume \"{}\"", event.0),
            EffectData::TerminateEvent { event } => format!("Terminate \"{}\"", event.0),
            EffectData::ApplyRmd { destination, .. } => {
                format!("Apply RMD to \"{}\"", destination.0)
            }
        }
    }

    fn plural(count: usize, word: &str) -> String {
        if count == 1 {
            format!("{} {}", count, word)
        } else {
            format!("{} {}s", count, word)
        }
    }

    // ========== Key Handlers ==========

    fn handle_event_list_keys(&self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let events_len = state.data().events.len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if events_len > 0 {
                    state.events_state.selected_event_index =
                        (state.events_state.selected_event_index + 1) % events_len;
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if events_len > 0 {
                    if state.events_state.selected_event_index == 0 {
                        state.events_state.selected_event_index = events_len - 1;
                    } else {
                        state.events_state.selected_event_index -= 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Char('t') => {
                // Toggle enabled status
                let idx = state.events_state.selected_event_index;
                if let Some(event) = state.data_mut().events.get_mut(idx) {
                    event.enabled = !event.enabled;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('a') => {
                // Add new event - show trigger type picker
                let trigger_types = vec![
                    "Date".to_string(),
                    "Age".to_string(),
                    "Repeating".to_string(),
                    "Manual".to_string(),
                    "Account Balance".to_string(),
                    "Net Worth".to_string(),
                    "Relative to Event".to_string(),
                ];
                state.modal = ModalState::Picker(PickerModal::new(
                    "Select Trigger Type",
                    trigger_types,
                    ModalAction::PickTriggerType,
                ));
                EventResult::Handled
            }
            KeyCode::Char('e') => {
                // Edit selected event
                if let Some(event) = state
                    .data()
                    .events
                    .get(state.events_state.selected_event_index)
                {
                    let trigger_summary = Self::format_trigger_short(&event.trigger);
                    let effects_summary = format!("{} effect(s)", event.effects.len());

                    let form = FormModal::new(
                        "Edit Event",
                        vec![
                            FormField::text("Name", &event.name.0),
                            FormField::text(
                                "Description",
                                event.description.as_deref().unwrap_or(""),
                            ),
                            FormField::text("Once Only (Y/N)", if event.once { "Y" } else { "N" }),
                            FormField::text(
                                "Enabled (Y/N)",
                                if event.enabled { "Y" } else { "N" },
                            ),
                            FormField::read_only("Trigger", &trigger_summary),
                            FormField::read_only("Effects", &effects_summary),
                        ],
                        ModalAction::EditEvent,
                    )
                    .with_context(&state.events_state.selected_event_index.to_string());

                    state.modal = ModalState::Form(form);
                }
                EventResult::Handled
            }
            KeyCode::Char('d') => {
                // Delete selected event with confirmation
                if let Some(event) = state
                    .data()
                    .events
                    .get(state.events_state.selected_event_index)
                {
                    state.modal = ModalState::Confirm(
                        ConfirmModal::new(
                            "Delete Event",
                            &format!("Delete event '{}'?", event.name.0),
                            ModalAction::DeleteEvent,
                        )
                        .with_context(&state.events_state.selected_event_index.to_string()),
                    );
                }
                EventResult::Handled
            }
            KeyCode::Char('c') => {
                // Copy selected event
                let idx = state.events_state.selected_event_index;
                if let Some(event) = state.data().events.get(idx).cloned() {
                    let mut new_event = event;
                    new_event.name = EventTag(format!("{} (Copy)", new_event.name.0));
                    state.data_mut().events.push(new_event);
                    // Select the newly copied event
                    state.events_state.selected_event_index = state.data().events.len() - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('f') => {
                // Manage effects for selected event
                let event_idx = state.events_state.selected_event_index;
                if let Some(event) = state.data().events.get(event_idx) {
                    // Build list of current effects + add option
                    let mut options: Vec<String> = event
                        .effects
                        .iter()
                        .enumerate()
                        .map(|(i, effect)| format!("{}. {}", i + 1, Self::format_effect(effect)))
                        .collect();
                    options.push("[ + Add New Effect ]".to_string());

                    state.modal = ModalState::Picker(PickerModal::new(
                        &format!("Manage Effects - {}", event.name.0),
                        options,
                        ModalAction::ManageEffects,
                    ));
                }
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    fn format_trigger_short(trigger: &TriggerData) -> String {
        match trigger {
            TriggerData::Date { date } => format!("Date: {}", date),
            TriggerData::Age { years, .. } => format!("Age: {}", years),
            TriggerData::Repeating { interval, .. } => {
                format!("Repeating: {}", Self::format_interval(interval))
            }
            TriggerData::Manual => "Manual".to_string(),
            TriggerData::AccountBalance { account, .. } => format!("Acct Bal: {}", account.0),
            TriggerData::AssetBalance { account, asset, .. } => {
                format!("Asset Bal: {}/{}", account.0, asset.0)
            }
            TriggerData::NetWorth { .. } => "Net Worth".to_string(),
            TriggerData::RelativeToEvent { event, .. } => format!("Relative: {}", event.0),
            TriggerData::And { conditions } => format!("AND ({})", conditions.len()),
            TriggerData::Or { conditions } => format!("OR ({})", conditions.len()),
        }
    }

    /// Get available accounts for effects
    pub fn get_account_names(state: &AppState) -> Vec<String> {
        state
            .data()
            .portfolios
            .accounts
            .iter()
            .map(|a| a.name.clone())
            .collect()
    }

    /// Get available event names for references
    pub fn get_event_names(state: &AppState) -> Vec<String> {
        state
            .data()
            .events
            .iter()
            .map(|e| e.name.0.clone())
            .collect()
    }
}

impl Component for EventsScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        match key.code {
            // Tab cycling through panels
            KeyCode::Tab if key.modifiers.is_empty() => {
                state.events_state.focused_panel = state.events_state.focused_panel.next();
                EventResult::Handled
            }
            KeyCode::BackTab => {
                state.events_state.focused_panel = state.events_state.focused_panel.prev();
                EventResult::Handled
            }
            _ => {
                // Delegate to focused panel handler
                match state.events_state.focused_panel {
                    EventsPanel::EventList => self.handle_event_list_keys(key, state),
                    EventsPanel::Details | EventsPanel::Timeline => {
                        // Details and Timeline panels - only navigation, actions on event list
                        match key.code {
                            KeyCode::Char('j') | KeyCode::Down => {
                                let events_len = state.data().events.len();
                                if events_len > 0 {
                                    state.events_state.selected_event_index =
                                        (state.events_state.selected_event_index + 1) % events_len;
                                }
                                EventResult::Handled
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                let events_len = state.data().events.len();
                                if events_len > 0 {
                                    if state.events_state.selected_event_index == 0 {
                                        state.events_state.selected_event_index = events_len - 1;
                                    } else {
                                        state.events_state.selected_event_index -= 1;
                                    }
                                }
                                EventResult::Handled
                            }
                            // Allow t/a/e/d/c/f even when not on event list
                            KeyCode::Char('t')
                            | KeyCode::Char('a')
                            | KeyCode::Char('e')
                            | KeyCode::Char('d')
                            | KeyCode::Char('c')
                            | KeyCode::Char('f') => self.handle_event_list_keys(key, state),
                            _ => EventResult::NotHandled,
                        }
                    }
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

        self.render_event_list(frame, columns[0], state);
        self.render_event_details(frame, columns[1], state);
        self.render_timeline(frame, columns[2], state);
    }
}

impl Screen for EventsScreen {
    fn title(&self) -> &str {
        "Events"
    }
}
