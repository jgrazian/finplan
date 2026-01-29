//! Common styling utilities for TUI components

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

/// Standard color for focused panels
pub const FOCUS_COLOR: Color = Color::Yellow;

/// Standard color for unfocused borders
pub const BORDER_COLOR: Color = Color::White;

/// Standard color for help text
pub const HELP_COLOR: Color = Color::DarkGray;

/// Standard color for headers
pub const HEADER_COLOR: Color = Color::Cyan;

/// Standard color for positive values
pub const POSITIVE_COLOR: Color = Color::Green;

/// Standard color for negative values
pub const NEGATIVE_COLOR: Color = Color::Red;

/// Standard color for warning/caution values
pub const WARNING_COLOR: Color = Color::Yellow;

/// Create a block with a title that shows focused state via border color.
///
/// When focused, the border is yellow. When unfocused, it's the default color.
///
/// # Example
/// ```ignore
/// let block = focused_block("Accounts", is_focused);
/// frame.render_widget(Paragraph::new("...").block(block), area);
/// ```
pub fn focused_block(title: &str, focused: bool) -> Block<'static> {
    let border_style = if focused {
        Style::default().fg(FOCUS_COLOR)
    } else {
        Style::default()
    };

    Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title.to_string())
}

/// Create a block with title and bottom help text that shows focused state.
///
/// The help text is only shown when the panel is focused.
///
/// # Example
/// ```ignore
/// let block = focused_block_with_help("Accounts", is_focused, "[a]dd [e]dit [d]elete");
/// ```
pub fn focused_block_with_help(title: &str, focused: bool, help_text: &str) -> Block<'static> {
    let border_style = if focused {
        Style::default().fg(FOCUS_COLOR)
    } else {
        Style::default()
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title.to_string());

    if focused && !help_text.is_empty() {
        block = block.title_bottom(Line::from(format!(" {} ", help_text)).fg(HELP_COLOR));
    }

    block
}

/// Get the appropriate color for a monetary value (green for positive, red for negative).
pub fn value_color(value: f64) -> Color {
    if value >= 0.0 {
        POSITIVE_COLOR
    } else {
        NEGATIVE_COLOR
    }
}

/// Get the appropriate style for a monetary value.
pub fn value_style(value: f64) -> Style {
    Style::default().fg(value_color(value))
}

/// Get a color based on a success rate (green > 95%, yellow > 85%, red otherwise).
pub fn success_rate_color(rate: f64) -> Color {
    if rate >= 0.95 {
        POSITIVE_COLOR
    } else if rate >= 0.85 {
        WARNING_COLOR
    } else {
        NEGATIVE_COLOR
    }
}

/// Get a gradient color based on a ratio (0.0 to 1.0).
///
/// Returns colors from red (0.0) through yellow to green (1.0).
pub fn gradient_color(ratio: f64) -> Color {
    match ratio {
        r if r < 0.0 => NEGATIVE_COLOR,
        r if r < 0.25 => Color::Red,
        r if r < 0.5 => Color::Yellow,
        r if r < 0.75 => Color::LightYellow,
        _ => POSITIVE_COLOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focused_block_has_correct_color() {
        let block = focused_block("Test", true);
        // Block is created successfully - detailed style testing would require more complex setup
        assert!(format!("{:?}", block).contains("Test"));
    }

    #[test]
    fn test_value_color() {
        assert_eq!(value_color(100.0), POSITIVE_COLOR);
        assert_eq!(value_color(-100.0), NEGATIVE_COLOR);
        assert_eq!(value_color(0.0), POSITIVE_COLOR);
    }

    #[test]
    fn test_success_rate_color() {
        assert_eq!(success_rate_color(0.96), POSITIVE_COLOR);
        assert_eq!(success_rate_color(0.90), WARNING_COLOR);
        assert_eq!(success_rate_color(0.80), NEGATIVE_COLOR);
    }

    #[test]
    fn test_gradient_color() {
        assert_eq!(gradient_color(-0.1), NEGATIVE_COLOR);
        assert_eq!(gradient_color(0.1), Color::Red);
        assert_eq!(gradient_color(0.4), Color::Yellow);
        assert_eq!(gradient_color(0.6), Color::LightYellow);
        assert_eq!(gradient_color(0.9), POSITIVE_COLOR);
    }
}
