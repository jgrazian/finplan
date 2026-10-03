//! Asset request bodies.

use serde::Deserialize;
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateAsset {
    /// The ticker symbol alone, as the Ticker column shows it: `VBTLX`, not
    /// `VBTLX Vanguard Total Bond Market`. A holding with no ticker takes a
    /// short label instead.
    pub name: String,
    /// The fund's full name, as the Name column shows it: `Vanguard Total
    /// Bond Market Index Fund Admiral Shares`.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "one")]
    pub initial_price: f64,
    /// Omitted or null creates the asset unmapped.
    #[serde(default)]
    pub return_profile_id: Option<i64>,
    #[serde(default)]
    pub tracking_error: Option<f64>,
    /// Omitted appends to the end of the scenario's list, which is where a new
    /// asset belongs — pinning it at 0 would put it in front of every row the
    /// user has already dragged into place.
    #[serde(default)]
    pub sort_order: Option<i64>,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateAsset {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub initial_price: Option<f64>,
    /// Doubly optional: absent leaves the mapping alone, an explicit null
    /// unmaps the asset. Every other field here reads absent as "unchanged",
    /// which would otherwise make unmapping unsayable.
    #[serde(default, deserialize_with = "crate::specs::double_option")]
    #[ts(optional, type = "number | null")]
    pub return_profile_id: Option<Option<i64>>,
    /// Doubly optional for the same reason: an explicit null removes the
    /// tracking error, which a zero would only approximate (a zero still
    /// reads as "set" to anything asking whether one was chosen).
    #[serde(default, deserialize_with = "crate::specs::double_option")]
    #[ts(optional, type = "number | null")]
    pub tracking_error: Option<Option<f64>>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}
