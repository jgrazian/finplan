//! Translation between stable database ids and the dense `u16` indices that
//! `finplan_core` uses internally.
//!
//! The engine indexes `Market` by `AssetId.0` and `ReturnProfileId.0` directly
//! into dense `Vec`s, so those must be a gapless `0..n` range per simulation.
//! Database ids are stable, sparse and permanent. This map is the seam, built
//! fresh for every compile and kept alongside results so engine output can be
//! attributed back to real rows.

use std::collections::HashMap;

use finplan_core::model::{AccountId, AssetId, EventId, ReturnProfileId};

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Default, Clone)]
pub struct IdMap {
    account_to_dense: HashMap<i64, AccountId>,
    account_to_db: HashMap<u16, i64>,
    asset_to_dense: HashMap<i64, AssetId>,
    asset_to_db: HashMap<u16, i64>,
    event_to_dense: HashMap<i64, EventId>,
    event_to_db: HashMap<u16, i64>,
    profile_to_dense: HashMap<i64, ReturnProfileId>,
    profile_to_db: HashMap<u16, i64>,
}

/// The engine's ids are `u16`, so a scenario cannot exceed this many of any one
/// entity. Far above any plausible plan, but checked rather than truncated.
const MAX_ENTITIES: usize = u16::MAX as usize;

fn next_index(len: usize, what: &'static str) -> ApiResult<u16> {
    if len >= MAX_ENTITIES {
        return Err(ApiError::unprocessable(format!(
            "scenario has too many {what} (limit {MAX_ENTITIES})"
        )));
    }
    Ok(len as u16)
}

impl IdMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern_account(&mut self, db_id: i64) -> ApiResult<AccountId> {
        if let Some(id) = self.account_to_dense.get(&db_id) {
            return Ok(*id);
        }
        let idx = next_index(self.account_to_dense.len(), "accounts")?;
        self.account_to_dense.insert(db_id, AccountId(idx));
        self.account_to_db.insert(idx, db_id);
        Ok(AccountId(idx))
    }

    pub fn intern_asset(&mut self, db_id: i64) -> ApiResult<AssetId> {
        if let Some(id) = self.asset_to_dense.get(&db_id) {
            return Ok(*id);
        }
        let idx = next_index(self.asset_to_dense.len(), "assets")?;
        self.asset_to_dense.insert(db_id, AssetId(idx));
        self.asset_to_db.insert(idx, db_id);
        Ok(AssetId(idx))
    }

    pub fn intern_event(&mut self, db_id: i64) -> ApiResult<EventId> {
        if let Some(id) = self.event_to_dense.get(&db_id) {
            return Ok(*id);
        }
        let idx = next_index(self.event_to_dense.len(), "events")?;
        self.event_to_dense.insert(db_id, EventId(idx));
        self.event_to_db.insert(idx, db_id);
        Ok(EventId(idx))
    }

    pub fn intern_profile(&mut self, db_id: i64) -> ApiResult<ReturnProfileId> {
        if let Some(id) = self.profile_to_dense.get(&db_id) {
            return Ok(*id);
        }
        let idx = next_index(self.profile_to_dense.len(), "return profiles")?;
        self.profile_to_dense.insert(db_id, ReturnProfileId(idx));
        self.profile_to_db.insert(idx, db_id);
        Ok(ReturnProfileId(idx))
    }

    /// Resolve an id that must already have been interned. Effects and triggers
    /// reference accounts/assets/events that the first pass registered, so a
    /// miss here means the row points outside the scenario.
    pub fn account(&self, db_id: i64) -> ApiResult<AccountId> {
        self.account_to_dense.get(&db_id).copied().ok_or_else(|| {
            ApiError::unprocessable(format!("account {db_id} is not part of this scenario"))
        })
    }

    pub fn asset(&self, db_id: i64) -> ApiResult<AssetId> {
        self.asset_to_dense.get(&db_id).copied().ok_or_else(|| {
            ApiError::unprocessable(format!("asset {db_id} is not part of this scenario"))
        })
    }

    pub fn event(&self, db_id: i64) -> ApiResult<EventId> {
        self.event_to_dense.get(&db_id).copied().ok_or_else(|| {
            ApiError::unprocessable(format!("event {db_id} is not part of this scenario"))
        })
    }

    pub fn profile(&self, db_id: i64) -> ApiResult<ReturnProfileId> {
        self.profile_to_dense.get(&db_id).copied().ok_or_else(|| {
            ApiError::unprocessable(format!("return profile {db_id} is not available"))
        })
    }

    // ── reverse direction, for attributing engine output to rows ────────────

    #[must_use]
    pub fn account_db_id(&self, id: AccountId) -> Option<i64> {
        self.account_to_db.get(&id.0).copied()
    }

    #[must_use]
    pub fn asset_db_id(&self, id: AssetId) -> Option<i64> {
        self.asset_to_db.get(&id.0).copied()
    }

    #[must_use]
    pub fn event_db_id(&self, id: EventId) -> Option<i64> {
        self.event_to_db.get(&id.0).copied()
    }

    #[must_use]
    pub fn profile_db_id(&self, id: ReturnProfileId) -> Option<i64> {
        self.profile_to_db.get(&id.0).copied()
    }
}
