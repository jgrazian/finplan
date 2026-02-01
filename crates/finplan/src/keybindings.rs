//! Keybindings matching utilities.
//!
//! Provides functions to convert between KeyEvent (or AppKeyEvent) and string representations,
//! and to check if a key event matches configured bindings.

#[cfg(feature = "native")]
use std::path::Path;

use crate::data::keybindings_data::KeybindingsConfig;
use crate::event::{AppKeyEvent, KeyCode};

impl KeybindingsConfig {
    /// Convert an AppKeyEvent to our string format.
    ///
    /// Examples:
    /// - KeyCode::Char('a') with no modifiers -> "a"
    /// - KeyCode::Char('s') with ctrl -> "ctrl+s"
    /// - KeyCode::Char('J') with shift -> "shift+j"
    /// - KeyCode::Enter -> "enter"
    /// - KeyCode::Tab with shift -> "shift+tab"
    pub fn app_key_to_string(key: &AppKeyEvent) -> String {
        let mut parts = Vec::new();

        // Add modifiers
        if key.ctrl {
            parts.push("ctrl");
        }
        if key.alt {
            parts.push("alt");
        }

        // Add key code
        let key_str = match &key.code {
            KeyCode::Char(c) => {
                // For shifted uppercase letters
                if key.shift && c.is_uppercase() {
                    parts.push("shift");
                    c.to_lowercase().to_string()
                } else if key.shift && !c.is_alphabetic() {
                    // For shift + non-alpha keys (like shift+1 for !)
                    parts.push("shift");
                    c.to_string()
                } else {
                    c.to_lowercase().to_string()
                }
            }
            KeyCode::Enter => "enter".to_string(),
            KeyCode::Tab => {
                if key.shift {
                    parts.push("shift");
                }
                "tab".to_string()
            }
            KeyCode::Backspace => "backspace".to_string(),
            KeyCode::Delete => "delete".to_string(),
            KeyCode::Esc => "esc".to_string(),
            KeyCode::Up => {
                if key.shift {
                    parts.push("shift");
                }
                "up".to_string()
            }
            KeyCode::Down => {
                if key.shift {
                    parts.push("shift");
                }
                "down".to_string()
            }
            KeyCode::Left => {
                if key.shift {
                    parts.push("shift");
                }
                "left".to_string()
            }
            KeyCode::Right => {
                if key.shift {
                    parts.push("shift");
                }
                "right".to_string()
            }
            KeyCode::Home => "home".to_string(),
            KeyCode::End => "end".to_string(),
            KeyCode::PageUp => "pageup".to_string(),
            KeyCode::PageDown => "pagedown".to_string(),
            KeyCode::F(n) => format!("f{}", n),
            #[cfg(feature = "native")]
            KeyCode::Insert => "insert".to_string(),
            #[cfg(feature = "native")]
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

    /// Check if an AppKeyEvent matches any of the configured bindings.
    pub fn matches(key: &AppKeyEvent, bindings: &[String]) -> bool {
        let key_str = Self::app_key_to_string(key);
        if key_str.is_empty() {
            return false;
        }
        bindings.iter().any(|b| b.eq_ignore_ascii_case(&key_str))
    }

    /// Get the keybindings file path
    #[cfg(feature = "native")]
    pub fn path(data_dir: &Path) -> std::path::PathBuf {
        data_dir.join("keybindings.yaml")
    }

    /// Load keybindings from file, returning defaults if file doesn't exist or fails to parse.
    #[cfg(feature = "native")]
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
    #[cfg(feature = "native")]
    pub fn save(&self, data_dir: &Path) -> Result<(), crate::data::storage::StorageError> {
        let path = Self::path(data_dir);
        let yaml = serde_saphyr::to_string(self).map_err(|e| {
            crate::data::storage::StorageError::Serialize(format!(
                "Failed to serialize keybindings: {}",
                e
            ))
        })?;

        std::fs::write(path, yaml).map_err(|e| {
            crate::data::storage::StorageError::Io(format!("Failed to write keybindings: {}", e))
        })
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;

    fn make_key(code: KeyCode, ctrl: bool, alt: bool, shift: bool) -> AppKeyEvent {
        AppKeyEvent {
            code,
            ctrl,
            alt,
            shift,
        }
    }

    #[test]
    fn test_key_to_string_basic() {
        let key = make_key(KeyCode::Char('a'), false, false, false);
        assert_eq!(KeybindingsConfig::app_key_to_string(&key), "a");
    }

    #[test]
    fn test_key_to_string_ctrl() {
        let key = make_key(KeyCode::Char('s'), true, false, false);
        assert_eq!(KeybindingsConfig::app_key_to_string(&key), "ctrl+s");
    }

    #[test]
    fn test_key_to_string_shift() {
        let key = make_key(KeyCode::Char('J'), false, false, true);
        assert_eq!(KeybindingsConfig::app_key_to_string(&key), "shift+j");
    }

    #[test]
    fn test_key_to_string_special() {
        let key = make_key(KeyCode::Enter, false, false, false);
        assert_eq!(KeybindingsConfig::app_key_to_string(&key), "enter");

        let key = make_key(KeyCode::Tab, false, false, false);
        assert_eq!(KeybindingsConfig::app_key_to_string(&key), "tab");

        let key = make_key(KeyCode::Tab, false, false, true);
        assert_eq!(KeybindingsConfig::app_key_to_string(&key), "shift+tab");
    }

    #[test]
    fn test_matches() {
        let key = make_key(KeyCode::Char('j'), false, false, false);
        let bindings = vec!["j".to_string(), "down".to_string()];
        assert!(KeybindingsConfig::matches(&key, &bindings));

        let key = make_key(KeyCode::Down, false, false, false);
        assert!(KeybindingsConfig::matches(&key, &bindings));

        let key = make_key(KeyCode::Char('k'), false, false, false);
        assert!(!KeybindingsConfig::matches(&key, &bindings));
    }
}
