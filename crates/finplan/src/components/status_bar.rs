use super::{Component, EventResult};
use crate::state::AppState;
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub struct StatusBar;

impl StatusBar {
    pub fn new() -> Self {
        Self
    }

    fn get_help_text(state: &AppState) -> String {
        // Return help text based on active tab
        match state.active_tab {
            crate::state::TabId::PortfolioProfiles => {
                "j/k: nav | Tab: panel | a: add | e: edit | d: delete | h: holdings | m: map | q: quit"
            }
            crate::state::TabId::Scenario => {
                "1-4: switch tabs | r: run simulation | m: monte carlo | s/l: save/load | q: quit"
            }
            crate::state::TabId::Events => {
                "1-4: switch tabs | j/k: navigate | Tab: switch panel | q: quit"
            }
            crate::state::TabId::Results => {
                "1-4: switch tabs | j/k: scroll | q: quit"
            }
        }
        .to_string()
    }
}

impl Component for StatusBar {
    fn handle_key(&mut self, _key: KeyEvent, _state: &mut AppState) -> EventResult {
        EventResult::NotHandled
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let content = if let Some(error) = &state.error_message {
            Line::from(vec![
                Span::styled("Error: ", Style::default().fg(Color::Red)),
                Span::raw(error),
            ])
        } else {
            Line::from(Span::styled(
                Self::get_help_text(state),
                Style::default().fg(Color::DarkGray),
            ))
        };

        let paragraph = Paragraph::new(content).block(Block::default().borders(Borders::TOP));

        frame.render_widget(paragraph, area);
    }
}
