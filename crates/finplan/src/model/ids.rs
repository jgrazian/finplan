//! Unique identifiers for simulation entities
//!
//! Each entity type has its own ID type to provide type safety and prevent
//! mixing up different kinds of identifiers.

use serde::{Deserialize, Serialize};

/// Unique identifier for an Account within a simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(pub u16);

/// Unique identifier for an Asset within a simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(pub u16);

/// Unique identifier for an Event within a simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub u16);
