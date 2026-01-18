use crate::components::{Component, EventResult};
use crate::state::AppState;
use crate::util::format::format_currency;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, List, ListItem, Paragraph},
};

use super::Screen;

pub struct ResultsScreen;

impl ResultsScreen {
    pub fn new() -> Self {
        Self
    }

    fn render_chart(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" NET WORTH PROJECTION ");

        if let Some(result) = &state.simulation_result {
            if result.years.is_empty() {
                let paragraph = Paragraph::new("No data to display").block(block);
                frame.render_widget(paragraph, area);
                return;
            }

            // Calculate how many bars we can show based on available width
            let inner_width = area.width.saturating_sub(2) as usize; // Account for borders
            let step = if result.years.len() > inner_width && inner_width > 0 {
                (result.years.len() as f64 / inner_width as f64).ceil() as usize
            } else {
                1
            };

            // Create bars for the chart - sample years to fit available space
            let bars: Vec<Bar> = result
                .years
                .iter()
                .step_by(step.max(1))
                .take(inner_width)
                .map(|year| {
                    // Scale net worth to u64 (in thousands for display)
                    let value = (year.net_worth / 1000.0).max(0.0) as u64;
                    let style = self.net_worth_style(year.net_worth, result.final_net_worth);

                    Bar::default()
                        .value(value)
                        .label(Line::from(format!("{}", year.year)))
                        .text_value(format_currency(year.net_worth))
                        .style(style)
                        .value_style(style.reversed())
                })
                .collect();

            let chart = BarChart::default()
                .block(block)
                .data(BarGroup::default().bars(&bars))
                .bar_width(4)
                .bar_gap(1)
                .direction(Direction::Vertical);

            frame.render_widget(chart, area);
        } else {
            let content = vec![
                Line::from(""),
                Line::from("No simulation results available."),
                Line::from(""),
                Line::from("Run a simulation from the Scenario screen to see results here."),
            ];
            let paragraph = Paragraph::new(content).block(block);
            frame.render_widget(paragraph, area);
        }
    }

    fn net_worth_style(&self, value: f64, final_value: f64) -> Style {
        // Color gradient based on progress toward final value
        let ratio = if final_value > 0.0 {
            (value / final_value).clamp(0.0, 1.5)
        } else {
            0.0
        };

        if value < 0.0 {
            Style::default().fg(Color::Red)
        } else if ratio < 0.25 {
            Style::default().fg(Color::Yellow)
        } else if ratio < 0.5 {
            Style::default().fg(Color::LightYellow)
        } else if ratio < 0.75 {
            Style::default().fg(Color::LightGreen)
        } else {
            Style::default().fg(Color::Green)
        }
    }

    fn render_summary(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let lines = if let Some(result) = &state.simulation_result {
            vec![
                Line::from(Span::styled(
                    "SUMMARY",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(format!(
                    "  Final Net Worth:    {}",
                    format_currency(result.final_net_worth)
                )),
                Line::from(format!("  Simulation Years:   {}", result.years.len())),
                Line::from(""),
                Line::from(Span::styled(
                    "[e] Export CSV  [p] PDF report",
                    Style::default().fg(Color::DarkGray),
                )),
            ]
        } else {
            vec![
                Line::from("No results available."),
                Line::from(""),
                Line::from("Run a simulation first."),
            ]
        };

        let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));

        frame.render_widget(paragraph, area);
    }

    fn render_yearly_breakdown(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let items: Vec<ListItem> = if let Some(result) = &state.simulation_result {
            let start_idx = state.results_state.scroll_offset;
            let visible_count = (area.height as usize).saturating_sub(3); // Account for borders and header

            // Header
            let mut items = vec![ListItem::new(Line::from(vec![Span::styled(
                format!(
                    "{:>6} {:>5} {:>12} {:>12} {:>12} {:>12}",
                    "Year", "Age", "Income", "Expense", "Taxes", "Net Worth"
                ),
                Style::default().add_modifier(Modifier::BOLD),
            )]))];

            // Data rows
            for year in result.years.iter().skip(start_idx).take(visible_count) {
                items.push(ListItem::new(Line::from(format!(
                    "{:>6} {:>5} {:>12} {:>12} {:>12} {:>12}",
                    year.year,
                    year.age,
                    format_currency(year.income),
                    format_currency(year.expenses),
                    format_currency(year.taxes),
                    format_currency(year.net_worth)
                ))));
            }

            items
        } else {
            vec![ListItem::new(Line::from("No data"))]
        };

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" YEARLY BREAKDOWN "),
        );

        frame.render_widget(list, area);
    }
}

impl Component for ResultsScreen {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(result) = &state.simulation_result {
                    if state.results_state.scroll_offset + 1 < result.years.len() {
                        state.results_state.scroll_offset += 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if state.results_state.scroll_offset > 0 {
                    state.results_state.scroll_offset -= 1;
                }
                EventResult::Handled
            }
            KeyCode::Char('e') => {
                state.set_error("Export CSV not yet implemented".to_string());
                EventResult::Handled
            }
            KeyCode::Char('p') => {
                state.set_error("PDF report not yet implemented".to_string());
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(20), // Chart
                Constraint::Length(10), // Summary
                Constraint::Min(0),     // Yearly breakdown
            ])
            .split(area);

        self.render_chart(frame, chunks[0], state);
        self.render_summary(frame, chunks[1], state);
        self.render_yearly_breakdown(frame, chunks[2], state);
    }
}

impl Screen for ResultsScreen {
    fn title(&self) -> &str {
        "Results"
    }
}
