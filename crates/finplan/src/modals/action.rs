/// Domain-scoped modal actions for better organization and extensibility.
///
/// This replaces the flat 34-variant ModalAction enum with domain-specific
/// enums that delegate through a top-level ModalAction.
/// Top-level action enum with domain delegation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalAction {
    Scenario(ScenarioAction),
    Account(AccountAction),
    Profile(ProfileAction),
    Holding(HoldingAction),
    Config(ConfigAction),
    Event(EventAction),
    Effect(EffectAction),
    Optimize(OptimizeAction),
}

/// Scenario-specific actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioAction {
    SaveAs,
    Load,
    SwitchTo,
    EditParameters,
    Import,
    Export,
    New,
    Duplicate,
    Delete,
}

/// Account-specific actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountAction {
    PickCategory,
    PickType,
    Create,
    Edit,
    Delete,
}

/// Profile-specific actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileAction {
    PickType,
    Create,
    Edit,
    Delete,
    /// Pick block size for historical bootstrap mode
    PickBlockSize,
}

/// Holding-specific actions (assets within investment accounts)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldingAction {
    PickReturnProfile,
    Add,
    Edit,
    Delete,
}

/// Config-specific actions (tax, inflation)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigAction {
    EditTax,
    EditInflation,
    PickInflationType,
    PickFederalBrackets,
}

/// Event-specific actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventAction {
    PickTriggerType,
    PickEventReference,
    PickInterval,
    Create,
    Edit,
    Delete,
    // Trigger builder actions for recursive trigger construction
    /// Pick type for start or end condition in repeating trigger
    PickChildTriggerType,
    /// Form/picker for child trigger details
    BuildChildTrigger,
    /// Finish child trigger, return to parent
    CompleteChildTrigger,
    /// Final form for repeating event (name, description)
    FinalizeRepeating,
    /// Pick a quick event template (Social Security, RMD, Medicare)
    PickQuickEvent,
}

/// Effect-specific actions (effects within events)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectAction {
    Manage,
    PickType,
    PickTypeForAdd,
    PickAccountForEffect,
    PickActionForEffect,
    Add,
    Edit,
    Delete,
}

/// Optimization-specific actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizeAction {
    AddParameter,
    ConfigureParameter { index: usize },
    DeleteParameter { index: usize },
    SelectObjective,
    ConfigureSettings,
    RunOptimization,
}

// Convenience constructors for common actions
impl ModalAction {
    // Scenario shortcuts
    pub const SAVE_AS: Self = Self::Scenario(ScenarioAction::SaveAs);
    pub const LOAD: Self = Self::Scenario(ScenarioAction::Load);
    pub const SWITCH_TO: Self = Self::Scenario(ScenarioAction::SwitchTo);
    pub const EDIT_PARAMETERS: Self = Self::Scenario(ScenarioAction::EditParameters);
    pub const IMPORT: Self = Self::Scenario(ScenarioAction::Import);
    pub const EXPORT: Self = Self::Scenario(ScenarioAction::Export);
    pub const NEW_SCENARIO: Self = Self::Scenario(ScenarioAction::New);
    pub const DUPLICATE_SCENARIO: Self = Self::Scenario(ScenarioAction::Duplicate);
    pub const DELETE_SCENARIO: Self = Self::Scenario(ScenarioAction::Delete);

    // Account shortcuts
    pub const PICK_ACCOUNT_CATEGORY: Self = Self::Account(AccountAction::PickCategory);
    pub const PICK_ACCOUNT_TYPE: Self = Self::Account(AccountAction::PickType);
    pub const CREATE_ACCOUNT: Self = Self::Account(AccountAction::Create);
    pub const EDIT_ACCOUNT: Self = Self::Account(AccountAction::Edit);
    pub const DELETE_ACCOUNT: Self = Self::Account(AccountAction::Delete);

    // Profile shortcuts
    pub const PICK_PROFILE_TYPE: Self = Self::Profile(ProfileAction::PickType);
    pub const CREATE_PROFILE: Self = Self::Profile(ProfileAction::Create);
    pub const EDIT_PROFILE: Self = Self::Profile(ProfileAction::Edit);
    pub const DELETE_PROFILE: Self = Self::Profile(ProfileAction::Delete);
    pub const PICK_BLOCK_SIZE: Self = Self::Profile(ProfileAction::PickBlockSize);

    // Holding shortcuts
    pub const PICK_RETURN_PROFILE: Self = Self::Holding(HoldingAction::PickReturnProfile);
    pub const ADD_HOLDING: Self = Self::Holding(HoldingAction::Add);
    pub const EDIT_HOLDING: Self = Self::Holding(HoldingAction::Edit);
    pub const DELETE_HOLDING: Self = Self::Holding(HoldingAction::Delete);

    // Config shortcuts
    pub const EDIT_TAX_CONFIG: Self = Self::Config(ConfigAction::EditTax);
    pub const EDIT_INFLATION: Self = Self::Config(ConfigAction::EditInflation);
    pub const PICK_INFLATION_TYPE: Self = Self::Config(ConfigAction::PickInflationType);
    pub const PICK_FEDERAL_BRACKETS: Self = Self::Config(ConfigAction::PickFederalBrackets);

    // Event shortcuts
    pub const PICK_TRIGGER_TYPE: Self = Self::Event(EventAction::PickTriggerType);
    pub const PICK_EVENT_REFERENCE: Self = Self::Event(EventAction::PickEventReference);
    pub const PICK_INTERVAL: Self = Self::Event(EventAction::PickInterval);
    pub const CREATE_EVENT: Self = Self::Event(EventAction::Create);
    pub const EDIT_EVENT: Self = Self::Event(EventAction::Edit);
    pub const DELETE_EVENT: Self = Self::Event(EventAction::Delete);
    // Trigger builder shortcuts
    pub const PICK_CHILD_TRIGGER_TYPE: Self = Self::Event(EventAction::PickChildTriggerType);
    pub const BUILD_CHILD_TRIGGER: Self = Self::Event(EventAction::BuildChildTrigger);
    pub const COMPLETE_CHILD_TRIGGER: Self = Self::Event(EventAction::CompleteChildTrigger);
    pub const FINALIZE_REPEATING: Self = Self::Event(EventAction::FinalizeRepeating);
    pub const PICK_QUICK_EVENT: Self = Self::Event(EventAction::PickQuickEvent);

    // Effect shortcuts
    pub const MANAGE_EFFECTS: Self = Self::Effect(EffectAction::Manage);
    pub const PICK_EFFECT_TYPE: Self = Self::Effect(EffectAction::PickType);
    pub const PICK_EFFECT_TYPE_FOR_ADD: Self = Self::Effect(EffectAction::PickTypeForAdd);
    pub const PICK_ACCOUNT_FOR_EFFECT: Self = Self::Effect(EffectAction::PickAccountForEffect);
    pub const PICK_ACTION_FOR_EFFECT: Self = Self::Effect(EffectAction::PickActionForEffect);
    pub const ADD_EFFECT: Self = Self::Effect(EffectAction::Add);
    pub const EDIT_EFFECT: Self = Self::Effect(EffectAction::Edit);
    pub const DELETE_EFFECT: Self = Self::Effect(EffectAction::Delete);

    // Optimize shortcuts
    pub const ADD_OPTIMIZE_PARAMETER: Self = Self::Optimize(OptimizeAction::AddParameter);
    pub const SELECT_OBJECTIVE: Self = Self::Optimize(OptimizeAction::SelectObjective);
    pub const CONFIGURE_OPTIMIZE_SETTINGS: Self = Self::Optimize(OptimizeAction::ConfigureSettings);
    pub const RUN_OPTIMIZATION: Self = Self::Optimize(OptimizeAction::RunOptimization);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_shortcuts() {
        assert_eq!(
            ModalAction::SAVE_AS,
            ModalAction::Scenario(ScenarioAction::SaveAs)
        );
        assert_eq!(
            ModalAction::CREATE_ACCOUNT,
            ModalAction::Account(AccountAction::Create)
        );
        assert_eq!(
            ModalAction::ADD_EFFECT,
            ModalAction::Effect(EffectAction::Add)
        );
    }

    #[test]
    fn test_action_pattern_matching() {
        let action = ModalAction::Account(AccountAction::Create);

        match action {
            ModalAction::Account(AccountAction::Create) => (),
            _ => panic!("Should match Account::Create"),
        }
    }
}
