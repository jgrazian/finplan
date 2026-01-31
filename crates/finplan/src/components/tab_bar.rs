use super::{Component, EventResult};
use crate::data::keybindings_data::KeybindingsConfig;
use crate::data::portfolio_data::AccountType;
use crate::state::{AppState, TabId};
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

pub struct TabBar;

impl Component for TabBar {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> EventResult {
        // Don't intercept keys when editing holdings (value input needs digits)
        if state
            .portfolio_profiles_state
            .account_mode
            .is_editing_value()
            || state.portfolio_profiles_state.account_mode.is_adding_new()
        {
            return EventResult::NotHandled;
        }

        let kb = &state.keybindings.global;
        if KeybindingsConfig::matches(&key, &kb.tab_1) {
            state.switch_tab(TabId::PortfolioProfiles);
            EventResult::Handled
        } else if KeybindingsConfig::matches(&key, &kb.tab_2) {
            state.switch_tab(TabId::Events);
            EventResult::Handled
        } else if KeybindingsConfig::matches(&key, &kb.tab_3) {
            state.switch_tab(TabId::Scenario);
            EventResult::Handled
        } else if KeybindingsConfig::matches(&key, &kb.tab_4) {
            state.switch_tab(TabId::Results);
            EventResult::Handled
        } else if KeybindingsConfig::matches(&key, &kb.tab_5) {
            state.switch_tab(TabId::Optimize);
            EventResult::Handled
        } else {
            EventResult::NotHandled
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        // Split area: tabs on left, status on right
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(60),    // Tabs (minimum width)
                Constraint::Length(30), // Status display
            ])
            .split(area);

        // Render tabs on the left
        let titles: Vec<Line> = TabId::ALL
            .iter()
            .enumerate()
            .map(|(idx, tab)| {
                let num = idx + 1;
                let name = tab.name();
                let content = format!("[{}] {}", num, name);

                if *tab == state.active_tab {
                    Line::from(Span::styled(
                        content,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                } else {
                    Line::from(Span::styled(content, Style::default().fg(Color::Gray)))
                }
            })
            .collect();

        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::BOTTOM))
            .select(state.active_tab.index())
            .style(Style::default())
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_widget(tabs, chunks[0]);

        // Render status on the right
        let status_line = self.build_status_line(state);
        let status = Paragraph::new(status_line)
            .alignment(Alignment::Right)
            .block(Block::default().borders(Borders::BOTTOM));

        frame.render_widget(status, chunks[1]);
    }
}

impl TabBar {
    /// Calculate current net worth from all accounts
    fn calculate_net_worth(&self, state: &AppState) -> f64 {
        state
            .data()
            .portfolios
            .accounts
            .iter()
            .map(|acc| match &acc.account_type {
                AccountType::Checking(prop)
                | AccountType::Savings(prop)
                | AccountType::HSA(prop)
                | AccountType::Property(prop)
                | AccountType::Collectible(prop) => prop.value,
                AccountType::Brokerage(inv)
                | AccountType::Traditional401k(inv)
                | AccountType::Roth401k(inv)
                | AccountType::TraditionalIRA(inv)
                | AccountType::RothIRA(inv) => inv.assets.iter().map(|a| a.value).sum(),
                AccountType::Mortgage(debt)
                | AccountType::LoanDebt(debt)
                | AccountType::StudentLoanDebt(debt) => -debt.balance,
            })
            .sum()
    }

    /// Format net worth in a compact way (e.g., $2.1M, $450K, $50K)
    fn format_compact_currency(&self, value: f64) -> String {
        let abs_value = value.abs();
        let sign = if value < 0.0 { "-" } else { "" };

        if abs_value >= 1_000_000.0 {
            format!("{}${:.1}M", sign, abs_value / 1_000_000.0)
        } else if abs_value >= 1_000.0 {
            format!("{}${:.0}K", sign, abs_value / 1_000.0)
        } else {
            format!("{}${:.0}", sign, abs_value)
        }
    }

    /// Build the status line showing scenario, net worth, and success rate
    fn build_status_line(&self, state: &AppState) -> Line<'static> {
        let mut spans = Vec::new();

        // Scenario name with dirty indicator
        let scenario_name = if state.is_current_dirty() {
            format!("{}*", state.current_scenario)
        } else {
            state.current_scenario.clone()
        };
        spans.push(Span::styled(
            scenario_name,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

        spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));

        // Net worth
        let net_worth = self.calculate_net_worth(state);
        let nw_color = if net_worth >= 0.0 {
            Color::Green
        } else {
            Color::Red
        };
        spans.push(Span::styled(
            self.format_compact_currency(net_worth),
            Style::default().fg(nw_color),
        ));

        spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));

        // Success rate from Monte Carlo (or -- if not run)
        let success_str = if let Some(mc) = &state.monte_carlo_result {
            format!("{:.0}%", mc.stats.success_rate * 100.0)
        } else {
            "--".to_string()
        };
        let success_color = if let Some(mc) = &state.monte_carlo_result {
            if mc.stats.success_rate >= 0.9 {
                Color::Green
            } else if mc.stats.success_rate >= 0.75 {
                Color::Yellow
            } else {
                Color::Red
            }
        } else {
            Color::DarkGray
        };
        spans.push(Span::styled(
            success_str,
            Style::default().fg(success_color),
        ));

        spans.push(Span::raw(" ")); // Padding from edge

        Line::from(spans)
    }
}
