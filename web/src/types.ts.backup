export interface SimulationParameters {
    start_date?: string; // YYYY-MM-DD
    duration_years: number;
    inflation_profile: InflationProfile;
    events: Event[];
    accounts: Account[];
}

export type InflationProfile =
    | "None"
    | { Fixed: number }
    | { Normal: { mean: number, std_dev: number } }
    | { LogNormal: { mean: number, std_dev: number } };

export interface Event {
    name: string;
    trigger: EventTrigger;
}

export type EventTrigger =
    | { Date: string }
    | { AccountBalance: { account_id: number, threshold: number, above: boolean } };

export interface Account {
    account_id: number;
    name: string;
    initial_balance: number;
    account_type: AccountType;
    return_profile: ReturnProfile;
    cash_flows: CashFlow[];
}

export type AccountType = "Taxable" | "TaxDeferred" | "TaxFree" | "Liability";

export type ReturnProfile =
    | "None"
    | { Fixed: number }
    | { Normal: { mean: number, std_dev: number } }
    | { LogNormal: { mean: number, std_dev: number } };

export interface CashFlow {
    cash_flow_id: number;
    description?: string;
    amount: number;
    start: Timepoint;
    end: Timepoint;
    repeats: RepeatInterval;
    cash_flow_limits?: CashFlowLimits;
    adjust_for_inflation: boolean;
}

export type Timepoint = "Immediate" | { Date: string } | { Event: string } | "Never";

export type RepeatInterval = "Never" | "Weekly" | "BiWeekly" | "Monthly" | "Quarterly" | "Yearly";

export interface CashFlowLimits {
    limit: number;
    limit_period: "Yearly" | "Lifetime";
}

export interface AggregatedResult {
    accounts: Record<number, TimePointStats[]>;
    total_portfolio: TimePointStats[];
}

export interface TimePointStats {
    date: string;
    p10: number;
    p50: number;
    p90: number;
}
