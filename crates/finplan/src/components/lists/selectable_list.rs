//! Generic selectable list component and input handling utilities.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
pub fn handle_list_navigation(key: &KeyEvent, selected: &mut usize, total: usize) -> bool {
    if total == 0 {
        return false;
    }

    // Don't handle if shift is pressed (that's for reordering)
    if key.modifiers.contains(KeyModifiers::SHIFT) {
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
    key: &KeyEvent,
    selected: usize,
    total: usize,
) -> Option<(usize, usize)> {
    if total < 2 {
        return None;
    }

    let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);
    if !has_shift {
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
pub fn handle_panel_navigation<P: PanelNavigable>(key: &KeyEvent, focused: &mut P) -> bool {
    match key.code {
        KeyCode::Tab if key.modifiers.is_empty() => {
            *focused = focused.next();
            true
        }
        KeyCode::BackTab => {
            *focused = focused.prev();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_centered_scroll_few_items() {
        // When total items fit in viewport, no scroll needed
        assert_eq!(calculate_centered_scroll(0, 5, 10), 0);
        assert_eq!(calculate_centered_scroll(4, 5, 10), 0);
    }

    #[test]
    fn test_calculate_centered_scroll_beginning() {
        // Near the top, no scroll
        assert_eq!(calculate_centered_scroll(0, 20, 10), 0);
        assert_eq!(calculate_centered_scroll(4, 20, 10), 0);
    }

    #[test]
    fn test_calculate_centered_scroll_middle() {
        // In the middle, center the selection
        let offset = calculate_centered_scroll(10, 20, 10);
        assert!(offset > 0 && offset < 10);
    }

    #[test]
    fn test_calculate_centered_scroll_end() {
        // Near the end, scroll to show last items
        assert_eq!(calculate_centered_scroll(19, 20, 10), 10);
        assert_eq!(calculate_centered_scroll(18, 20, 10), 10);
    }

    #[test]
    fn test_handle_list_navigation_down() {
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let mut selected = 0usize;

        assert!(handle_list_navigation(&key, &mut selected, 5));
        assert_eq!(selected, 1);
    }

    #[test]
    fn test_handle_list_navigation_down_wrap() {
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let mut selected = 4usize;

        assert!(handle_list_navigation(&key, &mut selected, 5));
        assert_eq!(selected, 0);
    }

    #[test]
    fn test_handle_list_navigation_up() {
        let key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        let mut selected = 2usize;

        assert!(handle_list_navigation(&key, &mut selected, 5));
        assert_eq!(selected, 1);
    }

    #[test]
    fn test_handle_list_navigation_up_wrap() {
        let key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        let mut selected = 0usize;

        assert!(handle_list_navigation(&key, &mut selected, 5));
        assert_eq!(selected, 4);
    }

    #[test]
    fn test_handle_list_navigation_empty() {
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let mut selected = 0usize;

        assert!(!handle_list_navigation(&key, &mut selected, 0));
    }

    #[test]
    fn test_handle_list_navigation_ignores_shift() {
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::SHIFT);
        let mut selected = 0usize;

        assert!(!handle_list_navigation(&key, &mut selected, 5));
        assert_eq!(selected, 0);
    }

    #[test]
    fn test_handle_list_reorder_down() {
        let key = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(handle_list_reorder(&key, 2, 5), Some((2, 3)));
    }

    #[test]
    fn test_handle_list_reorder_up() {
        let key = KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT);
        assert_eq!(handle_list_reorder(&key, 2, 5), Some((2, 1)));
    }

    #[test]
    fn test_handle_list_reorder_at_end() {
        let key = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(handle_list_reorder(&key, 4, 5), None);
    }

    #[test]
    fn test_handle_list_reorder_at_start() {
        let key = KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT);
        assert_eq!(handle_list_reorder(&key, 0, 5), None);
    }

    #[test]
    fn test_handle_list_reorder_without_shift() {
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(handle_list_reorder(&key, 2, 5), None);
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    enum TestPanel {
        A,
        B,
        C,
    }

    impl PanelNavigable for TestPanel {
        fn next(self) -> Self {
            match self {
                Self::A => Self::B,
                Self::B => Self::C,
                Self::C => Self::A,
            }
        }

        fn prev(self) -> Self {
            match self {
                Self::A => Self::C,
                Self::B => Self::A,
                Self::C => Self::B,
            }
        }
    }

    #[test]
    fn test_handle_panel_navigation_tab() {
        let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        let mut focused = TestPanel::A;

        assert!(handle_panel_navigation(&key, &mut focused));
        assert_eq!(focused, TestPanel::B);
    }

    #[test]
    fn test_handle_panel_navigation_backtab() {
        let key = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        let mut focused = TestPanel::B;

        assert!(handle_panel_navigation(&key, &mut focused));
        assert_eq!(focused, TestPanel::A);
    }

    #[test]
    fn test_handle_panel_navigation_ignores_other_keys() {
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let mut focused = TestPanel::A;

        assert!(!handle_panel_navigation(&key, &mut focused));
        assert_eq!(focused, TestPanel::A);
    }
}
