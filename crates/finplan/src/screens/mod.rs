pub mod events;
pub mod portfolio_profiles;
pub mod results;
pub mod scenario;

use crate::components::Component;

/// Trait for full screen views
pub trait Screen: Component {
    /// Get the screen title
    fn title(&self) -> &str;
}
