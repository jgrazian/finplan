use crate::actions::{self, ActionContext, ActionResult};
use crate::components::collapsible::CollapsiblePanel;
use crate::components::panels::EventListPanel;
use crate::components::{Component, EventResult};
use crate::data::events_data::{
    AmountData, EffectData, EventData, IntervalData, OffsetData, ThresholdData, TriggerData,
};
use crate::data::keybindings_data::KeybindingsConfig;
use crate::modals::{
    AmountAction, ConfirmedValue, EffectAction, EventAction, ModalAction, ModalContext, ModalState,
    context::TriggerContext,
};
use crate::state::{AppState, EventsPanel};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::Screen;

pub struct EventsScreen;

impl EventsScreen {
    fn render_event_details(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.events_state.focused_panel == EventsPanel::Details;

        let title = " EVENT DETAILS ";

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
            Self::append_trigger_details(&event.trigger, &mut lines, 1);
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "EFFECTS",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Green),
            )));

            if event.effects.is_empty() {
                lines.push(Line::from("  No effects"));
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
        let is_collapsed = state.events_state.timeline_collapsed;

        // Handle collapsed state
        if is_collapsed {
            let panel = CollapsiblePanel::new("TIMELINE", false).focused(is_focused);
            panel.render_collapsed(frame, area);
            return;
        }

        let indicator = "[-]";
        let title = format!(" {} TIMELINE ", indicator);

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
                "No events.",
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

        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);

        if is_focused {
            block = block.title_bottom(Line::from(" [Space] Toggle ").fg(Color::DarkGray));
        }

        let paragraph = Paragraph::new(lines)
            .block(block)
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

    /// Append detailed trigger information to lines for the details panel
    fn append_trigger_details(trigger: &TriggerData, lines: &mut Vec<Line<'_>>, indent: usize) {
        let prefix = "  ".repeat(indent);
        match trigger {
            TriggerData::Date { date } => {
                lines.push(Line::from(format!("{}Type: Date", prefix)));
                lines.push(Line::from(format!("{}Date: {}", prefix, date)));
            }
            TriggerData::Age { years, months } => {
                lines.push(Line::from(format!("{}Type: Age", prefix)));
                if let Some(m) = months {
                    lines.push(Line::from(format!(
                        "{}Age: {} years, {} months",
                        prefix, years, m
                    )));
                } else {
                    lines.push(Line::from(format!("{}Age: {} years", prefix, years)));
                }
            }
            TriggerData::RelativeToEvent { event, offset } => {
                lines.push(Line::from(format!("{}Type: Relative to Event", prefix)));
                lines.push(Line::from(format!("{}Event: \"{}\"", prefix, event.0)));
                lines.push(Line::from(format!(
                    "{}Offset: {}",
                    prefix,
                    Self::format_offset(offset)
                )));
            }
            TriggerData::AccountBalance { account, threshold } => {
                lines.push(Line::from(format!("{}Type: Account Balance", prefix)));
                lines.push(Line::from(format!("{}Account: \"{}\"", prefix, account.0)));
                lines.push(Line::from(format!(
                    "{}Threshold: {}",
                    prefix,
                    Self::format_threshold(threshold)
                )));
            }
            TriggerData::AssetBalance {
                account,
                asset,
                threshold,
            } => {
                lines.push(Line::from(format!("{}Type: Asset Balance", prefix)));
                lines.push(Line::from(format!("{}Account: \"{}\"", prefix, account.0)));
                lines.push(Line::from(format!("{}Asset: \"{}\"", prefix, asset.0)));
                lines.push(Line::from(format!(
                    "{}Threshold: {}",
                    prefix,
                    Self::format_threshold(threshold)
                )));
            }
            TriggerData::NetWorth { threshold } => {
                lines.push(Line::from(format!("{}Type: Net Worth", prefix)));
                lines.push(Line::from(format!(
                    "{}Threshold: {}",
                    prefix,
                    Self::format_threshold(threshold)
                )));
            }
            TriggerData::And { conditions } => {
                lines.push(Line::from(format!(
                    "{}Type: AND ({} conditions)",
                    prefix,
                    conditions.len()
                )));
                for (i, cond) in conditions.iter().enumerate() {
                    lines.push(Line::from(format!("{}Condition {}:", prefix, i + 1)));
                    Self::append_trigger_details(cond, lines, indent + 1);
                }
            }
            TriggerData::Or { conditions } => {
                lines.push(Line::from(format!(
                    "{}Type: OR ({} conditions)",
                    prefix,
                    conditions.len()
                )));
                for (i, cond) in conditions.iter().enumerate() {
                    lines.push(Line::from(format!("{}Condition {}:", prefix, i + 1)));
                    Self::append_trigger_details(cond, lines, indent + 1);
                }
            }
            TriggerData::Repeating {
                interval,
                start,
                end,
                max_occurrences,
            } => {
                lines.push(Line::from(format!("{}Type: Repeating", prefix)));
                lines.push(Line::from(format!(
                    "{}Interval: {}",
                    prefix,
                    Self::format_interval(interval)
                )));
                if let Some(start_trigger) = start {
                    lines.push(Line::from(vec![
                        Span::raw(format!("{}Start: ", prefix)),
                        Span::styled(
                            Self::format_trigger_inline(start_trigger),
                            Style::default().fg(Color::Green),
                        ),
                    ]));
                } else {
                    lines.push(Line::from(format!("{}Start: Immediately", prefix)));
                }
                if let Some(end_trigger) = end {
                    lines.push(Line::from(vec![
                        Span::raw(format!("{}End: ", prefix)),
                        Span::styled(
                            Self::format_trigger_inline(end_trigger),
                            Style::default().fg(Color::Red),
                        ),
                    ]));
                } else {
                    lines.push(Line::from(format!("{}End: Never", prefix)));
                }
                if let Some(max) = max_occurrences {
                    lines.push(Line::from(format!("{}Max occurrences: {}", prefix, max)));
                }
            }
            TriggerData::Manual => {
                lines.push(Line::from(format!("{}Type: Manual", prefix)));
                lines.push(Line::from(format!("{}(Triggered by other events)", prefix)));
            }
        }
    }

    /// Format a trigger in a single line for inline display
    fn format_trigger_inline(trigger: &TriggerData) -> String {
        match trigger {
            TriggerData::Date { date } => format!("Date {}", date),
            TriggerData::Age { years, months } => {
                if let Some(m) = months {
                    format!("Age {} years, {} months", years, m)
                } else {
                    format!("Age {} years", years)
                }
            }
            TriggerData::RelativeToEvent { event, offset } => {
                format!(
                    "Relative to \"{}\", {}",
                    event.0,
                    Self::format_offset(offset)
                )
            }
            TriggerData::AccountBalance { account, threshold } => {
                format!(
                    "Account \"{}\" {}",
                    account.0,
                    Self::format_threshold(threshold)
                )
            }
            TriggerData::AssetBalance {
                account,
                asset,
                threshold,
            } => {
                format!(
                    "Asset \"{}/{}\" {}",
                    account.0,
                    asset.0,
                    Self::format_threshold(threshold)
                )
            }
            TriggerData::NetWorth { threshold } => {
                format!("Net Worth {}", Self::format_threshold(threshold))
            }
            TriggerData::And { conditions } => format!("AND ({} conditions)", conditions.len()),
            TriggerData::Or { conditions } => format!("OR ({} conditions)", conditions.len()),
            TriggerData::Repeating { interval, .. } => {
                format!("Repeating {}", Self::format_interval(interval))
            }
            TriggerData::Manual => "Manual".to_string(),
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

    fn format_offset(offset: &OffsetData) -> String {
        match offset {
            OffsetData::Days { value } => {
                if *value == 0 {
                    "same day".to_string()
                } else if *value == 1 {
                    "1 day later".to_string()
                } else if *value == -1 {
                    "1 day before".to_string()
                } else if *value > 0 {
                    format!("{} days later", value)
                } else {
                    format!("{} days before", value.abs())
                }
            }
            OffsetData::Months { value } => {
                if *value == 0 {
                    "same month".to_string()
                } else if *value == 1 {
                    "1 month later".to_string()
                } else if *value == -1 {
                    "1 month before".to_string()
                } else if *value > 0 {
                    format!("{} months later", value)
                } else {
                    format!("{} months before", value.abs())
                }
            }
            OffsetData::Years { value } => {
                if *value == 0 {
                    "same year".to_string()
                } else if *value == 1 {
                    "1 year later".to_string()
                } else if *value == -1 {
                    "1 year before".to_string()
                } else if *value > 0 {
                    format!("{} years later", value)
                } else {
                    format!("{} years before", value.abs())
                }
            }
        }
    }

    fn format_threshold(threshold: &ThresholdData) -> String {
        match threshold {
            ThresholdData::GreaterThanOrEqual { value } => format!(">= ${:.2}", value),
            ThresholdData::LessThanOrEqual { value } => format!("<= ${:.2}", value),
        }
    }

    fn format_amount(amount: &AmountData) -> String {
        match amount {
            AmountData::Fixed { value } => format!("${:.2}", value),
            AmountData::InflationAdjusted { inner } => {
                format!("{} (inflation-adjusted)", Self::format_amount(inner))
            }
            AmountData::Scale { multiplier, inner } => {
                format!("{}% of {}", multiplier * 100.0, Self::format_amount(inner))
            }
            AmountData::SourceBalance => "Source Balance".to_string(),
            AmountData::ZeroTargetBalance => "Zero Target Balance".to_string(),
            AmountData::TargetToBalance { target } => format!("Target to ${:.2}", target),
            AmountData::AccountBalance { account } => {
                format!("\"{}\" Balance", account.0)
            }
            AmountData::AccountCashBalance { account } => {
                format!("\"{}\" Cash Balance", account.0)
            }
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
            EffectData::AdjustBalance { account, amount } => {
                format!(
                    "Adjust \"{}\" balance: {}",
                    account.0,
                    Self::format_amount(amount)
                )
            }
            EffectData::CashTransfer { from, to, amount } => {
                format!(
                    "Transfer from \"{}\" to \"{}\": {}",
                    from.0,
                    to.0,
                    Self::format_amount(amount)
                )
            }
            EffectData::Random {
                probability,
                on_true,
                on_false,
            } => {
                let prob_pct = (probability * 100.0) as u32;
                if let Some(on_false) = on_false {
                    format!(
                        "Random ({}%): \"{}\" / \"{}\"",
                        prob_pct, on_true.0, on_false.0
                    )
                } else {
                    format!("Random ({}%): \"{}\"", prob_pct, on_true.0)
                }
            }
        }
    }

    // ========== Key Handlers ==========

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

    /// Get accounts that can hold assets (investment accounts only)
    pub fn get_investment_account_names(state: &AppState) -> Vec<String> {
        state
            .data()
            .portfolios
            .accounts
            .iter()
            .filter(|a| a.account_type.can_hold_assets())
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
        // Panel navigation using configurable keybindings
        if KeybindingsConfig::matches(&key, &state.keybindings.navigation.next_panel) {
            state.events_state.focused_panel = state.events_state.focused_panel.next();
            return EventResult::Handled;
        }
        if KeybindingsConfig::matches(&key, &state.keybindings.navigation.prev_panel) {
            state.events_state.focused_panel = state.events_state.focused_panel.prev();
            return EventResult::Handled;
        }

        // Delegate to focused panel handler
        match state.events_state.focused_panel {
            EventsPanel::EventList => EventListPanel::handle_key(key, state),
            EventsPanel::Details => {
                // Details panel - navigation and forwarding
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
                    | KeyCode::Char('f') => EventListPanel::handle_key(key, state),
                    _ => EventResult::NotHandled,
                }
            }
            EventsPanel::Timeline => {
                // Timeline panel - navigation and collapse toggle
                match key.code {
                    KeyCode::Char(' ') => {
                        // Toggle timeline collapse
                        state.events_state.timeline_collapsed =
                            !state.events_state.timeline_collapsed;
                        EventResult::Handled
                    }
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
                    | KeyCode::Char('f') => EventListPanel::handle_key(key, state),
                    _ => EventResult::NotHandled,
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let timeline_collapsed = state.events_state.timeline_collapsed;

        // Dynamic constraints based on timeline collapse state
        let constraints = if timeline_collapsed {
            vec![
                Constraint::Percentage(30),
                Constraint::Percentage(67),
                Constraint::Length(5), // Collapsed timeline
            ]
        } else {
            vec![
                Constraint::Percentage(35),
                Constraint::Percentage(35),
                Constraint::Percentage(30),
            ]
        };

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);

        EventListPanel::render(frame, columns[0], state);
        self.render_event_details(frame, columns[1], state);
        self.render_timeline(frame, columns[2], state);
    }
}

impl Screen for EventsScreen {
    fn title(&self) -> &str {
        "Events"
    }
}

impl super::ModalHandler for EventsScreen {
    fn handles(&self, action: &ModalAction) -> bool {
        matches!(
            action,
            ModalAction::Event(_) | ModalAction::Effect(_) | ModalAction::Amount(_)
        )
    }

    fn handle_modal_result(
        &self,
        state: &mut AppState,
        action: ModalAction,
        value: &ConfirmedValue,
    ) -> ActionResult {
        // Extract modal context FIRST (clone to break the borrow)
        let modal_context = match &state.modal {
            ModalState::Form(form) => form.context.clone(),
            ModalState::Confirm(confirm) => confirm.context.clone(),
            ModalState::Picker(picker) => picker.context.clone(),
            _ => None,
        };

        let ctx = ActionContext::new(modal_context.as_ref(), value);

        match action {
            // Event actions
            ModalAction::Event(EventAction::PickTriggerType) => {
                actions::handle_trigger_type_pick(state, value.as_str().unwrap_or_default())
            }
            ModalAction::Event(EventAction::PickEventReference) => {
                // Check if we're editing a trigger or creating a new event
                if let Some(ModalContext::Trigger(TriggerContext::EditStart { event_index })) =
                    modal_context.as_ref()
                {
                    actions::handle_edit_event_reference(
                        *event_index,
                        value.as_str().unwrap_or_default(),
                    )
                } else {
                    actions::handle_event_reference_pick(value.as_str().unwrap_or_default())
                }
            }
            ModalAction::Event(EventAction::PickInterval) => {
                // Check if we're editing a trigger or creating a new event
                if let Some(ModalContext::Trigger(TriggerContext::EditStart { event_index })) =
                    modal_context.as_ref()
                {
                    actions::handle_edit_interval_pick(
                        *event_index,
                        value.as_str().unwrap_or_default(),
                    )
                } else {
                    actions::handle_interval_pick(value.as_str().unwrap_or_default())
                }
            }
            ModalAction::Event(EventAction::Create) => actions::handle_create_event(state, ctx),
            ModalAction::Event(EventAction::Edit) => actions::handle_edit_event(state, ctx),
            ModalAction::Event(EventAction::Delete) => actions::handle_delete_event(state, ctx),
            // Trigger builder actions
            ModalAction::Event(EventAction::PickChildTriggerType) => {
                actions::handle_pick_child_trigger_type(
                    state,
                    value.as_str().unwrap_or_default(),
                    ctx,
                )
            }
            ModalAction::Event(EventAction::BuildChildTrigger) => {
                actions::handle_build_child_trigger(state, value.as_str().unwrap_or_default(), ctx)
            }
            ModalAction::Event(EventAction::CompleteChildTrigger) => {
                actions::handle_complete_child_trigger(state, ctx)
            }
            ModalAction::Event(EventAction::FinalizeRepeating) => {
                actions::handle_finalize_repeating(state, ctx)
            }
            ModalAction::Event(EventAction::CreateRepeatingUnified) => {
                actions::handle_create_repeating_unified(state, ctx)
            }
            ModalAction::Event(EventAction::PickQuickEvent) => {
                actions::handle_quick_event_pick(state, value.as_str().unwrap_or_default())
            }
            // Trigger editing actions
            ModalAction::Event(EventAction::EditTriggerTypePick) => {
                actions::handle_edit_trigger_type_pick(
                    state,
                    value.as_str().unwrap_or_default(),
                    ctx,
                )
            }
            ModalAction::Event(EventAction::UpdateTrigger) => {
                actions::handle_update_trigger(state, ctx)
            }
            ModalAction::Event(EventAction::UpdateRepeating) => {
                actions::handle_update_repeating(state, ctx)
            }

            // Effect actions
            ModalAction::Effect(EffectAction::Manage) => {
                actions::handle_manage_effects(state, value.as_str().unwrap_or_default())
            }
            ModalAction::Effect(EffectAction::PickType) => ActionResult::close(),
            ModalAction::Effect(EffectAction::PickTypeForAdd) => {
                actions::handle_effect_type_for_add(state, value.as_str().unwrap_or_default())
            }
            ModalAction::Effect(EffectAction::PickAccountForEffect) => {
                // Check if we're editing a trigger or creating a new event/effect
                if let Some(ModalContext::Trigger(TriggerContext::EditStart { event_index })) =
                    modal_context.as_ref()
                {
                    actions::handle_edit_account_for_trigger(
                        *event_index,
                        value.as_str().unwrap_or_default(),
                    )
                } else {
                    actions::handle_account_for_effect_pick(value.as_str().unwrap_or_default())
                }
            }
            ModalAction::Effect(EffectAction::PickActionForEffect) => {
                actions::handle_action_for_effect_pick(
                    state,
                    value.as_str().unwrap_or_default(),
                    ctx,
                )
            }
            ModalAction::Effect(EffectAction::Add) => actions::handle_add_effect(state, ctx),
            ModalAction::Effect(EffectAction::Edit) => actions::handle_edit_effect(state, ctx),
            ModalAction::Effect(EffectAction::Delete) => actions::handle_delete_effect(state, ctx),

            // Amount actions (editing amounts within effect forms)
            ModalAction::Amount(AmountAction::PickType) => {
                actions::handle_amount_type_pick(state, value.as_str().unwrap_or_default(), ctx)
            }
            ModalAction::Amount(AmountAction::FixedForm) => {
                actions::handle_fixed_amount_form(state, ctx)
            }
            ModalAction::Amount(AmountAction::InflationForm) => {
                actions::handle_inflation_form(state, ctx)
            }
            ModalAction::Amount(AmountAction::ScaleForm) => actions::handle_scale_form(state, ctx),
            ModalAction::Amount(AmountAction::TargetForm) => {
                actions::handle_target_form(state, ctx)
            }
            ModalAction::Amount(AmountAction::AccountBalanceForm) => {
                actions::handle_account_balance_form(state, ctx)
            }
            ModalAction::Amount(AmountAction::CashBalanceForm) => {
                actions::handle_cash_balance_form(state, ctx)
            }

            // This shouldn't happen if handles() is correct
            _ => ActionResult::close(),
        }
    }
}
