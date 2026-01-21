//! Common rendering helpers for modal widgets.
//!
//! This module extracts duplicated patterns from modal rendering to reduce code
//! and ensure consistency across different modal types.

use std::rc::Rc;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::centered_rect;

// ========== Cursor Rendering ==========

/// Render a line of text with a visible cursor at the specified position.
///
/// This helper extracts the cursor rendering logic used in text input modals
/// and form fields. The cursor is shown as a white background block.
///
/// # Arguments
/// * `display_value` - The text to display (may be scrolled/truncated)
/// * `cursor_pos` - Position of the cursor within `display_value`
/// * `prefix` - Optional prefix string before the text (e.g., " " for padding)
pub fn render_cursor_line(display_value: &str, cursor_pos: usize, prefix: &str) -> Line<'static> {
    let mut spans = Vec::new();

    if !prefix.is_empty() {
        spans.push(Span::raw(prefix.to_string()));
    }

    let chars: Vec<char> = display_value.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i == cursor_pos {
            spans.push(Span::styled(
                c.to_string(),
                Style::default().bg(Color::White).fg(Color::Black),
            ));
        } else {
            spans.push(Span::raw(c.to_string()));
        }
    }

    // If cursor is at the end, show a cursor block
    if cursor_pos >= chars.len() {
        spans.push(Span::styled(
            " ",
            Style::default().bg(Color::White).fg(Color::Black),
        ));
    }

    Line::from(spans)
}

// ========== Horizontal Scroll ==========

/// Result of scroll calculation for text that's wider than its container.
pub struct ScrolledView {
    /// The visible portion of the text
    pub display_value: String,
    /// The cursor position within the visible portion
    pub cursor_pos: usize,
}

/// Calculate horizontal scroll for a text input that's wider than its container.
///
/// Centers the cursor in the visible area when the text is longer than `max_width`.
///
/// # Arguments
/// * `value` - The full text value
/// * `cursor_pos` - Position of cursor in `value`
/// * `max_width` - Maximum width available for display
pub fn calculate_scroll(value: &str, cursor_pos: usize, max_width: usize) -> ScrolledView {
    let input_width = max_width.saturating_sub(2);

    if value.len() <= input_width {
        return ScrolledView {
            display_value: value.to_string(),
            cursor_pos,
        };
    }

    // Center cursor in visible area
    let start = cursor_pos.saturating_sub(input_width / 2);
    let end = (start + input_width).min(value.len());
    let start = end.saturating_sub(input_width);

    ScrolledView {
        display_value: value[start..end].to_string(),
        cursor_pos: cursor_pos - start,
    }
}

// ========== Modal Frame ==========

/// Result of rendering a modal frame, containing layout information.
pub struct ModalFrame {
    /// The inner area (inside the border)
    pub inner: Rect,
    /// The layout chunks for content placement
    pub chunks: Rc<[Rect]>,
}

/// Render a standard modal frame with title, border, and layout.
///
/// This eliminates the ~15 lines of boilerplate present in every modal render function:
/// - Centering the modal
/// - Clearing the background
/// - Drawing the border with title
/// - Creating the vertical layout
///
/// # Arguments
/// * `frame` - The ratatui Frame to render to
/// * `title` - Modal title (displayed in the border)
/// * `width` - Modal width in characters
/// * `height` - Modal height in characters
/// * `border_color` - Color for the border
/// * `constraints` - Layout constraints for content areas
///
/// # Example
/// ```ignore
/// let mf = render_modal_frame(
///     frame,
///     "My Modal",
///     60, 10,
///     Color::Cyan,
///     &[Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)],
/// );
/// // Now render content into mf.chunks[0], mf.chunks[1], etc.
/// ```
pub fn render_modal_frame(
    frame: &mut Frame,
    title: &str,
    width: u16,
    height: u16,
    border_color: Color,
    constraints: &[Constraint],
) -> ModalFrame {
    let modal_area = centered_rect(width, height, frame.area());

    // Clear the area behind the modal
    frame.render_widget(Clear, modal_area);

    // Create the modal block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(" {} ", title));

    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    // Create layout chunks
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    ModalFrame { inner, chunks }
}

// ========== Help Text Builder ==========

/// Builder for modal help text with consistent styling.
///
/// Creates a paragraph with key-description pairs, where keys are colored
/// and descriptions are plain text.
///
/// # Example
/// ```ignore
/// let help = HelpText::new()
///     .key("[Enter]", Color::Green, "Confirm")
///     .key("[Esc]", Color::Yellow, "Cancel")
///     .build();
/// frame.render_widget(help, area);
/// ```
pub struct HelpText {
    pub(crate) items: Vec<(String, Color, String)>,
    separator: String,
}

impl HelpText {
    /// Create a new empty HelpText builder.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            separator: "  ".to_string(),
        }
    }

    /// Set a custom separator between key-description pairs.
    pub fn separator(mut self, sep: &str) -> Self {
        self.separator = sep.to_string();
        self
    }

    /// Add a key-description pair.
    ///
    /// # Arguments
    /// * `key` - The key text (e.g., "[Enter]")
    /// * `color` - Color for the key text
    /// * `desc` - Description of what the key does
    pub fn key(mut self, key: &str, color: Color, desc: &str) -> Self {
        self.items.push((key.to_string(), color, desc.to_string()));
        self
    }

    /// Build the help text into a Paragraph widget.
    pub fn build(self) -> Paragraph<'static> {
        let mut spans: Vec<Span> = Vec::new();

        for (i, (key, color, desc)) in self.items.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(self.separator.clone()));
            }
            spans.push(Span::styled(key, Style::default().fg(color)));
            spans.push(Span::raw(format!(" {}", desc)));
        }

        Paragraph::new(Line::from(spans))
    }

    /// Build the help text into a centered Paragraph widget.
    pub fn build_centered(self) -> Paragraph<'static> {
        self.build().alignment(Alignment::Center)
    }
}

impl Default for HelpText {
    fn default() -> Self {
        Self::new()
    }
}

// ========== Multi-line Help Text ==========

/// Builder for multi-line help text.
pub struct MultiLineHelp {
    lines: Vec<Vec<(String, Color, String)>>,
}

impl MultiLineHelp {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Add a line of help text.
    pub fn line(mut self, help: HelpText) -> Self {
        self.lines.push(help.items);
        self
    }

    /// Build into a Paragraph with multiple lines.
    pub fn build(self) -> Paragraph<'static> {
        let separator = "  ";
        let lines: Vec<Line> = self
            .lines
            .into_iter()
            .map(|items| {
                let mut spans: Vec<Span> = Vec::new();
                for (i, (key, color, desc)) in items.into_iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::raw(separator.to_string()));
                    }
                    spans.push(Span::styled(key, Style::default().fg(color)));
                    spans.push(Span::raw(format!(" {}", desc)));
                }
                Line::from(spans)
            })
            .collect();

        Paragraph::new(lines)
    }
}

impl Default for MultiLineHelp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_scroll_short_text() {
        let result = calculate_scroll("hello", 3, 20);
        assert_eq!(result.display_value, "hello");
        assert_eq!(result.cursor_pos, 3);
    }

    #[test]
    fn test_calculate_scroll_long_text() {
        let value = "this is a very long text that needs scrolling";
        let result = calculate_scroll(value, 20, 15);
        assert!(result.display_value.len() <= 13); // 15 - 2
        assert!(result.cursor_pos < result.display_value.len() + 1);
    }

    #[test]
    fn test_render_cursor_line_middle() {
        let line = render_cursor_line("hello", 2, "");
        assert_eq!(line.spans.len(), 5); // h, e, [l], l, o
    }

    #[test]
    fn test_render_cursor_line_end() {
        let line = render_cursor_line("hi", 2, "");
        assert_eq!(line.spans.len(), 3); // h, i, [cursor block]
    }

    #[test]
    fn test_help_text_builder() {
        let help = HelpText::new()
            .key("[Enter]", Color::Green, "Confirm")
            .key("[Esc]", Color::Yellow, "Cancel")
            .build();

        // Just verify it builds without panicking
        let _ = help;
    }
}
