use crate::components::{Component, EventResult};
use crate::data::events_data::{
    AmountData, EffectData, EventData, IntervalData, SpecialAmount, TriggerData,
};
use crate::state::{AppState, FocusedPanel};
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
        let is_focused = state.events_state.focused_panel == FocusedPanel::Left;

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
                    let event_desc = Self::format_event_summary(event);
                    let content = format!("{}: {}", event.name.0, event_desc);

                    let style = if idx == state.events_state.selected_event_index {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
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
        let is_focused = state.events_state.focused_panel == FocusedPanel::Right;

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

    fn format_effect(effect: &EffectData) -> String {
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
}

impl Component for EventsScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        let events = &state.data().events;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if state.events_state.focused_panel == FocusedPanel::Left && !events.is_empty() {
                    state.events_state.selected_event_index =
                        (state.events_state.selected_event_index + 1) % events.len();
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if state.events_state.focused_panel == FocusedPanel::Left && !events.is_empty() {
                    if state.events_state.selected_event_index == 0 {
                        state.events_state.selected_event_index = events.len() - 1;
                    } else {
                        state.events_state.selected_event_index -= 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Tab if key.modifiers.is_empty() => {
                state.events_state.focused_panel = match state.events_state.focused_panel {
                    FocusedPanel::Left => FocusedPanel::Right,
                    FocusedPanel::Right => FocusedPanel::Left,
                };
                EventResult::Handled
            }
            KeyCode::Char('a') => {
                state.set_error("Add event not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('d') => {
                state.set_error("Delete event not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('e') => {
                state.set_error("Edit event not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('c') => {
                state.set_error("Copy event not yet implemented".to_string());
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(area);

        self.render_event_list(frame, chunks[0], state);
        self.render_event_details(frame, chunks[1], state);
    }
}

impl Screen for EventsScreen {
    fn title(&self) -> &str {
        "Events"
    }
}
