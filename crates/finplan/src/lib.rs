pub mod actions;
pub mod app;
pub mod components;
pub mod data;
pub mod logging;
pub mod modals;
pub mod platform;
pub mod screens;
pub mod state;
pub mod util;
pub mod worker;

pub use app::App;
pub use state::AppState;

// Re-export platform abstractions
pub use platform::{
    SimulationRequest, SimulationResponse, SimulationWorker, Storage, StorageError,
};

// Re-export native platform implementations
#[cfg(feature = "native")]
pub use platform::{NativeStorage, NativeWorker};

#[cfg(feature = "native")]
pub use logging::init_logging;

#[cfg(feature = "web")]
pub use logging::init_logging_web;
