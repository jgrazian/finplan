use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Component, EventResult};
use crate::event::AppKeyEvent;
use crate::state::{AppState, SimulationStatus};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

// Spinner animation state
static SPINNER_FRAME: AtomicUsize = AtomicUsize::new(0);

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
        format!("{} | ctrl + s: save | q: quit", base)
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
    fn handle_key(&mut self, _key: AppKeyEvent, _state: &mut AppState) -> EventResult {
        EventResult::NotHandled
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let content = if let Some(error) = &state.error_message {
            // Error message takes priority
            Line::from(vec![
                Span::styled("Error: ", Style::default().fg(Color::Red)),
                Span::raw(error),
            ])
        } else if let Some(sim_spans) = Self::render_simulation_status(&state.simulation_status) {
            // Simulation status when running
            Line::from(sim_spans)
        } else {
            // Normal help text
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
