//! Amount builder state for recursive amount editing.
//!
//! This module provides `AmountBuilderState` for navigating and editing recursive
//! `AmountData` structures (e.g., InflationAdjusted wrapping Scale wrapping Fixed).

use crate::data::events_data::{AccountTag, AmountData};

/// Path segment indicating position within a recursive AmountData structure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmountPath {
    /// Inside InflationAdjusted.inner
    InflationAdjustedInner,
    /// Inside Scale.inner
    ScaleInner,
}

/// State for building recursive AmountData expressions.
///
/// Similar to TriggerBuilderState, this enables nested editing of amount types.
#[derive(Debug, Clone)]
pub struct AmountBuilderState {
    /// The root amount being built
    pub root: AmountData,
    /// Stack of parent paths for nested editing
    pub path: Vec<AmountPath>,
}

impl AmountBuilderState {
    /// Create a new builder with the given initial amount
    pub fn new(initial: AmountData) -> Self {
        Self {
            root: initial,
            path: Vec::new(),
        }
    }

    /// Create a new builder with a default Fixed(0) amount
    pub fn default_fixed() -> Self {
        Self::new(AmountData::fixed(0.0))
    }

    /// Check if at root level (not nested)
    pub fn is_at_root(&self) -> bool {
        self.path.is_empty()
    }

    /// Get the current nesting depth
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// Get the currently focused node (immutable)
    pub fn current(&self) -> &AmountData {
        let mut node = &self.root;
        for segment in &self.path {
            node = match (segment, node) {
                (AmountPath::InflationAdjustedInner, AmountData::InflationAdjusted { inner }) => {
                    inner
                }
                (AmountPath::ScaleInner, AmountData::Scale { inner, .. }) => inner,
                _ => return node, // Path mismatch, stay at current
            };
        }
        node
    }

    /// Get mutable reference to current node
    pub fn current_mut(&mut self) -> &mut AmountData {
        // Navigate through path to find current node
        let depth = self.path.len();
        Self::get_at_depth(&mut self.root, &self.path[..depth])
    }

    /// Helper to navigate to a specific depth (recursive to satisfy borrow checker)
    fn get_at_depth<'a>(node: &'a mut AmountData, path: &[AmountPath]) -> &'a mut AmountData {
        if path.is_empty() {
            return node;
        }

        let segment = &path[0];
        let rest = &path[1..];

        match (segment, node) {
            (AmountPath::InflationAdjustedInner, AmountData::InflationAdjusted { inner }) => {
                Self::get_at_depth(inner.as_mut(), rest)
            }
            (AmountPath::ScaleInner, AmountData::Scale { inner, .. }) => {
                Self::get_at_depth(inner.as_mut(), rest)
            }
            // Path mismatch - this shouldn't happen with valid path, but return current node
            (_, node) => node,
        }
    }

    /// Replace the current node with a new value
    pub fn set_current(&mut self, value: AmountData) {
        *self.current_mut() = value;
    }

    /// Wrap the current node in InflationAdjusted and descend into it
    pub fn wrap_inflation_adjusted(&mut self) {
        let current = self.current_mut();
        let inner = std::mem::replace(current, AmountData::fixed(0.0));
        *current = AmountData::InflationAdjusted {
            inner: Box::new(inner),
        };
        self.path.push(AmountPath::InflationAdjustedInner);
    }

    /// Wrap the current node in Scale and descend into it
    pub fn wrap_scale(&mut self, multiplier: f64) {
        let current = self.current_mut();
        let inner = std::mem::replace(current, AmountData::fixed(0.0));
        *current = AmountData::Scale {
            multiplier,
            inner: Box::new(inner),
        };
        self.path.push(AmountPath::ScaleInner);
    }

    /// Descend into the inner amount (if current is InflationAdjusted or Scale)
    pub fn descend(&mut self) -> bool {
        let path_segment = match self.current() {
            AmountData::InflationAdjusted { .. } => Some(AmountPath::InflationAdjustedInner),
            AmountData::Scale { .. } => Some(AmountPath::ScaleInner),
            _ => None,
        };

        if let Some(segment) = path_segment {
            self.path.push(segment);
            true
        } else {
            false
        }
    }

    /// Navigate up to parent (returns false if already at root)
    pub fn pop(&mut self) -> bool {
        self.path.pop().is_some()
    }

    /// Unwrap the current node (remove InflationAdjusted or Scale wrapper)
    /// Returns false if current node isn't a wrapper type
    pub fn unwrap(&mut self) -> bool {
        let current = self.current_mut();
        match current {
            AmountData::InflationAdjusted { inner } => {
                let inner_val = std::mem::replace(inner.as_mut(), AmountData::fixed(0.0));
                *current = inner_val;
                true
            }
            AmountData::Scale { inner, .. } => {
                let inner_val = std::mem::replace(inner.as_mut(), AmountData::fixed(0.0));
                *current = inner_val;
                true
            }
            _ => false,
        }
    }

    /// Get human-readable description of current path
    pub fn path_description(&self) -> String {
        if self.path.is_empty() {
            return "Root".to_string();
        }

        let parts: Vec<&str> = self
            .path
            .iter()
            .map(|p| match p {
                AmountPath::InflationAdjustedInner => "Inflation Adjusted",
                AmountPath::ScaleInner => "Scaled",
            })
            .collect();

        format!("Root > {}", parts.join(" > "))
    }

    /// Get a human-readable summary of the entire amount structure
    pub fn summary(&self) -> String {
        format_amount_summary(&self.root)
    }

    /// Get the final built amount
    pub fn build(self) -> AmountData {
        self.root
    }
}

/// Format an AmountData as a human-readable summary
pub fn format_amount_summary(amount: &AmountData) -> String {
    match amount {
        AmountData::Fixed { value } => format!("${:.2}", value),
        AmountData::InflationAdjusted { inner } => {
            format!("{} (inflation-adjusted)", format_amount_summary(inner))
        }
        AmountData::Scale { multiplier, inner } => {
            format!(
                "{:.1}% of {}",
                multiplier * 100.0,
                format_base_amount(inner)
            )
        }
        AmountData::SourceBalance => "Source balance".to_string(),
        AmountData::ZeroTargetBalance => "Zero target balance".to_string(),
        AmountData::TargetToBalance { target } => format!("To ${:.2}", target),
        AmountData::AccountBalance { account } => format!("{} balance", account.0),
        AmountData::AccountCashBalance { account } => format!("{} cash balance", account.0),
    }
}

/// Format the base (non-wrapper) part of an amount for Scale display
fn format_base_amount(amount: &AmountData) -> String {
    match amount {
        AmountData::Fixed { value } => format!("${:.2}", value),
        AmountData::InflationAdjusted { inner } => {
            format!("{} (infl-adj)", format_base_amount(inner))
        }
        AmountData::Scale { multiplier, inner } => {
            format!(
                "{:.1}% of {}",
                multiplier * 100.0,
                format_base_amount(inner)
            )
        }
        AmountData::SourceBalance => "source balance".to_string(),
        AmountData::ZeroTargetBalance => "zero target".to_string(),
        AmountData::TargetToBalance { target } => format!("target ${:.2}", target),
        AmountData::AccountBalance { account } => format!("{} balance", account.0),
        AmountData::AccountCashBalance { account } => format!("{} cash", account.0),
    }
}

/// Options for amount type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountTypeOption {
    Fixed,
    InflationAdjusted,
    Scale,
    SourceBalance,
    ZeroTargetBalance,
    TargetToBalance,
    AccountBalance,
    AccountCashBalance,
}

impl AmountTypeOption {
    /// Get all options
    pub fn all() -> &'static [AmountTypeOption] {
        &[
            AmountTypeOption::Fixed,
            AmountTypeOption::InflationAdjusted,
            AmountTypeOption::Scale,
            AmountTypeOption::SourceBalance,
            AmountTypeOption::ZeroTargetBalance,
            AmountTypeOption::TargetToBalance,
            AmountTypeOption::AccountBalance,
            AmountTypeOption::AccountCashBalance,
        ]
    }

    /// Get display name for the option
    pub fn display_name(&self) -> &'static str {
        match self {
            AmountTypeOption::Fixed => "Fixed Amount ($X)",
            AmountTypeOption::InflationAdjusted => "Inflation-Adjusted",
            AmountTypeOption::Scale => "Percentage/Scale",
            AmountTypeOption::SourceBalance => "Source Balance",
            AmountTypeOption::ZeroTargetBalance => "Zero Target Balance",
            AmountTypeOption::TargetToBalance => "Target To Balance",
            AmountTypeOption::AccountBalance => "Account Balance",
            AmountTypeOption::AccountCashBalance => "Account Cash Balance",
        }
    }

    /// Get option names as strings for picker
    pub fn option_strings() -> Vec<String> {
        Self::all()
            .iter()
            .map(|o| o.display_name().to_string())
            .collect()
    }

    /// Parse from display name
    pub fn from_display_name(name: &str) -> Option<Self> {
        Self::all()
            .iter()
            .find(|o| o.display_name() == name)
            .copied()
    }

    /// Create a default AmountData for this type
    pub fn default_amount(&self) -> AmountData {
        match self {
            AmountTypeOption::Fixed => AmountData::fixed(0.0),
            AmountTypeOption::InflationAdjusted => {
                AmountData::inflation_adjusted(AmountData::fixed(0.0))
            }
            AmountTypeOption::Scale => AmountData::scale(0.04, AmountData::fixed(0.0)),
            AmountTypeOption::SourceBalance => AmountData::SourceBalance,
            AmountTypeOption::ZeroTargetBalance => AmountData::ZeroTargetBalance,
            AmountTypeOption::TargetToBalance => AmountData::TargetToBalance { target: 0.0 },
            AmountTypeOption::AccountBalance => AmountData::AccountBalance {
                account: AccountTag(String::new()),
            },
            AmountTypeOption::AccountCashBalance => AmountData::AccountCashBalance {
                account: AccountTag(String::new()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_navigation() {
        let amount = AmountData::inflation_adjusted(AmountData::scale(
            0.04,
            AmountData::AccountBalance {
                account: AccountTag("Brokerage".to_string()),
            },
        ));

        let mut builder = AmountBuilderState::new(amount);

        // At root - should be InflationAdjusted
        assert!(builder.is_at_root());
        assert!(matches!(
            builder.current(),
            AmountData::InflationAdjusted { .. }
        ));

        // Descend into InflationAdjusted.inner - should be Scale
        assert!(builder.descend());
        assert!(!builder.is_at_root());
        assert!(matches!(builder.current(), AmountData::Scale { .. }));

        // Descend into Scale.inner - should be AccountBalance
        assert!(builder.descend());
        assert_eq!(builder.depth(), 2);
        assert!(matches!(
            builder.current(),
            AmountData::AccountBalance { .. }
        ));

        // Can't descend further
        assert!(!builder.descend());

        // Pop back up
        assert!(builder.pop());
        assert!(matches!(builder.current(), AmountData::Scale { .. }));

        assert!(builder.pop());
        assert!(builder.is_at_root());

        // Can't pop past root
        assert!(!builder.pop());
    }

    #[test]
    fn test_builder_wrap() {
        let mut builder = AmountBuilderState::new(AmountData::fixed(1000.0));

        // Wrap in inflation adjusted
        builder.wrap_inflation_adjusted();
        assert!(!builder.is_at_root()); // Now inside the wrapper
        assert!(matches!(builder.current(), AmountData::Fixed { .. }));

        // Pop to see the wrapper
        builder.pop();
        assert!(matches!(
            builder.current(),
            AmountData::InflationAdjusted { .. }
        ));

        // Summary should reflect structure
        let summary = builder.summary();
        assert!(summary.contains("inflation-adjusted"));
    }

    #[test]
    fn test_builder_set_current() {
        let mut builder = AmountBuilderState::new(AmountData::fixed(0.0));

        builder.set_current(AmountData::SourceBalance);
        assert!(matches!(builder.current(), AmountData::SourceBalance));
    }

    #[test]
    fn test_format_summary() {
        assert_eq!(
            format_amount_summary(&AmountData::fixed(1000.0)),
            "$1000.00"
        );

        assert_eq!(
            format_amount_summary(&AmountData::inflation_adjusted(AmountData::fixed(5000.0))),
            "$5000.00 (inflation-adjusted)"
        );

        assert_eq!(
            format_amount_summary(&AmountData::scale(
                0.04,
                AmountData::AccountBalance {
                    account: AccountTag("Brokerage".to_string())
                }
            )),
            "4.0% of Brokerage balance"
        );
    }
}
