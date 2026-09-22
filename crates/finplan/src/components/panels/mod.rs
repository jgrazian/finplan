//! Panel components extracted from screen implementations.

mod accounts_panel;
pub mod event_list_panel;
mod ledger_panel;
pub mod parameters_panel;
mod profiles_panel;

pub use accounts_panel::AccountsPanel;
pub use event_list_panel::EventListPanel;
pub use ledger_panel::LedgerPanel;
pub use profiles_panel::ProfilesPanel;
