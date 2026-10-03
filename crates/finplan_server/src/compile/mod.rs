//! Compiling a scenario graph into an engine config moved to
//! `finplan_plan::compile`; this keeps the old paths working while the rest of
//! the plan model follows.

pub mod rows;

pub use finplan_plan::compile::*;
