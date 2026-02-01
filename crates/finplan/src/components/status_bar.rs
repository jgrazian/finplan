use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Component, EventResult};
use crate::state::{AppState, SimulationStatus};
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

// Spinner animation state
static SPINNER_FRAME: AtomicUsize = AtomicUsize::new(0);

pub struct StatusBar;

impl StatusBar {
    /// Returns tab-specific help text for the left side of the status bar
    fn get_tab_help_text(state: &AppState) -> &'static str {
        match state.active_tab {
            crate::state::TabId::PortfolioProfiles => "Tab: panel | y: hist/param",
            crate::state::TabId::Events => {
                "Tab: panel | a: add | e: edit | d: del | c: copy | t: toggle | f: effects"
            }
            crate::state::TabId::Scenario => {
                "r: run | m/M: MC | M: MC Conv | R: all | c: copy | n: new | s/l: save/load | e: params"
            }
            crate::state::TabId::Results => {
                "h/l: year | $: real/nominal | v: percentile | f: filter"
            }
            crate::state::TabId::Optimize => "Tab: panel | r: run | a: add | d: del | s: settings",
        }
    }

    /// Returns global help text for the right side of the status bar
    fn get_global_help_text() -> &'static str {
        "1-5: tabs | Ctrl+S: save | q: quit"
    }

    fn get_dirty_indicator(state: &AppState) -> Option<&'static str> {
        if state.has_unsaved_changes() {
            Some("[*]")
        } else {
            None
        }
    }

    /// Render simulation status with spinner/progress
    fn render_simulation_status(status: &SimulationStatus) -> Option<Vec<Span<'static>>> {
        const SPINNER_CHARS: [char; 4] = ['|', '/', '-', '\\'];

        match status {
            SimulationStatus::Idle => None,
            SimulationStatus::RunningSingle => {
                let idx = SPINNER_FRAME.fetch_add(1, Ordering::Relaxed) % 4;
                Some(vec![
                    Span::styled(
                        format!(" {} ", SPINNER_CHARS[idx]),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("Running simulation...", Style::default().fg(Color::Yellow)),
                    Span::styled(" [Esc to cancel]", Style::default().fg(Color::DarkGray)),
                ])
            }
            SimulationStatus::RunningMonteCarlo { current, total } => {
                let idx = SPINNER_FRAME.fetch_add(1, Ordering::Relaxed) % 4;
                let pct = if *total > 0 {
                    (*current as f64 / *total as f64 * 100.0) as usize
                } else {
                    0
                };
                let bar_width = 20;
                let filled = (pct * bar_width / 100).min(bar_width);
                let empty = bar_width - filled;
                let bar = format!("[{}{}]", "=".repeat(filled), " ".repeat(empty));

                Some(vec![
                    Span::styled(
                        format!(" {} ", SPINNER_CHARS[idx]),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("MC ", Style::default().fg(Color::Cyan)),
                    Span::styled(bar, Style::default().fg(Color::Cyan)),
                    Span::styled(format!(" {}%", pct), Style::default().fg(Color::Cyan)),
                    Span::styled(" [Esc to cancel]", Style::default().fg(Color::DarkGray)),
                ])
            }
            SimulationStatus::RunningBatch {
                scenario_index,
                scenario_total,
                iteration_current,
                iteration_total,
                current_scenario_name,
            } => {
                let idx = SPINNER_FRAME.fetch_add(1, Ordering::Relaxed) % 4;

                // Calculate overall progress
                let total_iterations = *scenario_total * *iteration_total;
                let completed_iterations = *scenario_index * *iteration_total + *iteration_current;
                let overall_pct = if total_iterations > 0 {
                    (completed_iterations as f64 / total_iterations as f64 * 100.0) as usize
                } else {
                    0
                };

                // Build progress bar
                let bar_width = 15;
                let filled = (overall_pct * bar_width / 100).min(bar_width);
                let empty = bar_width - filled;
                let bar = format!("[{}{}]", "=".repeat(filled), " ".repeat(empty));

                // Scenario name (truncated if needed)
                let scenario_display = current_scenario_name
                    .as_ref()
                    .map(|n| {
                        if n.len() > 12 {
                            format!("{}…", n.chars().take(11).collect::<String>())
                        } else {
                            n.clone()
                        }
                    })
                    .unwrap_or_else(|| "...".to_string());

                Some(vec![
                    Span::styled(
                        format!(" {} ", SPINNER_CHARS[idx]),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("Batch {}/{} ", scenario_index + 1, scenario_total),
                        Style::default().fg(Color::Magenta),
                    ),
                    Span::styled(bar, Style::default().fg(Color::Magenta)),
                    Span::styled(
                        format!(" {}% ", overall_pct),
                        Style::default().fg(Color::Magenta),
                    ),
                    Span::styled(
                        format!("({})", scenario_display),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(" [Esc]", Style::default().fg(Color::DarkGray)),
                ])
            }
        }
    }
}

impl Component for StatusBar {
    fn handle_key(&mut self, _key: KeyEvent, _state: &mut AppState) -> EventResult {
        EventResult::NotHandled
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        // Check for priority content (error or simulation status)
        let priority_content: Option<Line<'_>> = if let Some(error) = &state.error_message {
            // Error message takes priority
            Some(Line::from(vec![
                Span::styled("Error: ", Style::default().fg(Color::Red)),
                Span::raw(error.clone()),
            ]))
        } else {
            // Simulation status when running
            Self::render_simulation_status(&state.simulation_status).map(Line::from)
        };

        if let Some(content) = priority_content {
            // Render priority content spanning the full width
            let paragraph = Paragraph::new(content).block(Block::default().borders(Borders::TOP));
            frame.render_widget(paragraph, area);
        } else {
            // Render split layout with tab help on left and global help on right
            let block = Block::default().borders(Borders::TOP);
            let inner_area = block.inner(area);
            frame.render_widget(block, area);

            // Split the inner area into left and right sections
            // Global help text is 34 chars: "1-5: tabs | Ctrl+S: save | q: quit"
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(35)])
                .split(inner_area);

            // Build left side: dirty indicator + tab-specific help
            let mut left_spans = vec![];
            if let Some(indicator) = Self::get_dirty_indicator(state) {
                left_spans.push(Span::styled(indicator, Style::default().fg(Color::Yellow)));
                left_spans.push(Span::raw(" "));
            }
            left_spans.push(Span::styled(
                Self::get_tab_help_text(state),
                Style::default().fg(Color::DarkGray),
            ));
            let left_paragraph = Paragraph::new(Line::from(left_spans));
            frame.render_widget(left_paragraph, chunks[0]);

            // Build right side: global help (brighter, right-aligned)
            let global_text = Self::get_global_help_text();
            let right_paragraph = Paragraph::new(Line::from(Span::styled(
                global_text,
                Style::default().fg(Color::White),
            )))
            .alignment(ratatui::layout::Alignment::Right);
            frame.render_widget(right_paragraph, chunks[1]);
        }
    }
}
