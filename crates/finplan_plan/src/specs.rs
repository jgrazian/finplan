//! Request specs: the typed shapes a plan is built and edited from.
//!
//! Each derives `ts_rs::TS` with `#[ts(export)]`, so the web client's types
//! come from here (`scripts/gen-bindings.sh`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Extra contribution room from `from_age` through `through_age` (inclusive;
/// null for no upper bound), on top of the account's contribution limit. Age
/// is the one reached by December 31 of the contribution year. Tiers do not
/// stack: the largest that applies wins.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CatchUpSpec {
    pub from_age: u8,
    #[serde(default)]
    pub through_age: Option<u8>,
    pub amount: f64,
}
