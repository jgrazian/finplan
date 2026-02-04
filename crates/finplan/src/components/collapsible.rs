//! Collapsible panel component for TUI screens.
//!
//! Provides a reusable collapsible panel that can be expanded/collapsed
//! with keyboard shortcuts.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Block,
};

use crate::util::styles::focused_block;

/// State for a collapsible panel
#[derive(Debug, Clone)]
pub struct CollapsibleState {
    /// Whether the panel is expanded
    pub expanded: bool,
}

impl Default for CollapsibleState {
    fn default() -> Self {
        Self { expanded: true }
    }
}

impl CollapsibleState {
    pub fn new(expanded: bool) -> Self {
        Self { expanded }
    }

    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }

    pub fn expand(&mut self) {
        self.expanded = true;
    }

    pub fn collapse(&mut self) {
        self.expanded = false;
    }
}

/// Configuration for rendering a collapsible panel
pub struct CollapsiblePanel<'a> {
    title: &'a str,
    expanded: bool,
    focused: bool,
}

impl<'a> CollapsiblePanel<'a> {
    pub fn new(title: &'a str, expanded: bool) -> Self {
        Self {
            title,
            expanded,
            focused: false,
        }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Returns the height needed for this panel when collapsed (title bar only)
    pub fn collapsed_height() -> u16 {
        1
    }

    /// Create a block for the panel with appropriate styling
    pub fn block(&self) -> Block<'a> {
        let indicator = if self.expanded { "[-]" } else { "[+]" };
        let title = format!(" {} {} ", indicator, self.title);

        focused_block(&title, self.focused)
    }

    /// Render a collapsed panel (just the title bar with indicator)
    pub fn render_collapsed(&self, frame: &mut Frame, area: Rect) {
        let indicator = if self.expanded { "[-]" } else { "[+]" };

        // For collapsed state, render a minimal block
        let title_line = Line::from(vec![
            Span::styled(
                format!("{} ", indicator),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(self.title, Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(" (collapsed)", Style::default().fg(Color::DarkGray)),
        ]);

        let block = focused_block("", self.focused).title(title_line);

        frame.render_widget(block, area);
    }

    /// Check if a key should toggle this panel's collapse state
    /// Returns true if the key was handled
    pub fn handle_collapse_key(key: char, state: &mut CollapsibleState) -> bool {
        match key {
            '-' | '_' => {
                state.collapse();
                true
            }
            '+' | '=' => {
                state.expand();
                true
            }
            ' ' => {
                state.toggle();
                true
            }
            _ => false,
        }
    }
}

/// Calculate layout constraints for a list of panels with collapse states
/// Returns constraints that respect collapsed panels
pub fn calculate_panel_constraints(
    panels: &[(u16, bool)], // (expanded_height, is_expanded)
    total_height: u16,
) -> Vec<u16> {
    let collapsed_height = 3u16; // Minimum height for collapsed panel (borders + title)

    // Calculate total expanded and collapsed heights
    let mut total_expanded = 0u16;
    let mut total_collapsed = 0u16;
    let mut expanded_count = 0;

    for (height, is_expanded) in panels {
        if *is_expanded {
            total_expanded += height;
            expanded_count += 1;
        } else {
            total_collapsed += collapsed_height;
        }
    }

    // Calculate available space for expanded panels
    let available_for_expanded = total_height.saturating_sub(total_collapsed);

    // Distribute space among expanded panels
    panels
        .iter()
        .map(|(height, is_expanded)| {
            if *is_expanded {
                if expanded_count > 0 && total_expanded > 0 {
                    // Proportional distribution
                    let ratio = *height as f32 / total_expanded as f32;
                    (available_for_expanded as f32 * ratio) as u16
                } else {
                    *height
                }
            } else {
                collapsed_height
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collapsible_state_toggle() {
        let mut state = CollapsibleState::default();
        assert!(state.expanded);

        state.toggle();
        assert!(!state.expanded);

        state.toggle();
        assert!(state.expanded);
    }

    #[test]
    fn test_calculate_constraints_all_expanded() {
        let panels = vec![(10, true), (10, true)];
        let constraints = calculate_panel_constraints(&panels, 20);
        assert_eq!(constraints, vec![10, 10]);
    }

    #[test]
    fn test_calculate_constraints_one_collapsed() {
        let panels = vec![(10, true), (10, false)];
        let constraints = calculate_panel_constraints(&panels, 20);
        // First panel gets remaining space, second is collapsed
        assert_eq!(constraints[1], 3); // Collapsed height
    }
}
