pub mod portfolio;
pub mod profiles;
pub mod scenario;
pub mod events;
pub mod results;

use crate::components::Component;

/// Trait for full screen views
pub trait Screen: Component {
    /// Get the screen title
    fn title(&self) -> &str;
}
