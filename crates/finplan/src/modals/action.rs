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
    Analysis(AnalysisAction),
    Mapping(MappingAction),
    Amount(AmountAction),
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
    /// Run Monte Carlo with convergence-based stopping
    MonteCarloConvergence,
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
    /// Create repeating event from unified form (all fields in one form)
    CreateRepeatingUnified,
    /// Pick a quick event template (Social Security, RMD, Medicare)
    PickQuickEvent,
    // Trigger editing actions
    /// Pick trigger type when editing an existing event's trigger
    EditTriggerTypePick,
    /// Update an existing event's trigger (simple triggers)
    UpdateTrigger,
    /// Update an existing event's trigger (repeating triggers)
    UpdateRepeating,
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

/// Optimization-specific actions (legacy, kept for backwards compatibility)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizeAction {
    AddParameter,
    ConfigureParameter { index: usize },
    DeleteParameter { index: usize },
    SelectObjective,
    ConfigureSettings,
    RunOptimization,
}

/// Analysis-specific actions (parameter sweep sensitivity analysis)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisAction {
    /// Add a sweep parameter (shows event picker)
    AddParameter,
    /// Create a new parameter from form submission
    CreateParameter,
    /// Configure an existing sweep parameter (min/max/steps)
    ConfigureParameter { index: usize },
    /// Delete a sweep parameter
    DeleteParameter { index: usize },
    /// Toggle a metric on/off
    ToggleMetric,
    /// Configure analysis settings (MC iterations, steps)
    ConfigureSettings,
    /// Run the analysis
    RunAnalysis,
    /// Select parameter target after picking event
    SelectParameterTarget { event_index: usize },
    /// Configure a result chart (type, parameters, metric)
    ConfigureChart { index: usize },
}

/// Mapping-specific actions (asset price editing in MAPPINGS panel)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingAction {
    EditPrice,
}

/// Amount-specific actions (editing amount fields in effects)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountAction {
    /// Pick amount type from picker
    PickType,
    /// Fixed amount form
    FixedForm,
    /// Inflation-adjusted form
    InflationForm,
    /// Scale/percentage form
    ScaleForm,
    /// Target to balance form
    TargetForm,
    /// Account balance reference form
    AccountBalanceForm,
    /// Account cash balance reference form
    CashBalanceForm,
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
    pub const MONTE_CARLO_CONVERGENCE: Self = Self::Scenario(ScenarioAction::MonteCarloConvergence);

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
    pub const CREATE_REPEATING_UNIFIED: Self = Self::Event(EventAction::CreateRepeatingUnified);
    pub const PICK_QUICK_EVENT: Self = Self::Event(EventAction::PickQuickEvent);
    // Trigger editing shortcuts
    pub const EDIT_TRIGGER_TYPE_PICK: Self = Self::Event(EventAction::EditTriggerTypePick);
    pub const UPDATE_TRIGGER: Self = Self::Event(EventAction::UpdateTrigger);
    pub const UPDATE_REPEATING: Self = Self::Event(EventAction::UpdateRepeating);

    // Effect shortcuts
    pub const MANAGE_EFFECTS: Self = Self::Effect(EffectAction::Manage);
    pub const PICK_EFFECT_TYPE: Self = Self::Effect(EffectAction::PickType);
    pub const PICK_EFFECT_TYPE_FOR_ADD: Self = Self::Effect(EffectAction::PickTypeForAdd);
    pub const PICK_ACCOUNT_FOR_EFFECT: Self = Self::Effect(EffectAction::PickAccountForEffect);
    pub const PICK_ACTION_FOR_EFFECT: Self = Self::Effect(EffectAction::PickActionForEffect);
    pub const ADD_EFFECT: Self = Self::Effect(EffectAction::Add);
    pub const EDIT_EFFECT: Self = Self::Effect(EffectAction::Edit);
    pub const DELETE_EFFECT: Self = Self::Effect(EffectAction::Delete);

    // Optimize shortcuts (legacy)
    pub const ADD_OPTIMIZE_PARAMETER: Self = Self::Optimize(OptimizeAction::AddParameter);
    pub const SELECT_OBJECTIVE: Self = Self::Optimize(OptimizeAction::SelectObjective);
    pub const CONFIGURE_OPTIMIZE_SETTINGS: Self = Self::Optimize(OptimizeAction::ConfigureSettings);
    pub const RUN_OPTIMIZATION: Self = Self::Optimize(OptimizeAction::RunOptimization);

    // Analysis shortcuts
    pub const ADD_ANALYSIS_PARAMETER: Self = Self::Analysis(AnalysisAction::AddParameter);
    pub const TOGGLE_ANALYSIS_METRIC: Self = Self::Analysis(AnalysisAction::ToggleMetric);
    pub const CONFIGURE_ANALYSIS_SETTINGS: Self = Self::Analysis(AnalysisAction::ConfigureSettings);
    pub const RUN_ANALYSIS: Self = Self::Analysis(AnalysisAction::RunAnalysis);

    // Mapping shortcuts
    pub const EDIT_ASSET_PRICE: Self = Self::Mapping(MappingAction::EditPrice);

    // Amount shortcuts
    pub const PICK_AMOUNT_TYPE: Self = Self::Amount(AmountAction::PickType);
    pub const AMOUNT_FIXED_FORM: Self = Self::Amount(AmountAction::FixedForm);
    pub const AMOUNT_INFLATION_FORM: Self = Self::Amount(AmountAction::InflationForm);
    pub const AMOUNT_SCALE_FORM: Self = Self::Amount(AmountAction::ScaleForm);
    pub const AMOUNT_TARGET_FORM: Self = Self::Amount(AmountAction::TargetForm);
    pub const AMOUNT_ACCOUNT_BALANCE_FORM: Self = Self::Amount(AmountAction::AccountBalanceForm);
    pub const AMOUNT_CASH_BALANCE_FORM: Self = Self::Amount(AmountAction::CashBalanceForm);
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
