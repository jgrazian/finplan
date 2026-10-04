//! What an analysis hands back.
//!
//! The bodies themselves are `finplan_plan::analysis::results`, shared with a
//! plan analysed in the browser; only the cached sweep, which is the server's
//! own record, is defined here.

pub use finplan_plan::analysis::results::*;

use serde::Serialize;
use ts_rs::TS;

/// A sweep read back from the cache rather than from the job that ran it.
///
/// Carries when it was run, because a restored grid is the one thing on the
/// Analysis screen that may be older than the plan it describes: the screen
/// says so in its footer instead of passing it off as this session's answer.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CachedSweep {
    pub scenario_id: i64,
    /// When the sweep finished, UTC, `YYYY-MM-DD HH:MM:SS`.
    pub created_at: String,
    pub results: SweepResults,
    /// The graphs arranged over this grid, exactly as the client stored them,
    /// or `null` where nobody has arranged any. Opaque here: what a graph is
    /// drawn as, against what, and sliced where are the client's choices, and
    /// typing them server-side would mean a deploy to add a chart kind.
    #[ts(type = "unknown")]
    pub layout: Option<serde_json::Value>,
}
