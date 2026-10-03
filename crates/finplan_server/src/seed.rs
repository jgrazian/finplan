//! Starter data for a new account.
//!
//! A scenario cannot reference a return profile that does not exist, so every
//! user is given a small library of market assumptions and a default tax table
//! at registration. These are ordinary rows: the user can edit or delete them.
//!
//! The data itself is `finplan_plan::library::seed()`, so the browser's local
//! library and the server's start from one copy; this only writes it.

use crate::db::Db;
use crate::error::ApiResult;

pub async fn seed_user_library(db: &Db, user_id: &str) -> ApiResult<()> {
    let mut tx = db.begin().await?;
    crate::db::library::insert(&mut tx, user_id, &finplan_plan::library::seed()).await?;
    tx.commit().await?;
    Ok(())
}
