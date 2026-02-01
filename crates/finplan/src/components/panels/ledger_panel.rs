//! Ledger panel component extracted from ResultsScreen.
//!
//! Renders the simulation ledger with filtering and scrolling.

use std::collections::HashMap;

use crate::state::{AppState, LedgerFilter, PercentileView};
use crate::util::format::format_currency;
use finplan_core::model::{AccountId, EventId, LedgerEntry, StateEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

/// Ledger panel component.
pub struct LedgerPanel;

impl LedgerPanel {
    /// Render the ledger panel.
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let filter = state.results_state.ledger_filter;

        // Build title with percentile indicator if viewing Monte Carlo
        let title = if state.results_state.viewing_monte_carlo {
            let pct = state.results_state.percentile_view.short_label();
            if focused {
                format!(
                    " LEDGER [{}] ({}) [j/k scroll, f filter, v view] ",
                    filter.label(),
                    pct
                )
            } else {
                format!(" LEDGER [{}] ({}) ", filter.label(), pct)
            }
        } else if focused {
            format!(" LEDGER [{}] [j/k scroll, f filter] ", filter.label())
        } else {
            format!(" LEDGER [{}] ", filter.label())
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        if let Some(core_result) = Self::get_current_core_result(state) {
            let account_names = Self::build_account_name_map(state);
            let event_names = Self::build_event_name_map(state);

            // Filter entries
            let filtered_entries: Vec<&LedgerEntry> = core_result
                .ledger
                .iter()
                .filter(|entry| Self::matches_ledger_filter(entry, filter))
                .collect();

            if filtered_entries.is_empty() {
                let paragraph = Paragraph::new("No matching entries").block(block);
                frame.render_widget(paragraph, area);
                return;
            }

            let visible_count = (area.height as usize).saturating_sub(2);
            let scroll_offset = state
                .results_state
                .ledger_scroll_offset
                .min(filtered_entries.len().saturating_sub(1));

            let items: Vec<ListItem> = filtered_entries
                .iter()
                .skip(scroll_offset)
                .take(visible_count)
                .map(|entry| {
                    let color = Self::get_event_color(&entry.event);
                    let text = Self::format_state_event(&entry.event, &account_names, &event_names);
                    let line = Line::from(vec![
                        Span::styled(
                            format!("[{}] ", entry.date),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(text, Style::default().fg(color)),
                    ]);
                    ListItem::new(line)
                })
                .collect();

            let list = List::new(items).block(block);
            frame.render_widget(list, area);
        } else {
            let paragraph = Paragraph::new("No simulation data").block(block);
            frame.render_widget(paragraph, area);
        }
    }

    // ========== Helper Functions ==========

    /// Get the current core result based on viewing mode (Monte Carlo percentile or single run).
    fn get_current_core_result(state: &AppState) -> Option<&finplan_core::model::SimulationResult> {
        if state.results_state.viewing_monte_carlo {
            if let Some(mc) = &state.monte_carlo_result {
                match state.results_state.percentile_view {
                    PercentileView::P5 => mc.get_percentile_core(0.05),
                    PercentileView::P50 => mc.get_percentile_core(0.50),
                    PercentileView::P95 => mc.get_percentile_core(0.95),
                    PercentileView::Mean => mc.mean_core_result.as_ref(),
                }
            } else {
                state.core_simulation_result.as_ref()
            }
        } else {
            state.core_simulation_result.as_ref()
        }
    }

    /// Build a map of AccountId to account names from the current simulation data.
    fn build_account_name_map(state: &AppState) -> HashMap<AccountId, String> {
        let mut map = HashMap::new();
        for (idx, account) in state.data().portfolios.accounts.iter().enumerate() {
            let id = AccountId((idx + 1) as u16);
            map.insert(id, account.name.clone());
        }
        map
    }

    /// Build a map of EventId to event names from the current simulation data.
    fn build_event_name_map(state: &AppState) -> HashMap<EventId, String> {
        let mut map = HashMap::new();
        for (idx, event) in state.data().events.iter().enumerate() {
            let id = EventId((idx + 1) as u16);
            map.insert(id, event.name.0.clone());
        }
        map
    }

    /// Check if a ledger entry matches the current filter.
    fn matches_ledger_filter(entry: &LedgerEntry, filter: LedgerFilter) -> bool {
        match filter {
            LedgerFilter::All => true,
            LedgerFilter::CashOnly => entry.event.is_cash_event(),
            LedgerFilter::AssetsOnly => entry.event.is_asset_event(),
            LedgerFilter::TaxesOnly => entry.event.is_tax_event(),
            LedgerFilter::EventsOnly => entry.event.is_event_management(),
        }
    }

    /// Get the color for a ledger entry based on its type.
    fn get_event_color(event: &StateEvent) -> Color {
        if event.is_cash_event() {
            Color::Cyan
        } else if event.is_asset_event() {
            Color::Magenta
        } else if event.is_tax_event() {
            Color::Red
        } else if event.is_event_management() {
            Color::Yellow
        } else {
            Color::Gray
        }
    }

    /// Format a StateEvent for display in the ledger.
    fn format_state_event(
        event: &StateEvent,
        account_names: &HashMap<AccountId, String>,
        event_names: &HashMap<EventId, String>,
    ) -> String {
        match event {
            StateEvent::TimeAdvance {
                from_date,
                to_date,
                days_elapsed,
            } => {
                format!("Time: {} -> {} ({} days)", from_date, to_date, days_elapsed)
            }
            StateEvent::CreateAccount(account) => {
                let name = account_names
                    .get(&account.account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Create account: {}", name)
            }
            StateEvent::DeleteAccount(id) => {
                let name = account_names
                    .get(id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Delete account: {}", name)
            }
            StateEvent::CashCredit { to, amount, kind } => {
                let name = account_names
                    .get(to)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let kind_str = match kind {
                    finplan_core::model::CashFlowKind::Income => "Income",
                    finplan_core::model::CashFlowKind::LiquidationProceeds => "Withdrawal",
                    finplan_core::model::CashFlowKind::Appreciation => "Interest",
                    finplan_core::model::CashFlowKind::RmdWithdrawal => "RMD",
                    finplan_core::model::CashFlowKind::Transfer => "Transfer",
                    _ => "Credit",
                };
                format!("{}: {} to {}", kind_str, format_currency(*amount), name)
            }
            StateEvent::CashDebit { from, amount, kind } => {
                let name = account_names
                    .get(from)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let kind_str = match kind {
                    finplan_core::model::CashFlowKind::Expense => "Expense",
                    finplan_core::model::CashFlowKind::Contribution => "Contribution",
                    finplan_core::model::CashFlowKind::InvestmentPurchase => "Purchase",
                    finplan_core::model::CashFlowKind::Transfer => "Transfer",
                    _ => "Debit",
                };
                format!("{}: {} from {}", kind_str, format_currency(*amount), name)
            }
            StateEvent::CashAppreciation {
                account_id,
                previous_value,
                new_value,
                return_rate,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let gain = new_value - previous_value;
                format!(
                    "{}: {} appreciation ({:.2}%)",
                    name,
                    format_currency(gain),
                    return_rate * 100.0
                )
            }
            StateEvent::LiabilityInterestAccrual {
                account_id,
                previous_principal,
                new_principal,
                interest_rate,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let interest = new_principal - previous_principal;
                format!(
                    "{}: {} interest accrued ({:.2}%)",
                    name,
                    format_currency(interest),
                    interest_rate * 100.0
                )
            }
            StateEvent::AssetPurchase {
                account_id,
                units,
                cost_basis,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!(
                    "{}: Buy {:.2} units for {}",
                    name,
                    units,
                    format_currency(*cost_basis)
                )
            }
            StateEvent::AssetSale {
                account_id,
                units,
                proceeds,
                short_term_gain,
                long_term_gain,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let total_gain = short_term_gain + long_term_gain;
                format!(
                    "{}: Sell {:.2} units for {} (gain: {})",
                    name,
                    units,
                    format_currency(*proceeds),
                    format_currency(total_gain)
                )
            }
            StateEvent::IncomeTax {
                gross_amount,
                federal_tax,
                state_tax,
            } => {
                let total = federal_tax + state_tax;
                format!(
                    "Income tax on {}: {} (Fed: {}, State: {})",
                    format_currency(*gross_amount),
                    format_currency(total),
                    format_currency(*federal_tax),
                    format_currency(*state_tax)
                )
            }
            StateEvent::ShortTermCapitalGainsTax {
                gross_gain,
                federal_tax,
                state_tax,
            } => {
                let total = federal_tax + state_tax;
                format!(
                    "ST Cap Gains tax on {}: {}",
                    format_currency(*gross_gain),
                    format_currency(total)
                )
            }
            StateEvent::LongTermCapitalGainsTax {
                gross_gain,
                federal_tax,
                state_tax,
            } => {
                let total = federal_tax + state_tax;
                format!(
                    "LT Cap Gains tax on {}: {}",
                    format_currency(*gross_gain),
                    format_currency(total)
                )
            }
            StateEvent::EarlyWithdrawalPenalty {
                gross_amount,
                penalty_amount,
                penalty_rate,
            } => {
                format!(
                    "Early withdrawal penalty on {}: {} ({:.0}%)",
                    format_currency(*gross_amount),
                    format_currency(*penalty_amount),
                    penalty_rate * 100.0
                )
            }
            StateEvent::EventTriggered { event_id } => {
                let name = event_names
                    .get(event_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Event triggered: {}", name)
            }
            StateEvent::EventPaused { event_id } => {
                let name = event_names
                    .get(event_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Event paused: {}", name)
            }
            StateEvent::EventResumed { event_id } => {
                let name = event_names
                    .get(event_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Event resumed: {}", name)
            }
            StateEvent::EventTerminated { event_id } => {
                let name = event_names
                    .get(event_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!("Event terminated: {}", name)
            }
            StateEvent::YearRollover { from_year, to_year } => {
                format!("Year rollover: {} -> {}", from_year, to_year)
            }
            StateEvent::RmdWithdrawal {
                account_id,
                age,
                required_amount,
                actual_amount,
                ..
            } => {
                let name = account_names
                    .get(account_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                format!(
                    "{}: RMD at age {} - required {}, withdrew {}",
                    name,
                    age,
                    format_currency(*required_amount),
                    format_currency(*actual_amount)
                )
            }
            StateEvent::BalanceAdjusted {
                account,
                previous_balance,
                new_balance,
                delta,
            } => {
                let name = account_names
                    .get(account)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let direction = if *delta >= 0.0 {
                    "increased"
                } else {
                    "decreased"
                };
                format!(
                    "{}: Balance {} by {} ({} -> {})",
                    name,
                    direction,
                    format_currency(delta.abs()),
                    format_currency(*previous_balance),
                    format_currency(*new_balance)
                )
            }
        }
    }

    /// Get the filtered entry count for scrolling calculations.
    pub fn get_filtered_count(state: &AppState) -> usize {
        if let Some(core_result) = Self::get_current_core_result(state) {
            let filter = state.results_state.ledger_filter;
            core_result
                .ledger
                .iter()
                .filter(|e| Self::matches_ledger_filter(e, filter))
                .count()
        } else {
            0
        }
    }
}
