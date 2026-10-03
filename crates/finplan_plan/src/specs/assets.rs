//! Asset request bodies.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{PlanError, PlanResult};

/// What a second asset of the same name is refused with.
pub const NAME_TAKEN: &str = "an asset with that name already exists";

/// An asset's price is what a unit costs, so it is above zero.
pub fn check_initial_price(price: f64) -> PlanResult<()> {
    if price <= 0.0 {
        return Err(PlanError::invalid("initial_price must be positive"));
    }
    Ok(())
}

#[derive(Debug, Serialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[ts(export)]
pub struct Asset {
    pub id: i64,
    /// The ticker symbol (the Ticker column), e.g. `VBTLX`.
    pub name: String,
    /// The fund's full name (the Name column).
    pub description: Option<String>,
    pub initial_price: f64,
    /// Null while the asset is unmapped — it has a price, but nothing yet
    /// making it move. A run compiles such an asset at flat zero growth.
    pub return_profile_id: Option<i64>,
    pub tracking_error: Option<f64>,
    pub sort_order: i64,
}

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
