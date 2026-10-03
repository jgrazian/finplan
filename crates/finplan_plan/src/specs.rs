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

/// How a loan pays itself off: a level monthly payment from a cash account.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RepaymentSpec {
    /// Bank or investment account the payment is drawn from.
    pub from_account_id: i64,
    /// Months remaining at plan start — 360 for a new 30-year mortgage. A
    /// loan drawn by a BuyProperty takes the term that effect names instead.
    pub term_months: u32,
}

/// Both columns or neither: the payer set to NULL by its deletion leaves a
/// term with nothing to draw from, which reads as no repayment.
pub fn repayment_of(from: Option<i64>, term: Option<i64>) -> Option<RepaymentSpec> {
    Some(RepaymentSpec {
        from_account_id: from?,
        term_months: u32::try_from(term?).ok().filter(|t| *t > 0)?,
    })
}
