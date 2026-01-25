//! Platform abstraction layer for native/web compatibility.
//!
//! This module provides traits that abstract platform-specific functionality:
//! - [`Storage`]: Persistence for scenarios and configuration
//! - [`SimulationWorker`]: Background simulation execution
//!
//! Each trait has implementations for native (using filesystem and threads)
//! and web (using browser APIs and Web Workers).

mod storage;
mod worker;

#[cfg(feature = "native")]
pub mod native;

// TODO: Add web module when implementing Phase 4
// #[cfg(feature = "web")]
// pub mod web;

pub use storage::{LoadResult, Storage, StorageError};
pub use worker::{SimulationRequest, SimulationResponse, SimulationWorker};

// Re-export platform-specific implementations
#[cfg(feature = "native")]
pub use native::{NativeStorage, NativeWorker};
