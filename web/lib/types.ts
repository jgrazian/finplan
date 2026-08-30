/**
 * TypeScript mirror of the `finplan_core` data model.
 *
 * These shapes deliberately track the Rust types so the mock layer can be
 * swapped for real engine output without touching components:
 *   Account/AccountFlavor/TaxStatus  → crates/finplan_core/src/model/accounts.rs
 *   MonteCarloStats/SimulationResult → crates/finplan_core/src/model/results.rs
 */

// ── ids ───────────────────────────────────────────────────────────────────
export type AccountId = string;
export type AssetId = string;
export type EventId = string;
export type ReturnProfileId = string;

// ── accounts ──────────────────────────────────────────────────────────────
export type TaxStatus = "Taxable" | "TaxDeferred" | "TaxFree";

/** Discriminant of `AccountFlavor`; `n/a` tax status applies to the last two. */
export type AccountFlavorKind =
  | "Bank"
  | "Investment"
  | "Property"
  | "Liability";

export type ContributionLimitPeriod = "Monthly" | "Yearly";

export interface ContributionLimit {
  amount: number;
  period: ContributionLimitPeriod;
}

/** A single purchase lot for cost-basis tracking. */
export interface AssetLot {
  assetId: AssetId;
  purchaseDate: string; // ISO date
  units: number;
  costBasis: number;
}

export interface Account {
  accountId: AccountId;
  name: string;
  flavor: AccountFlavorKind;
  /** Absent for Property and Liability, which have no tax treatment. */
  taxStatus?: TaxStatus;
  /** Signed: liabilities are negative. */
  balance: number;
  returnProfileId: ReturnProfileId;
  contributionLimit?: ContributionLimit;
  positions: AssetLot[];
  /** Ids of events that read or write this account. */
  referencedBy: EventId[];
}

// ── results ───────────────────────────────────────────────────────────────
export type Percentile = "p5" | "p50" | "p95";

export interface MonteCarloStats {
  numIterations: number;
  /** Fraction in [0,1] of runs ending with positive net worth. */
  successRate: number;
  meanFinalNetWorth: number;
  stdDevFinalNetWorth: number;
  minFinalNetWorth: number;
  maxFinalNetWorth: number;
  percentileValues: Array<[number, number]>;
  converged?: boolean;
  /** Iteration count at which the convergence metric was met. */
  convergedAt?: number;
  lifetimeTaxes: number;
}

/** Net-worth paths, one value per year, aligned to `years`. */
export interface NetWorthBands {
  years: number[];
  ages: number[];
  p5: number[];
  p50: number[];
  p95: number[];
}

/** Per-account contribution to net worth, for the stacked view. */
export interface AccountSeries {
  accountId: AccountId;
  label: string;
  color: string;
  values: number[];
}

export interface YearlyCashFlow {
  year: number;
  income: number;
  expenses: number;
  taxes: number;
}

export type WarningKind =
  | "EffectSkipped"
  | "EvaluationFailed"
  | "IterationLimitHit";

export interface SimulationWarning {
  id: string;
  kind: WarningKind;
  title: string;
  detail: string;
}

export interface ResultsData {
  stats: MonteCarloStats;
  bands: NetWorthBands;
  accountSeries: AccountSeries[];
  cashFlows: YearlyCashFlow[];
  warnings: SimulationWarning[];
  /** Plan horizon, used in the success-rate caption. */
  finalAge: number;
}

// ── scenario ──────────────────────────────────────────────────────────────
export interface Scenario {
  id: string;
  name: string;
  dirty: boolean;
}

// ── events ────────────────────────────────────────────────────────────────
/** Discriminant of `EventTrigger` (crates/finplan_core/src/model/events.rs). */
export type TriggerKind =
  | "Date"
  | "Age"
  | "Repeating"
  | "NetWorth"
  | "AccountBalance"
  | "RelativeToEvent"
  | "And"
  | "Or";

/** Discriminant of `EventEffect`. */
export type EffectKind =
  | "Income"
  | "Expense"
  | "CashTransfer"
  | "AssetPurchase"
  | "Sweep"
  | "ApplyRmd"
  | "PauseEvent"
  | "TriggerEvent"
  | "TerminateEvent";

export interface EventEffect {
  kind: EffectKind;
  /** Human-readable summary of the effect's configuration. */
  detail: string;
}

export interface PlanEvent {
  id: EventId;
  /** Short trigger summary for the list, e.g. "Repeating · monthly". */
  trigger: string;
  triggerKind: TriggerKind;
  /** Long form shown in the inspector. */
  triggerDetail: string;
  /** Next fire date, or an estimate for balance-driven triggers. */
  next: string;
  /** Inclusive [startAge, endAge]; equal ages mark a one-shot. */
  span: [number, number];
  amount: string;
  effects: EventEffect[];
  firesOnce?: boolean;
}

/** Scenario-level parameters that apply to every event. */
export interface ScenarioParams {
  start: string;
  durationYears: number;
  birthDate: string;
  iterations: number;
  inflationProfileId: string;
  withdrawalOrder: WithdrawalOrder;
}

export type WithdrawalOrder = "TaxEfficientEarly" | "ProRata" | "PenaltyAware";

// ── return / inflation profiles ───────────────────────────────────────────
/** Shape of `ReturnProfile` (crates/finplan_core/src/model/market.rs). */
export type DistributionKind =
  | "Fixed"
  | "Normal"
  | "LogNormal"
  | "Bootstrap"
  | "Historical";

export interface ReturnProfile {
  id: ReturnProfileId;
  kind: DistributionKind;
  /** Provenance of the samples, e.g. "US total market · 1928–2024". */
  source: string;
  /** Annual mean return, in percent. */
  mean: number;
  /** Annual standard deviation, in percent; 0 for Fixed. */
  sd: number;
  /** Assets or accounts drawing on this profile. */
  usedBy: string[];
}

export interface InflationProfile {
  id: string;
  kind: DistributionKind;
  mean: number;
  sd: number;
  note: string;
}
