use crate::util::format::format_currency;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

/// Consistent dark grey color for empty bar portions
const DARK_GREY: Color = Color::Rgb(40, 40, 40);

/// Represents a single account bar in the portfolio overview chart
pub struct AccountBar {
    pub name: String,
    pub value: f64,
    pub color: Color,
}

impl AccountBar {
    pub fn new(name: impl Into<String>, value: f64, color: Color) -> Self {
        Self {
            name: name.into(),
            value,
            color,
        }
    }
}

/// A horizontal bar chart showing portfolio accounts and their values
pub struct PortfolioOverviewChart<'a> {
    accounts: &'a [AccountBar],
    title: Option<String>,
    focused: bool,
    /// If true, render values on top of the bar instead of to the right
    value_overlay: bool,
    /// Number of blank lines between each bar
    line_spacing: u16,
}

impl<'a> PortfolioOverviewChart<'a> {
    pub fn new(accounts: &'a [AccountBar]) -> Self {
        Self {
            accounts,
            title: None,
            focused: false,
            value_overlay: false,
            line_spacing: 0,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// If true, render values on top of the bar instead of to the right
    pub fn value_overlay(mut self, overlay: bool) -> Self {
        self.value_overlay = overlay;
        self
    }

    /// Set the number of blank lines between each bar (default: 0)
    pub fn line_spacing(mut self, spacing: u16) -> Self {
        self.line_spacing = spacing;
        self
    }

    pub fn render(self, frame: &mut Frame, area: Rect) {
        let border_style = if self.focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = self
            .title
            .unwrap_or_else(|| " PORTFOLIO OVERVIEW ".to_string());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if self.accounts.is_empty() {
            let msg =
                Paragraph::new("No accounts defined").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, inner_area);
            return;
        }

        // Calculate total positive value (exclude debts from percentage base)
        let total_positive: f64 = self
            .accounts
            .iter()
            .map(|a| a.value)
            .filter(|v| *v > 0.0)
            .sum();

        if total_positive == 0.0 {
            let msg =
                Paragraph::new("No positive value").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, inner_area);
            return;
        }

        // Render horizontal bars
        let available_height = inner_area.height as usize;
        let row_height = 1 + self.line_spacing as usize;
        let max_bars = if row_height > 0 {
            available_height / row_height
        } else {
            available_height
        };

        let mut y_offset = 0u16;
        for account in self.accounts.iter().take(max_bars) {
            let value = account.value;
            let color = account.color;

            // Truncate name if needed
            let name = if account.name.len() > 12 {
                format!("{}...", &account.name[..9])
            } else {
                account.name.clone()
            };

            // For debts, show as negative but calculate percentage of positive portfolio
            let percentage = if value >= 0.0 {
                (value / total_positive * 100.0).round() as i16
            } else {
                -((value.abs() / total_positive * 100.0).round() as i16)
            };

            let line = if self.value_overlay {
                // Overlay mode: value displayed on top of the bar (left-justified)
                // Layout: name (12) + space + bar (with value overlay) + space + percentage (5)
                let value_str = format_currency(value);
                let pct_str = format!("{:>4}%", percentage);

                // Calculate bar width: total - name(12) - spaces(2) - pct(5)
                let bar_width = inner_area.width.saturating_sub(20) as usize;
                let filled = if value >= 0.0 {
                    (bar_width as f64 * value / total_positive).round() as usize
                } else {
                    (bar_width as f64 * value.abs() / total_positive)
                        .round()
                        .min(bar_width as f64) as usize
                };

                let value_len = value_str.len();

                // Determine text color based on background brightness (only black or white)
                let text_color_on_color = match color {
                    Color::Red | Color::DarkGray | Color::Blue | Color::Magenta => Color::White,
                    _ => Color::Black,
                };

                // Build spans for bar with value overlay (left-justified)
                let mut spans = vec![Span::styled(
                    format!("{:<12} ", name),
                    Style::default().fg(color),
                )];

                let chars: Vec<char> = value_str.chars().collect();

                if filled >= value_len {
                    // Value fits entirely inside the filled portion
                    spans.push(Span::styled(
                        value_str,
                        Style::default().fg(text_color_on_color).bg(color),
                    ));
                    // Rest of filled bar - use spaces with colored background to match
                    let remaining_filled = filled.saturating_sub(value_len);
                    if remaining_filled > 0 {
                        spans.push(Span::styled(
                            " ".repeat(remaining_filled),
                            Style::default().bg(color),
                        ));
                    }
                    // Empty portion - solid grey background
                    let empty = bar_width.saturating_sub(filled);
                    if empty > 0 {
                        spans.push(Span::styled(
                            " ".repeat(empty),
                            Style::default().bg(DARK_GREY),
                        ));
                    }
                } else if filled > 0 {
                    // Value extends beyond filled portion - split the text
                    let filled_part: String = chars[..filled].iter().collect();
                    let empty_part: String = chars[filled..].iter().collect();

                    // Part on filled background (color bg)
                    spans.push(Span::styled(
                        filled_part,
                        Style::default().fg(text_color_on_color).bg(color),
                    ));
                    // Part on empty background (dark grey bg, white text)
                    spans.push(Span::styled(
                        empty_part,
                        Style::default().fg(Color::White).bg(DARK_GREY),
                    ));
                    // Fill rest - solid grey background
                    let remaining = bar_width.saturating_sub(value_len);
                    if remaining > 0 {
                        spans.push(Span::styled(
                            " ".repeat(remaining),
                            Style::default().bg(DARK_GREY),
                        ));
                    }
                } else {
                    // No filled portion - all text on dark grey (white text)
                    spans.push(Span::styled(
                        value_str,
                        Style::default().fg(Color::White).bg(DARK_GREY),
                    ));
                    // Fill rest - solid grey background
                    let remaining = bar_width.saturating_sub(value_len);
                    if remaining > 0 {
                        spans.push(Span::styled(
                            " ".repeat(remaining),
                            Style::default().bg(DARK_GREY),
                        ));
                    }
                }

                spans.push(Span::raw(format!(" {} ", pct_str)));
                Line::from(spans)
            } else {
                // Standard mode: value displayed to the right of the bar
                let bar_width = inner_area.width.saturating_sub(32) as usize;
                let filled = if value >= 0.0 {
                    (bar_width as f64 * value / total_positive).round() as usize
                } else {
                    (bar_width as f64 * value.abs() / total_positive)
                        .round()
                        .min(bar_width as f64) as usize
                };
                let empty = bar_width.saturating_sub(filled);

                // Use solid backgrounds instead of characters
                let bar_filled: String = " ".repeat(filled);
                let bar_empty: String = " ".repeat(empty);

                Line::from(vec![
                    Span::styled(format!("{:<12} ", name), Style::default().fg(color)),
                    Span::styled(bar_filled, Style::default().bg(color)),
                    Span::styled(bar_empty, Style::default().bg(DARK_GREY)),
                    Span::raw(format!(" {:>4}% ", percentage)),
                    Span::styled(
                        format_currency(value),
                        Style::default().fg(if value >= 0.0 {
                            Color::Rgb(100, 100, 100)
                        } else {
                            Color::Red
                        }),
                    ),
                ])
            };

            let bar_area = Rect::new(inner_area.x, inner_area.y + y_offset, inner_area.width, 1);
            frame.render_widget(Paragraph::new(line), bar_area);
            y_offset += 1 + self.line_spacing;
        }
    }
}
