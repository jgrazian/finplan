//! Common utility functions for form parsing and options

use std::thread::available_parallelism;

/// Parse "Yes"/"No" strings to bool
///
/// Accepts: "Yes", "Y", "yes", "y", "TRUE", "true", "1" -> true
/// Everything else -> false
pub fn parse_yes_no(s: &str) -> bool {
    let lower = s.to_lowercase();
    matches!(lower.as_str(), "yes" | "y" | "true" | "1")
}

/// Get standard yes/no options for form selects
///
/// Returns ["Yes", "No"] - use with `parse_yes_no()` for consistent parsing
pub fn yes_no_options() -> Vec<String> {
    vec!["Yes".to_string(), "No".to_string()]
}

pub fn cpu_parallel_batches() -> usize {
    available_parallelism().map(|n| n.get()).unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yes_no() {
        // Truthy values
        assert!(parse_yes_no("Yes"));
        assert!(parse_yes_no("yes"));
        assert!(parse_yes_no("YES"));
        assert!(parse_yes_no("Y"));
        assert!(parse_yes_no("y"));
        assert!(parse_yes_no("true"));
        assert!(parse_yes_no("TRUE"));
        assert!(parse_yes_no("1"));

        // Falsy values
        assert!(!parse_yes_no("No"));
        assert!(!parse_yes_no("no"));
        assert!(!parse_yes_no("N"));
        assert!(!parse_yes_no("false"));
        assert!(!parse_yes_no("0"));
        assert!(!parse_yes_no(""));
        assert!(!parse_yes_no("Maybe"));
    }

    #[test]
    fn test_yes_no_options() {
        let options = yes_no_options();
        assert_eq!(options, vec!["Yes", "No"]);
    }
}
