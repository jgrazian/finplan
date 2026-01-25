// Shared modules (available on all platforms)
pub mod data;
pub mod event;
pub mod logging;
pub mod platform;
pub mod state;
pub mod util;

// UI modules - now platform-agnostic thanks to AppKeyEvent abstraction
pub mod actions;
pub mod components;
pub mod modals;
pub mod screens;

// Native-only modules (use crossterm event loop / threads)
#[cfg(feature = "native")]
pub mod app;
#[cfg(feature = "native")]
pub mod worker;

// Web-only entry point
#[cfg(feature = "web")]
pub mod web;

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
