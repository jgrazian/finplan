// Shared modules (available on all platforms)
pub mod data;
pub mod logging;
pub mod platform;
pub mod state;
pub mod util;

// Native-only modules (use crossterm/threads)
#[cfg(feature = "native")]
pub mod actions;
#[cfg(feature = "native")]
pub mod app;
#[cfg(feature = "native")]
pub mod components;
#[cfg(feature = "native")]
pub mod modals;
#[cfg(feature = "native")]
pub mod screens;
#[cfg(feature = "native")]
pub mod worker;

#[cfg(feature = "native")]
pub use app::App;
pub use state::AppState;

// Re-export platform abstractions
pub use platform::{
    SimulationRequest, SimulationResponse, SimulationWorker, Storage, StorageError,
};

// Re-export native platform implementations
#[cfg(feature = "native")]
pub use platform::{NativeStorage, NativeWorker};

// Re-export web platform implementations
#[cfg(feature = "web")]
pub use platform::{WebStorage, WebWorker};

#[cfg(feature = "native")]
pub use logging::init_logging;

#[cfg(feature = "web")]
pub use logging::init_logging_web;
