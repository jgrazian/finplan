use crate::components::collapsible::CollapsiblePanel;
use crate::components::{Component, EventResult};
use crate::data::events_data::{
    AmountData, EffectData, EventData, EventTag, IntervalData, OffsetData, SpecialAmount,
    ThresholdData, TriggerData,
};
use crate::state::context::ModalContext;
use crate::state::{
    AppState, ConfirmModal, EventsPanel, FormField, FormModal, ModalAction, ModalState, PickerModal,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::Screen;

pub struct EventsScreen;

impl EventsScreen {
    fn render_event_list(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.events_state.focused_panel == EventsPanel::EventList;

        let items: Vec<ListItem> = if state.data().events.is_empty() {
            vec![
                ListItem::new(Line::from(Span::styled(
                    "No events configured.",
                    Style::default().fg(Color::DarkGray),
                ))),
                ListItem::new(Line::from(Span::styled(
                    "Press 'a' to add.",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        } else {
            state
                .data()
                .events
                .iter()
                .enumerate()
                .map(|(idx, event)| {
                    let enabled_prefix = if event.enabled { "✓" } else { "x" };

                    // Create inline effect preview
                    let effect_preview = if event.effects.is_empty() {
                        "No effects".to_string()
                    } else {
                        let first_effect = Self::format_effect_short(&event.effects[0]);
                        if event.effects.len() > 1 {
                            format!("{} +{}", first_effect, event.effects.len() - 1)
                        } else {
                            first_effect
                        }
                    };

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

                    // Compact format: [✓] Name: effect_preview
                    let line = Line::from(vec![
                        Span::styled(format!("[{}] ", enabled_prefix), style),
                        Span::styled(&event.name.0, style.add_modifier(Modifier::BOLD)),
                        Span::styled(": ", style),
                        Span::styled(effect_preview, style),
                    ]);

                    ListItem::new(line)
                })
                .collect()
        };

        let title = " EVENTS ";

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
            block = block.title_bottom(
                Line::from(" [a]dd [e]dit [d]el [c]opy [Shift+J/K] Reorder [t]oggle ")
                    .fg(Color::DarkGray),
            );
        }

        let list = List::new(items).block(block);

        frame.render_widget(list, area);
    }

    /// Format an effect in short form for inline preview
    fn format_effect_short(effect: &EffectData) -> String {
        match effect {
            EffectData::Income { to, amount, .. } => {
                format!("Income -> {}: {}", to.0, Self::format_amount_short(amount))
            }
            EffectData::Expense { from, amount } => {
                format!(
                    "Expense <- {}: {}",
                    from.0,
                    Self::format_amount_short(amount)
                )
            }
            EffectData::AssetPurchase { asset, amount, .. } => {
                format!("Buy {}: {}", asset.0, Self::format_amount_short(amount))
            }
            EffectData::AssetSale { asset, amount, .. } => {
                if let Some(a) = asset {
                    format!("Sell {}: {}", a.0, Self::format_amount_short(amount))
                } else {
                    format!("Liquidate: {}", Self::format_amount_short(amount))
                }
            }
            EffectData::Sweep { to, amount, .. } => {
                format!("Sweep -> {}: {}", to.0, Self::format_amount_short(amount))
            }
            EffectData::TriggerEvent { event } => format!("Trigger {}", event.0),
            EffectData::PauseEvent { event } => format!("Pause {}", event.0),
            EffectData::ResumeEvent { event } => format!("Resume {}", event.0),
            EffectData::TerminateEvent { event } => format!("End {}", event.0),
            EffectData::ApplyRmd { .. } => "Apply RMD".to_string(),
            EffectData::AdjustBalance { account, amount } => {
                format!(
                    "Adjust {}: {}",
                    account.0,
                    Self::format_amount_short(amount)
                )
            }
            EffectData::CashTransfer { from, to, amount } => {
                format!(
                    "Transfer {} -> {}: {}",
                    from.0,
                    to.0,
                    Self::format_amount_short(amount)
                )
            }
        }
    }

    /// Format amount in short form
    fn format_amount_short(amount: &AmountData) -> String {
        match amount {
            AmountData::Fixed(val) => {
                if *val >= 1_000_000.0 {
                    format!("${:.1}M", val / 1_000_000.0)
                } else if *val >= 1_000.0 {
                    format!("${:.0}K", val / 1_000.0)
                } else {
                    format!("${:.0}", val)
                }
            }
            AmountData::Special(special) => match special {
                SpecialAmount::SourceBalance => "SrcBal".to_string(),
                SpecialAmount::ZeroTargetBalance => "ZeroTgt".to_string(),
                SpecialAmount::TargetToBalance { .. } => "TgtBal".to_string(),
                SpecialAmount::AccountBalance { .. } => "AcctBal".to_string(),
                SpecialAmount::AccountCashBalance { .. } => "CashBal".to_string(),
            },
        }
    }

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
        }
    }

    // ========== Key Handlers ==========

    fn handle_event_list_keys(&self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let events_len = state.data().events.len();
        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            // Move down (Shift+J or Shift+Down)
            KeyCode::Char('J') if has_shift => {
                let idx = state.events_state.selected_event_index;
                if events_len >= 2 && idx < events_len - 1 {
                    state.data_mut().events.swap(idx, idx + 1);
                    state.events_state.selected_event_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Down if has_shift => {
                let idx = state.events_state.selected_event_index;
                if events_len >= 2 && idx < events_len - 1 {
                    state.data_mut().events.swap(idx, idx + 1);
                    state.events_state.selected_event_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            // Move up (Shift+K or Shift+Up)
            KeyCode::Char('K') if has_shift => {
                let idx = state.events_state.selected_event_index;
                if events_len >= 2 && idx > 0 {
                    state.data_mut().events.swap(idx, idx - 1);
                    state.events_state.selected_event_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Up if has_shift => {
                let idx = state.events_state.selected_event_index;
                if events_len >= 2 && idx > 0 {
                    state.data_mut().events.swap(idx, idx - 1);
                    state.events_state.selected_event_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
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
                    "Quick Events".to_string(),
                ];
                state.modal = ModalState::Picker(PickerModal::new(
                    "Select Trigger Type",
                    trigger_types,
                    ModalAction::PICK_TRIGGER_TYPE,
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

                    let yes_no = vec!["No".to_string(), "Yes".to_string()];
                    let form = FormModal::new(
                        "Edit Event",
                        vec![
                            FormField::text("Name", &event.name.0),
                            FormField::text(
                                "Description",
                                event.description.as_deref().unwrap_or(""),
                            ),
                            FormField::select(
                                "Once Only",
                                yes_no.clone(),
                                if event.once { "Yes" } else { "No" },
                            ),
                            FormField::select(
                                "Enabled",
                                yes_no,
                                if event.enabled { "Yes" } else { "No" },
                            ),
                            FormField::read_only("Trigger", &trigger_summary),
                            FormField::read_only("Effects", &effects_summary),
                        ],
                        ModalAction::EDIT_EVENT,
                    )
                    .with_typed_context(ModalContext::event_index(
                        state.events_state.selected_event_index,
                    ));

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
                            &format!("Delete event '{}'?\n\nThis cannot be undone.", event.name.0),
                            ModalAction::DELETE_EVENT,
                        )
                        .with_typed_context(ModalContext::event_index(
                            state.events_state.selected_event_index,
                        )),
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
                        ModalAction::MANAGE_EFFECTS,
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
                            | KeyCode::Char('f') => self.handle_event_list_keys(key, state),
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
                            | KeyCode::Char('f') => self.handle_event_list_keys(key, state),
                            _ => EventResult::NotHandled,
                        }
                    }
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

impl super::ModalHandler for EventsScreen {
    fn handles(&self, action: &ModalAction) -> bool {
        matches!(action, ModalAction::Event(_) | ModalAction::Effect(_))
    }

    fn handle_modal_result(
        &self,
        state: &mut AppState,
        action: ModalAction,
        value: &crate::modals::ConfirmedValue,
        legacy_value: &str,
    ) -> crate::actions::ActionResult {
        use crate::actions::{self, ActionContext, ActionResult};
        use crate::state::{EffectAction, EventAction};

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
                actions::handle_trigger_type_pick(state, legacy_value)
            }
            ModalAction::Event(EventAction::PickEventReference) => {
                actions::handle_event_reference_pick(legacy_value)
            }
            ModalAction::Event(EventAction::PickInterval) => {
                actions::handle_interval_pick(legacy_value)
            }
            ModalAction::Event(EventAction::Create) => actions::handle_create_event(state, ctx),
            ModalAction::Event(EventAction::Edit) => actions::handle_edit_event(state, ctx),
            ModalAction::Event(EventAction::Delete) => actions::handle_delete_event(state, ctx),
            // Trigger builder actions
            ModalAction::Event(EventAction::PickChildTriggerType) => {
                actions::handle_pick_child_trigger_type(state, legacy_value, ctx)
            }
            ModalAction::Event(EventAction::BuildChildTrigger) => {
                actions::handle_build_child_trigger(state, legacy_value, ctx)
            }
            ModalAction::Event(EventAction::CompleteChildTrigger) => {
                actions::handle_complete_child_trigger(state, ctx)
            }
            ModalAction::Event(EventAction::FinalizeRepeating) => {
                actions::handle_finalize_repeating(state, ctx)
            }
            ModalAction::Event(EventAction::PickQuickEvent) => {
                actions::handle_quick_event_pick(state, legacy_value)
            }

            // Effect actions
            ModalAction::Effect(EffectAction::Manage) => {
                actions::handle_manage_effects(state, legacy_value)
            }
            ModalAction::Effect(EffectAction::PickType) => ActionResult::close(),
            ModalAction::Effect(EffectAction::PickTypeForAdd) => {
                actions::handle_effect_type_for_add(state, legacy_value)
            }
            ModalAction::Effect(EffectAction::PickAccountForEffect) => {
                actions::handle_account_for_effect_pick(legacy_value)
            }
            ModalAction::Effect(EffectAction::PickActionForEffect) => {
                actions::handle_action_for_effect_pick(state, legacy_value, ctx)
            }
            ModalAction::Effect(EffectAction::Add) => actions::handle_add_effect(state, ctx),
            ModalAction::Effect(EffectAction::Edit) => actions::handle_edit_effect(state, ctx),
            ModalAction::Effect(EffectAction::Delete) => actions::handle_delete_effect(state, ctx),

            // This shouldn't happen if handles() is correct
            _ => ActionResult::close(),
        }
    }
}
