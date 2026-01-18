mod app_state;
pub mod context;
mod errors;
mod modal;
pub mod modal_action;
mod panels;
mod screen_state;
mod tabs;

// Re-export all types from submodules
pub use app_state::*;
pub use errors::*;
pub use modal::*;
pub use panels::*;
pub use screen_state::*;
pub use tabs::*;

// Note: context and modal_action are public modules but not re-exported with `*`
// to avoid naming conflicts during gradual migration.
// Import them explicitly: `use crate::state::context::*;` or `use crate::state::modal_action::*;`
