//! Generic selectable list component and input handling utilities.

use crate::event::{AppKeyEvent, KeyCode};

/// Trait for panel enums that support Tab/BackTab navigation.
pub trait PanelNavigable: Copy + Eq {
    /// Get the next panel in the cycle.
    fn next(self) -> Self;
    /// Get the previous panel in the cycle.
    fn prev(self) -> Self;
}

/// Configuration for rendering a selectable list.
pub struct SelectableListConfig<'a> {
    /// Title of the list (shown in block border).
    pub title: &'a str,
    /// Help text (shown at bottom when focused).
    pub help_text: &'a str,
    /// Whether the list is currently focused.
    pub focused: bool,
    /// Current selected index.
    pub selected_index: usize,
}

impl<'a> SelectableListConfig<'a> {
    /// Create a new config with default values.
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            help_text: "",
            focused: false,
            selected_index: 0,
        }
    }

    /// Set whether the list is focused.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set the help text.
    pub fn help_text(mut self, help_text: &'a str) -> Self {
        self.help_text = help_text;
        self
    }

    /// Set the selected index.
    pub fn selected_index(mut self, index: usize) -> Self {
        self.selected_index = index;
        self
    }
}

/// Calculate centered scroll offset for a list.
///
/// Returns the scroll offset that keeps the selected item centered when possible,
/// while handling edge cases near the beginning and end of the list.
///
/// # Arguments
/// * `selected_idx` - The currently selected item index
/// * `total_items` - Total number of items in the list
/// * `visible_count` - Number of items visible in the viewport
///
/// # Returns
/// The scroll offset to apply to keep the selection visible and centered
pub fn calculate_centered_scroll(
    selected_idx: usize,
    total_items: usize,
    visible_count: usize,
) -> usize {
    if total_items <= visible_count {
        return 0;
    }

    let center = visible_count / 2;

    if selected_idx <= center {
        // Near the top: selection moves down from top
        0
    } else if selected_idx >= total_items.saturating_sub(visible_count.saturating_sub(center)) {
        // Near the bottom: keep at least half visible
        total_items.saturating_sub(visible_count)
    } else {
        // Middle: center the selection
        selected_idx.saturating_sub(center)
    }
}

/// Handle j/k or Up/Down list navigation.
///
/// Moves selection up or down with wrapping at boundaries.
///
/// # Arguments
/// * `key` - The key event to handle
/// * `selected` - Mutable reference to the selected index
/// * `total` - Total number of items in the list
///
/// # Returns
/// `true` if the key was handled, `false` otherwise
pub fn handle_list_navigation(key: &AppKeyEvent, selected: &mut usize, total: usize) -> bool {
    if total == 0 {
        return false;
    }

    // Don't handle if shift is pressed (that's for reordering)
    if key.shift {
        return false;
    }

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            *selected = (*selected + 1) % total;
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            *selected = if *selected == 0 {
                total - 1
            } else {
                *selected - 1
            };
            true
        }
        _ => false,
    }
}

/// Handle Shift+J/K or Shift+Up/Down for list reordering.
///
/// Returns the indices to swap if a reorder should happen.
///
/// # Arguments
/// * `key` - The key event to handle
/// * `selected` - Current selected index
/// * `total` - Total number of items in the list
///
/// # Returns
/// `Some((from, to))` if items should be swapped, `None` otherwise
pub fn handle_list_reorder(
    key: &AppKeyEvent,
    selected: usize,
    total: usize,
) -> Option<(usize, usize)> {
    if total < 2 {
        return None;
    }

    if !key.shift {
        return None;
    }

    match key.code {
        KeyCode::Char('J') | KeyCode::Down if selected < total - 1 => {
            Some((selected, selected + 1))
        }
        KeyCode::Char('K') | KeyCode::Up if selected > 0 => Some((selected, selected - 1)),
        _ => None,
    }
}

/// Handle Tab/BackTab panel navigation.
///
/// Cycles through panels using their `next()` and `prev()` methods.
///
/// # Arguments
/// * `key` - The key event to handle
/// * `focused` - Mutable reference to the focused panel
///
/// # Returns
/// `true` if the key was handled, `false` otherwise
pub fn handle_panel_navigation<P: PanelNavigable>(key: &AppKeyEvent, focused: &mut P) -> bool {
    // Handle back-tab (Shift+Tab on web, BackTab on native)
    if key.is_back_tab() {
        *focused = focused.prev();
        return true;
    }

    // Handle forward tab
    if matches!(key.code, KeyCode::Tab) && key.no_modifiers() {
        *focused = focused.next();
        return true;
    }

    false
}
