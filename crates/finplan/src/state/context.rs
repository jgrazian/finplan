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

/// Event trigger context for create/edit
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerContext {
    /// Date-based trigger (no additional context needed)
    Date,
    /// Repeating trigger with interval
    Repeating(IntervalData),
    /// Account balance trigger with account name
    AccountBalance(String),
    /// Relative to event trigger with event name reference
    RelativeToEvent(String),
}

impl TriggerContext {
    /// Get the trigger type name for display
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Date => "Date",
            Self::Repeating(_) => "Repeating",
            Self::AccountBalance(_) => "Account Balance",
            Self::RelativeToEvent(_) => "Relative to Event",
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
