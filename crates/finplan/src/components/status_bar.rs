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
    fn get_help_text(state: &AppState) -> String {
        // Return help text based on active tab
        let base = match state.active_tab {
            crate::state::TabId::PortfolioProfiles => {
                "j/k: scroll | Tab: panel | a: add | e: edit | d: delete | h: holdings | m: map"
            }
            crate::state::TabId::Scenario => {
                "r: run | m: monte carlo | s/l: save/load scenario | i/x: import/export"
            }
            crate::state::TabId::Events => {
                "j/k: scroll | Tab: panel | a: add | e: edit | d: del | c: copy | t: toggle | f: effects"
            }
            crate::state::TabId::Results => "j/k: scroll",
            crate::state::TabId::Optimize => {
                "Tab: panel | j/k: nav | r: run | a: add param | d: delete"
            }
        };
        format!("{} | ^S: save | q: quit", base)
    }

    fn get_dirty_indicator(state: &AppState) -> Option<&'static str> {
        if state.has_unsaved_changes() {
            Some("[*]")
        } else {
            None
        }
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
            let mut spans = vec![];

            // Add dirty indicator if there are unsaved changes
            if let Some(indicator) = Self::get_dirty_indicator(state) {
                spans.push(Span::styled(indicator, Style::default().fg(Color::Yellow)));
                spans.push(Span::raw(" "));
            }

            spans.push(Span::styled(
                Self::get_help_text(state),
                Style::default().fg(Color::DarkGray),
            ));

            Line::from(spans)
        };

        let paragraph = Paragraph::new(content).block(Block::default().borders(Borders::TOP));

        frame.render_widget(paragraph, area);
    }
}
