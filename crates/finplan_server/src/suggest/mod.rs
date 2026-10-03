//! Suggestions: structured edits to a saved plan, and the model-backed and
//! rule-based ways of proposing them.
//!
//! The edit vocabulary itself ([`Change`], [`resolve`], the diffs, writing a
//! batch into an in-memory graph) lives in [`finplan_plan::suggest`], which has
//! no database; what stays here is writing a batch through the routes' SQL
//! halves (`sql`) and everything that talks to a model.

pub mod ai;
pub mod rules;
mod sql;
pub mod templates;

pub use finplan_plan::suggest::*;
pub use sql::{apply_steps_sql, apply_to_sql};
