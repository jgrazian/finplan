//! Keybindings matching utilities.
//!
//! Provides functions to convert between crossterm KeyEvent and string representations,
//! and to check if a key event matches configured bindings.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;

use crate::data::keybindings_data::KeybindingsConfig;
use crate::data::storage::StorageError;

impl KeybindingsConfig {
    /// Convert a KeyEvent to our string format.
    ///
    /// Examples:
    /// - KeyCode::Char('a') with no modifiers -> "a"
    /// - KeyCode::Char('s') with CONTROL -> "ctrl+s"
    /// - KeyCode::Char('J') with SHIFT -> "shift+j"
    /// - KeyCode::Enter -> "enter"
    /// - KeyCode::Tab with SHIFT -> "shift+tab"
    pub fn key_to_string(key: &KeyEvent) -> String {
        let mut parts = Vec::new();

        // Add modifiers
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("ctrl");
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            parts.push("alt");
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            // Only add shift for non-character keys or when it changes the meaning
            match key.code {
                KeyCode::Char(c) if c.is_uppercase() => {
                    // For uppercase chars, we'll handle shift separately
                }
                KeyCode::Char(_) => {
                    // Don't add shift for lowercase chars
                }
                _ => {
                    parts.push("shift");
                }
            }
        }

        // Add key code
        let key_str = match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::SHIFT) && c.is_uppercase() {
                    // Uppercase letter with shift
                    parts.push("shift");
                    c.to_lowercase().to_string()
                } else {
                    c.to_lowercase().to_string()
                }
            }
            KeyCode::Enter => "enter".to_string(),
            KeyCode::Tab => "tab".to_string(),
            KeyCode::Backspace => "backspace".to_string(),
            KeyCode::Delete => "delete".to_string(),
            KeyCode::Esc => "esc".to_string(),
            KeyCode::Up => "up".to_string(),
            KeyCode::Down => "down".to_string(),
            KeyCode::Left => "left".to_string(),
            KeyCode::Right => "right".to_string(),
            KeyCode::Home => "home".to_string(),
            KeyCode::End => "end".to_string(),
            KeyCode::PageUp => "pageup".to_string(),
            KeyCode::PageDown => "pagedown".to_string(),
            KeyCode::F(n) => format!("f{}", n),
            KeyCode::Insert => "insert".to_string(),
            KeyCode::BackTab => {
                // BackTab is Shift+Tab
                if !parts.contains(&"shift") {
                    parts.push("shift");
                }
                "tab".to_string()
            }
            _ => return String::new(), // Unsupported key
        };

        parts.push(&key_str);
        parts.join("+")
    }

    /// Check if a KeyEvent matches any of the configured bindings.
    pub fn matches(key: &KeyEvent, bindings: &[String]) -> bool {
        let key_str = Self::key_to_string(key);
        if key_str.is_empty() {
            return false;
        }
        bindings.iter().any(|b| b.eq_ignore_ascii_case(&key_str))
    }

    /// Get the keybindings file path
    pub fn path(data_dir: &Path) -> std::path::PathBuf {
        data_dir.join("keybindings.yaml")
    }

    /// Load keybindings from file, returning defaults if file doesn't exist or fails to parse.
    pub fn load_or_default(data_dir: &Path) -> Self {
        let path = Self::path(data_dir);
        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => serde_saphyr::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save keybindings to file.
    pub fn save(&self, data_dir: &Path) -> Result<(), StorageError> {
        let path = Self::path(data_dir);
        let yaml = serde_saphyr::to_string(self).map_err(|e| {
            StorageError::Serialize(format!("Failed to serialize keybindings: {}", e))
        })?;

        std::fs::write(path, yaml)
            .map_err(|e| StorageError::Io(format!("Failed to write keybindings: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_to_string_basic() {
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(KeybindingsConfig::key_to_string(&key), "a");
    }

    #[test]
    fn test_key_to_string_ctrl() {
        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(KeybindingsConfig::key_to_string(&key), "ctrl+s");
    }

    #[test]
    fn test_key_to_string_shift() {
        let key = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(KeybindingsConfig::key_to_string(&key), "shift+j");
    }

    #[test]
    fn test_key_to_string_special() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(KeybindingsConfig::key_to_string(&key), "enter");

        let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(KeybindingsConfig::key_to_string(&key), "tab");

        let key = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(KeybindingsConfig::key_to_string(&key), "shift+tab");
    }

    #[test]
    fn test_matches() {
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let bindings = vec!["j".to_string(), "down".to_string()];
        assert!(KeybindingsConfig::matches(&key, &bindings));

        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert!(KeybindingsConfig::matches(&key, &bindings));

        let key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        assert!(!KeybindingsConfig::matches(&key, &bindings));
    }
}
