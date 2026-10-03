//! `RowBatch`, `Placed` and the in-memory `merge_into` moved to
//! `finplan_plan::batch`; the SQL sink is `db::batch`. This keeps the old
//! path working while the rest of the plan model follows.

pub use finplan_plan::batch::*;
