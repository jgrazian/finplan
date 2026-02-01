pub mod events;
pub mod optimize;
pub mod portfolio_profiles;
pub mod results;
pub mod scenario;

use crate::components::Component;

pub use crate::modals::ModalHandler;

/// Trait for full screen views
pub trait Screen: Component {
    /// Get the screen title
    fn title(&self) -> &str;
}
