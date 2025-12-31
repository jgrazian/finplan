// ============================================================================
// ID Types
// ============================================================================

export type AccountId = number;
export type AssetId = number;
export type CashFlowId = number;
export type EventId = number;
export type SpendingTargetId = number;

// ============================================================================
// Asset & Account Types
// ============================================================================

export type AssetClass = "Investable" | "RealEstate" | "Depreciating" | "Liability";

export interface Asset {
    asset_id: AssetId;
    name?: string;
    asset_class: AssetClass;
    initial_value: number;
    return_profile_index: number;
}

export type AccountType = "Taxable" | "TaxDeferred" | "TaxFree" | "Illiquid";

export interface Account {
    account_id: AccountId;
    account_type: AccountType;
    assets: Asset[];
}

// ============================================================================
// Cash Flow Types
// ============================================================================

export type RepeatInterval = "Never" | "Weekly" | "BiWeekly" | "Monthly" | "Quarterly" | "Yearly";

export type CashFlowEndpoint =
    | "External"
    | { Asset: { account_id: AccountId; asset_id: AssetId } };

export type CashFlowState = "Pending" | "Active" | "Paused" | "Terminated";

export type LimitPeriod = "Yearly" | "Lifetime";

export interface CashFlowLimits {
    limit: number;
    limit_period: LimitPeriod;
}

export interface CashFlow {
    cash_flow_id: CashFlowId;
    amount: number;
    repeats: RepeatInterval;
    cash_flow_limits?: CashFlowLimits;
    adjust_for_inflation: boolean;
    source: CashFlowEndpoint;
    target: CashFlowEndpoint;
    state: CashFlowState;
}

// ============================================================================
// Event Types
// ============================================================================

export type TriggerOffset =
    | { Days: number }
    | { Months: number }
    | { Years: number };

export type EventTrigger =
    | { Date: string }
    | { Age: { years: number; months?: number } }
    | { RelativeToEvent: { event_id: EventId; offset: TriggerOffset } }
    | { AccountBalance: { account_id: AccountId; threshold: number; above: boolean } }
    | { AssetBalance: { account_id: AccountId; asset_id: AssetId; threshold: number; above: boolean } }
    | { NetWorth: { threshold: number; above: boolean } }
    | { AccountDepleted: AccountId }
    | { CashFlowEnded: CashFlowId }
    | { TotalIncomeBelow: number }
    | { And: EventTrigger[] }
    | { Or: EventTrigger[] }
    | "Manual";

export type EventEffect =
    | { CreateAccount: Account }
    | { DeleteAccount: AccountId }
    | { CreateCashFlow: CashFlow }
    | { ActivateCashFlow: CashFlowId }
    | { PauseCashFlow: CashFlowId }
    | { ResumeCashFlow: CashFlowId }
    | { TerminateCashFlow: CashFlowId }
    | { ModifyCashFlow: { cash_flow_id: CashFlowId; new_amount?: number; new_repeats?: RepeatInterval } }
    | { CreateSpendingTarget: SpendingTarget }
    | { ActivateSpendingTarget: SpendingTargetId }
    | { PauseSpendingTarget: SpendingTargetId }
    | { ResumeSpendingTarget: SpendingTargetId }
    | { TerminateSpendingTarget: SpendingTargetId }
    | { ModifySpendingTarget: { spending_target_id: SpendingTargetId; new_amount?: number } }
    | { TransferAsset: { from_account: AccountId; to_account: AccountId; from_asset_id: AssetId; to_asset_id: AssetId; amount?: number } }
    | { TriggerEvent: EventId };

export interface Event {
    event_id: EventId;
    trigger: EventTrigger;
    effects: EventEffect[];
    once: boolean;
}

// ============================================================================
// Tax Types
// ============================================================================

export interface TaxBracket {
    threshold: number;
    rate: number;
}

export interface TaxConfig {
    federal_brackets: TaxBracket[];
    state_rate: number;
    capital_gains_rate: number;
    taxable_gains_percentage: number;
}

// ============================================================================
// Spending Target Types
// ============================================================================

export type SpendingTargetState = "Pending" | "Active" | "Paused" | "Terminated";

export type WithdrawalStrategy =
    | { Sequential: { order: AccountId[] } }
    | "ProRata"
    | "TaxOptimized";

export interface SpendingTarget {
    spending_target_id: SpendingTargetId;
    amount: number;
    net_amount_mode: boolean;
    repeats: RepeatInterval;
    adjust_for_inflation: boolean;
    withdrawal_strategy: WithdrawalStrategy;
    exclude_accounts: AccountId[];
    state: SpendingTargetState;
}

// ============================================================================
// Profile Types
// ============================================================================

export type InflationProfile =
    | "None"
    | { Fixed: number }
    | { Normal: { mean: number; std_dev: number } }
    | { LogNormal: { mean: number; std_dev: number } };

export type ReturnProfile =
    | "None"
    | { Fixed: number }
    | { Normal: { mean: number; std_dev: number } }
    | { LogNormal: { mean: number; std_dev: number } };

// ============================================================================
// Named Return Profile
// ============================================================================

export interface NamedReturnProfile {
    name: string;
    profile: ReturnProfile;
}

// ============================================================================
// Simulation Parameters
// ============================================================================

export interface SimulationParameters {
    start_date?: string;
    duration_years: number;
    birth_date?: string;
    inflation_profile: InflationProfile;
    return_profiles: ReturnProfile[];
    named_return_profiles?: NamedReturnProfile[];
    events: Event[];
    accounts: Account[];
    cash_flows: CashFlow[];
    spending_targets: SpendingTarget[];
    tax_config: TaxConfig;
}

// ============================================================================
// Simulation Results
// ============================================================================

export interface TimePointStats {
    date: string;
    p10: number;
    p50: number;
    p90: number;
}

export interface AggregatedResult {
    accounts: Record<string, TimePointStats[]>;
    total_portfolio: TimePointStats[];
}

// ============================================================================
// Saved Simulation Types
// ============================================================================

export interface SavedSimulation {
    id: string;
    name: string;
    description?: string;
    parameters: SimulationParameters;
    created_at: string;
    updated_at: string;
}

export interface SimulationListItem {
    id: string;
    name: string;
    description?: string;
    created_at: string;
    updated_at: string;
}

export interface SimulationRunRecord {
    id: string;
    simulation_id: string;
    iterations: number;
    ran_at: string;
}

// ============================================================================
// Default Values
// ============================================================================

export const DEFAULT_TAX_CONFIG: TaxConfig = {
    federal_brackets: [
        { threshold: 0, rate: 0.10 },
        { threshold: 11600, rate: 0.12 },
        { threshold: 47150, rate: 0.22 },
        { threshold: 100525, rate: 0.24 },
        { threshold: 191950, rate: 0.32 },
        { threshold: 243725, rate: 0.35 },
        { threshold: 609350, rate: 0.37 },
    ],
    state_rate: 0.05,
    capital_gains_rate: 0.15,
    taxable_gains_percentage: 0.50,
};

export const DEFAULT_NAMED_RETURN_PROFILES: NamedReturnProfile[] = [
    { name: "US Stocks", profile: { Normal: { mean: 0.096, std_dev: 0.165 } } },
    { name: "Bonds", profile: { Normal: { mean: 0.045, std_dev: 0.055 } } },
    { name: "Cash", profile: { Fixed: 0.03 } },
];

export const DEFAULT_SIMULATION_PARAMETERS: SimulationParameters = {
    duration_years: 30,
    inflation_profile: { Normal: { mean: 0.035, std_dev: 0.028 } },
    return_profiles: [{ Normal: { mean: 0.096, std_dev: 0.165 } }],
    named_return_profiles: DEFAULT_NAMED_RETURN_PROFILES,
    events: [],
    accounts: [],
    cash_flows: [],
    spending_targets: [],
    tax_config: DEFAULT_TAX_CONFIG,
};
