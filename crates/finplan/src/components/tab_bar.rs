use super::{Component, EventResult};
use crate::state::{AppState, TabId};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Tabs},
    Frame,
};

pub struct TabBar;

impl TabBar {
    pub fn new() -> Self {
        Self
    }
}

impl Component for TabBar {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        match key.code {
            KeyCode::Char('1') => {
                state.switch_tab(TabId::Portfolio);
                EventResult::Handled
            }
            KeyCode::Char('2') => {
                state.switch_tab(TabId::Profiles);
                EventResult::Handled
            }
            KeyCode::Char('3') => {
                state.switch_tab(TabId::Scenario);
                EventResult::Handled
            }
            KeyCode::Char('4') => {
                state.switch_tab(TabId::Events);
                EventResult::Handled
            }
            KeyCode::Char('5') => {
                state.switch_tab(TabId::Results);
                EventResult::Handled
            }
            KeyCode::Tab => {
                state.next_tab();
                EventResult::Handled
            }
            KeyCode::BackTab => {
                state.prev_tab();
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let titles: Vec<Line> = TabId::ALL
            .iter()
            .enumerate()
            .map(|(idx, tab)| {
                let num = idx + 1;
                let name = tab.name();
                let content = format!("[{}] {}", num, name);

                if *tab == state.active_tab {
                    Line::from(Span::styled(
                        content,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                } else {
                    Line::from(Span::styled(content, Style::default().fg(Color::Gray)))
                }
            })
            .collect();

        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::BOTTOM))
            .select(state.active_tab.index())
            .style(Style::default())
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_widget(tabs, area);
    }
}
