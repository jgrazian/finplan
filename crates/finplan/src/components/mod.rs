pub mod collapsible;
pub mod portfolio_overview;
pub mod status_bar;
pub mod tab_bar;

use crate::state::AppState;
use crossterm::event::KeyEvent;
use ratatui::Frame;

/// Result of handling an event
#[derive(Debug, Clone, PartialEq)]
pub enum EventResult {
    /// Event was handled, continue
    Handled,
    /// Event was not handled, pass to parent
    NotHandled,
    /// Request app exit
    Exit,
}

/// Trait for components that can handle input and render
pub trait Component {
    /// Handle a key event
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult;

    /// Render the component
    fn render(&mut self, frame: &mut Frame, area: ratatui::layout::Rect, state: &AppState);
}
