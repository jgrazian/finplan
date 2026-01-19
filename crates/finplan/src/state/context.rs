/// Typed modal context system to replace string-based context passing.
///
/// This provides type safety and eliminates the need for string parsing
/// in modal handlers.
use crate::data::events_data::IntervalData;

/// Top-level context enum for modal operations
#[derive(Debug, Clone, PartialEq)]
pub enum ModalContext {
    /// Context for single-index operations (account, profile, event)
    Index(IndexContext),
    /// Context for account type selection/creation
    AccountType(AccountTypeContext),
    /// Context for profile type selection/creation
    ProfileType(ProfileTypeContext),
    /// Context for event trigger configuration
    Trigger(TriggerContext),
    /// Context for effect operations
    Effect(EffectContext),
    /// Context for config operations (tax, inflation)
    Config(ConfigContext),
}

/// Simple index-based context
#[derive(Debug, Clone, PartialEq)]
pub enum IndexContext {
    Account(usize),
    Profile(usize),
    Event(usize),
    /// Holding within an account: (account_index, holding_index)
    Holding {
        account: usize,
        holding: usize,
    },
}

/// Account type context for create/edit
#[derive(Debug, Clone, PartialEq)]
pub enum AccountTypeContext {
    // Investment accounts
    Brokerage,
    Traditional401k,
    Roth401k,
    TraditionalIRA,
    RothIRA,
    // Cash/Property accounts
    Checking,
    Savings,
    HSA,
    Property,
    Collectible,
    // Debt accounts
    Mortgage,
    Loan,
    StudentLoan,
}

impl AccountTypeContext {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Brokerage" => Some(Self::Brokerage),
            "Traditional401k" | "Traditional 401k" => Some(Self::Traditional401k),
            "Roth401k" | "Roth 401k" => Some(Self::Roth401k),
            "TraditionalIRA" | "Traditional IRA" => Some(Self::TraditionalIRA),
            "RothIRA" | "Roth IRA" => Some(Self::RothIRA),
            "Checking" => Some(Self::Checking),
            "Savings" => Some(Self::Savings),
            "HSA" => Some(Self::HSA),
            "Property" => Some(Self::Property),
            "Collectible" => Some(Self::Collectible),
            "Mortgage" => Some(Self::Mortgage),
            "Loan" => Some(Self::Loan),
            "StudentLoan" | "Student Loan" => Some(Self::StudentLoan),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Brokerage => "Brokerage",
            Self::Traditional401k => "Traditional 401k",
            Self::Roth401k => "Roth 401k",
            Self::TraditionalIRA => "Traditional IRA",
            Self::RothIRA => "Roth IRA",
            Self::Checking => "Checking",
            Self::Savings => "Savings",
            Self::HSA => "HSA",
            Self::Property => "Property",
            Self::Collectible => "Collectible",
            Self::Mortgage => "Mortgage",
            Self::Loan => "Loan",
            Self::StudentLoan => "Student Loan",
        }
    }
}

/// Profile type context for create/edit
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileTypeContext {
    Fixed,
    Normal,
    LogNormal,
}

impl ProfileTypeContext {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Fixed" => Some(Self::Fixed),
            "Normal" => Some(Self::Normal),
            "LogNormal" | "Log-Normal" => Some(Self::LogNormal),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Fixed => "Fixed",
            Self::Normal => "Normal",
            Self::LogNormal => "Log-Normal",
        }
    }
}

/// Phase tracking for repeating trigger builder - which slot are we building?
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TriggerChildSlot {
    Start,
    End,
}

/// Partial trigger being built (any type)
#[derive(Debug, Clone, PartialEq)]
pub enum PartialTrigger {
    /// Explicitly no trigger (start immediately / run forever)
    None,
    /// Date-based trigger
    Date { date: Option<String> },
    /// Age-based trigger
    Age { years: Option<u8>, months: Option<u8> },
    /// Manual trigger
    Manual,
    /// Net worth threshold trigger
    NetWorth {
        threshold: Option<f64>,
        comparison: Option<String>,
    },
    /// Account balance threshold trigger
    AccountBalance {
        account: String,
        threshold: Option<f64>,
        comparison: Option<String>,
    },
    /// Relative to another event
    RelativeToEvent {
        event: String,
        offset_years: Option<i32>,
        offset_months: Option<i32>,
    },
    /// Repeating trigger (can contain nested triggers)
    Repeating {
        interval: IntervalData,
        start: Option<Box<PartialTrigger>>,
        end: Option<Box<PartialTrigger>>,
    },
}

impl PartialTrigger {
    /// Check if this partial trigger is complete and can be converted
    pub fn is_complete(&self) -> bool {
        match self {
            PartialTrigger::None => true,
            PartialTrigger::Date { date } => date.is_some(),
            PartialTrigger::Age { years, .. } => years.is_some(),
            PartialTrigger::Manual => true,
            PartialTrigger::NetWorth { threshold, comparison } => {
                threshold.is_some() && comparison.is_some()
            }
            PartialTrigger::AccountBalance { threshold, comparison, .. } => {
                threshold.is_some() && comparison.is_some()
            }
            PartialTrigger::RelativeToEvent { offset_years, offset_months, .. } => {
                offset_years.is_some() || offset_months.is_some()
            }
            PartialTrigger::Repeating { .. } => true, // Always complete at the repeating level
        }
    }

    /// Get a display name for the trigger type
    pub fn type_name(&self) -> &'static str {
        match self {
            PartialTrigger::None => "None",
            PartialTrigger::Date { .. } => "Date",
            PartialTrigger::Age { .. } => "Age",
            PartialTrigger::Manual => "Manual",
            PartialTrigger::NetWorth { .. } => "Net Worth",
            PartialTrigger::AccountBalance { .. } => "Account Balance",
            PartialTrigger::RelativeToEvent { .. } => "Relative to Event",
            PartialTrigger::Repeating { .. } => "Repeating",
        }
    }
}

/// Builder state for constructing triggers recursively
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerBuilderState {
    /// Current trigger being built
    pub current: PartialTrigger,
    /// Stack of parents for nested building: (parent, which_slot)
    pub parent_stack: Vec<(PartialTrigger, TriggerChildSlot)>,
    /// Event metadata
    pub event_name: Option<String>,
    pub event_description: Option<String>,
}

impl TriggerBuilderState {
    /// Create a new builder for a repeating trigger
    pub fn new_repeating(interval: IntervalData) -> Self {
        Self {
            current: PartialTrigger::Repeating {
                interval,
                start: None,
                end: None,
            },
            parent_stack: Vec::new(),
            event_name: None,
            event_description: None,
        }
    }

    /// Push into a child slot (start building nested trigger)
    pub fn push_child(&mut self, slot: TriggerChildSlot, child: PartialTrigger) {
        let parent = std::mem::replace(&mut self.current, child);
        self.parent_stack.push((parent, slot));
    }

    /// Pop back to parent after completing nested trigger
    /// Returns true if pop was successful, false if already at root
    pub fn pop_to_parent(&mut self) -> bool {
        if let Some((mut parent, slot)) = self.parent_stack.pop() {
            let completed_child = std::mem::replace(&mut self.current, PartialTrigger::None);

            // Insert completed child into parent's appropriate slot
            if let PartialTrigger::Repeating { start, end, .. } = &mut parent {
                let boxed = if matches!(completed_child, PartialTrigger::None) {
                    None
                } else {
                    Some(Box::new(completed_child))
                };
                match slot {
                    TriggerChildSlot::Start => *start = boxed,
                    TriggerChildSlot::End => *end = boxed,
                }
            }

            self.current = parent;
            true
        } else {
            false
        }
    }

    /// Check if we're at the root level (not building a nested trigger)
    pub fn is_at_root(&self) -> bool {
        self.parent_stack.is_empty()
    }

    /// Get the current nesting depth
    pub fn depth(&self) -> usize {
        self.parent_stack.len()
    }

    /// Get the current phase based on what's been built
    /// Returns Start if start hasn't been set, End if start is set but end isn't
    pub fn current_phase(&self) -> Option<TriggerChildSlot> {
        if !self.is_at_root() {
            return None; // We're building a nested trigger
        }

        if let PartialTrigger::Repeating { start, end, .. } = &self.current {
            if start.is_none() {
                Some(TriggerChildSlot::Start)
            } else if end.is_none() {
                Some(TriggerChildSlot::End)
            } else {
                None // Both are set
            }
        } else {
            None
        }
    }
}

/// Event trigger context for create/edit
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerContext {
    /// Date-based trigger (no additional context needed)
    Date,
    /// Age-based trigger (no additional context needed)
    Age,
    /// Manual trigger (no additional context needed)
    Manual,
    /// Net worth trigger (no additional context needed)
    NetWorth,
    /// Repeating trigger with interval (simple mode)
    Repeating(IntervalData),
    /// Account balance trigger with account name
    AccountBalance(String),
    /// Relative to event trigger with event name reference
    RelativeToEvent(String),
    /// Full builder state for recursive trigger construction
    RepeatingBuilder(TriggerBuilderState),
}

impl TriggerContext {
    /// Get the trigger type name for display
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Date => "Date",
            Self::Age => "Age",
            Self::Manual => "Manual",
            Self::NetWorth => "Net Worth",
            Self::Repeating(_) => "Repeating",
            Self::AccountBalance(_) => "Account Balance",
            Self::RelativeToEvent(_) => "Relative to Event",
            Self::RepeatingBuilder(_) => "Repeating",
        }
    }
}

/// Effect context for add/edit/delete operations
#[derive(Debug, Clone, PartialEq)]
pub enum EffectContext {
    /// Effect within an event: (event_index, effect_index)
    Existing { event: usize, effect: usize },
    /// Adding a new effect of a specific type to an event
    Add {
        event: usize,
        effect_type: EffectTypeContext,
    },
}

/// Effect type context for creation
#[derive(Debug, Clone, PartialEq)]
pub enum EffectTypeContext {
    Income,
    Expense,
    TriggerEvent,
    PauseEvent,
    ResumeEvent,
    TerminateEvent,
    Transfer,
    AssetAllocation,
    Withdrawal,
    Contribution,
}

impl EffectTypeContext {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Income" => Some(Self::Income),
            "Expense" => Some(Self::Expense),
            "TriggerEvent" | "Trigger Event" => Some(Self::TriggerEvent),
            "PauseEvent" | "Pause Event" => Some(Self::PauseEvent),
            "ResumeEvent" | "Resume Event" => Some(Self::ResumeEvent),
            "TerminateEvent" | "Terminate Event" => Some(Self::TerminateEvent),
            "Transfer" => Some(Self::Transfer),
            "AssetAllocation" | "Asset Allocation" => Some(Self::AssetAllocation),
            "Withdrawal" => Some(Self::Withdrawal),
            "Contribution" => Some(Self::Contribution),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Income => "Income",
            Self::Expense => "Expense",
            Self::TriggerEvent => "Trigger Event",
            Self::PauseEvent => "Pause Event",
            Self::ResumeEvent => "Resume Event",
            Self::TerminateEvent => "Terminate Event",
            Self::Transfer => "Transfer",
            Self::AssetAllocation => "Asset Allocation",
            Self::Withdrawal => "Withdrawal",
            Self::Contribution => "Contribution",
        }
    }
}

/// Config context for tax/inflation editing
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigContext {
    Tax(TaxConfigContext),
    Inflation(InflationConfigContext),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaxConfigContext {
    StateRate,
    FederalBrackets,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InflationConfigContext {
    Fixed,
    Normal,
    LogNormal,
}

impl InflationConfigContext {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Fixed" => Some(Self::Fixed),
            "Normal" => Some(Self::Normal),
            "LogNormal" | "Log-Normal" => Some(Self::LogNormal),
            _ => None,
        }
    }
}

// Convenience constructors
impl ModalContext {
    pub fn account_index(idx: usize) -> Self {
        Self::Index(IndexContext::Account(idx))
    }

    pub fn profile_index(idx: usize) -> Self {
        Self::Index(IndexContext::Profile(idx))
    }

    pub fn event_index(idx: usize) -> Self {
        Self::Index(IndexContext::Event(idx))
    }

    pub fn holding_index(account: usize, holding: usize) -> Self {
        Self::Index(IndexContext::Holding { account, holding })
    }

    pub fn effect_existing(event: usize, effect: usize) -> Self {
        Self::Effect(EffectContext::Existing { event, effect })
    }

    pub fn effect_add(event: usize, effect_type: EffectTypeContext) -> Self {
        Self::Effect(EffectContext::Add { event, effect_type })
    }
}

// Extraction helpers
impl ModalContext {
    /// Extract account index if this is an account index context
    pub fn as_account_index(&self) -> Option<usize> {
        match self {
            Self::Index(IndexContext::Account(idx)) => Some(*idx),
            _ => None,
        }
    }

    /// Extract profile index if this is a profile index context
    pub fn as_profile_index(&self) -> Option<usize> {
        match self {
            Self::Index(IndexContext::Profile(idx)) => Some(*idx),
            _ => None,
        }
    }

    /// Extract event index if this is an event index context
    pub fn as_event_index(&self) -> Option<usize> {
        match self {
            Self::Index(IndexContext::Event(idx)) => Some(*idx),
            _ => None,
        }
    }

    /// Extract holding indices if this is a holding context
    pub fn as_holding_index(&self) -> Option<(usize, usize)> {
        match self {
            Self::Index(IndexContext::Holding { account, holding }) => Some((*account, *holding)),
            _ => None,
        }
    }

    /// Extract account type context
    pub fn as_account_type(&self) -> Option<&AccountTypeContext> {
        match self {
            Self::AccountType(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Extract profile type context
    pub fn as_profile_type(&self) -> Option<&ProfileTypeContext> {
        match self {
            Self::ProfileType(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Extract trigger context
    pub fn as_trigger(&self) -> Option<&TriggerContext> {
        match self {
            Self::Trigger(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Extract effect context
    pub fn as_effect(&self) -> Option<&EffectContext> {
        match self {
            Self::Effect(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Extract config context
    pub fn as_config(&self) -> Option<&ConfigContext> {
        match self {
            Self::Config(ctx) => Some(ctx),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_type_from_str() {
        assert_eq!(
            AccountTypeContext::from_str("Brokerage"),
            Some(AccountTypeContext::Brokerage)
        );
        assert_eq!(
            AccountTypeContext::from_str("Traditional 401k"),
            Some(AccountTypeContext::Traditional401k)
        );
        assert_eq!(AccountTypeContext::from_str("invalid"), None);
    }

    #[test]
    fn test_context_extractors() {
        let ctx = ModalContext::account_index(5);
        assert_eq!(ctx.as_account_index(), Some(5));
        assert_eq!(ctx.as_profile_index(), None);

        let ctx = ModalContext::holding_index(2, 3);
        assert_eq!(ctx.as_holding_index(), Some((2, 3)));
    }
}
