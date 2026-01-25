//! Platform-agnostic keyboard event types.
//!
//! This module provides a unified event type that works with both:
//! - Native: crossterm::event::KeyEvent
//! - Web: ratzilla::event::KeyEvent

/// Key code abstraction that works on both native and web.
/// Re-exports from crossterm on native, from ratzilla on web.
#[cfg(feature = "native")]
pub use crossterm::event::KeyCode;

#[cfg(feature = "web")]
pub use ratzilla::event::KeyCode;

/// Unified key event that abstracts over platform-specific implementations.
#[derive(Debug, Clone)]
pub struct AppKeyEvent {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl AppKeyEvent {
    /// Check if control modifier is pressed.
    pub fn ctrl(&self) -> bool {
        self.ctrl
    }

    /// Check if alt modifier is pressed.
    pub fn alt(&self) -> bool {
        self.alt
    }

    /// Check if shift modifier is pressed.
    pub fn shift(&self) -> bool {
        self.shift
    }

    /// Check if no modifiers are pressed.
    pub fn no_modifiers(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift
    }

    /// Check if this is a "back tab" (Shift+Tab).
    /// On native, this matches KeyCode::BackTab.
    /// On web, this matches Shift+Tab since BackTab doesn't exist.
    #[cfg(feature = "native")]
    pub fn is_back_tab(&self) -> bool {
        matches!(self.code, KeyCode::BackTab)
    }

    #[cfg(feature = "web")]
    pub fn is_back_tab(&self) -> bool {
        matches!(self.code, KeyCode::Tab) && self.shift
    }
}

#[cfg(feature = "native")]
impl From<crossterm::event::KeyEvent> for AppKeyEvent {
    fn from(event: crossterm::event::KeyEvent) -> Self {
        use crossterm::event::KeyModifiers;
        Self {
            code: event.code,
            ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
            alt: event.modifiers.contains(KeyModifiers::ALT),
            shift: event.modifiers.contains(KeyModifiers::SHIFT),
        }
    }
}

#[cfg(feature = "web")]
impl From<ratzilla::event::KeyEvent> for AppKeyEvent {
    fn from(event: ratzilla::event::KeyEvent) -> Self {
        Self {
            code: event.code,
            ctrl: event.ctrl,
            alt: event.alt,
            shift: event.shift,
        }
    }
}

#[cfg(feature = "web")]
impl From<&ratzilla::event::KeyEvent> for AppKeyEvent {
    fn from(event: &ratzilla::event::KeyEvent) -> Self {
        Self {
            code: event.code.clone(),
            ctrl: event.ctrl,
            alt: event.alt,
            shift: event.shift,
        }
    }
}
