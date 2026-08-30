/**
 * View models for the screens.
 *
 * The API's own shapes live in `lib/api/generated`, written straight from the
 * Rust structs. These are the presentation-side types the components consume:
 * flattened, pre-formatted and ordered for display. `lib/view/*` is the only
 * place that maps one onto the other, so a server change surfaces there as a
 * type error rather than as a wrong number on screen.
 */

// ── ids ───────────────────────────────────────────────────────────────────
/** Display identity — a name where the domain has one, else the row id. */
export type AccountId = string;
export type AssetId = string;
export type EventId = string;
export type ReturnProfileId = string;

// ── accounts ──────────────────────────────────────────────────────────────
export type TaxStatus = "Taxable" | "TaxDeferred" | "TaxFree";

/** Discriminant of `FlavorSpec`; the last two carry no tax treatment. */
export type AccountFlavorKind = "Bank" | "Investment" | "Property" | "Liability";

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
  /** Database id, for the mutation endpoints. */
  serverId: number;
  name: string;
  flavor: AccountFlavorKind;
  /** Absent for Property and Liability, which have no tax treatment. */
  taxStatus?: TaxStatus;
  /** Signed: liabilities are negative. */
  balance: number;
  /** Name of the return profile driving this account's cash or value. */
  returnProfileId: ReturnProfileId;
  contributionLimit?: ContributionLimit;
  positions: AssetLot[];
  /** Names of events that read or write this account. */
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
  /** Which statistic convergence was judged on, and where it settled. */
  convergenceMetric?: string;
  convergenceValue?: number;
  lifetimeTaxes: number;
}

/** Net-worth paths, one value per year, aligned to `years`. */
export interface NetWorthBands {
  years: number[];
  /** Age at each year, or the calendar year again when no birth date is set. */
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

export interface SimulationWarning {
  id: string;
  /** `WarningKind` from the engine; treated as opaque text here. */
  kind: string;
  title: string;
  detail: string;
}

export interface ResultsData {
  stats: MonteCarloStats;
  bands: NetWorthBands;
  accountSeries: AccountSeries[];
  cashFlows: YearlyCashFlow[];
  warnings: SimulationWarning[];
  /** End of the plan horizon, e.g. `age 81` or `2061` without a birth date. */
  horizonLabel: string;
}

// ── scenario ──────────────────────────────────────────────────────────────
export interface Scenario {
  id: string;
  serverId: number;
  name: string;
  /** The plan changed after the last successful run, so results are stale. */
  dirty: boolean;
}

// ── events ────────────────────────────────────────────────────────────────
/** Discriminant of `TriggerSpec`. */
export type TriggerKind =
  | "Date"
  | "Age"
  | "Repeating"
  | "NetWorth"
  | "AccountBalance"
  | "AssetBalance"
  | "RelativeToEvent"
  | "And"
  | "Or"
  | "Manual";

/** Discriminant of `EffectSpec`. */
export type EffectKind =
  | "Income"
  | "Expense"
  | "CashTransfer"
  | "AssetPurchase"
  | "AssetSale"
  | "Sweep"
  | "AdjustBalance"
  | "ApplyRmd"
  | "RsuVesting"
  | "DeleteAccount"
  | "PauseEvent"
  | "ResumeEvent"
  | "TriggerEvent"
  | "TerminateEvent"
  | "Random";

export interface EventEffect {
  kind: EffectKind;
  /** Human-readable summary of the effect's configuration. */
  detail: string;
}

export interface PlanEvent {
  id: EventId;
  /** Database id, for the mutation endpoints. */
  serverId: number;
  /** Short trigger summary for the list, e.g. "Repeating · monthly". */
  trigger: string;
  triggerKind: TriggerKind;
  /** Long form shown in the inspector. */
  triggerDetail: string;
  /** Next fire date, or a description for balance-driven triggers. */
  next: string;
  /**
   * Inclusive [start, end] on the timeline's axis (ages, or calendar years
   * when the scenario has no birth date); equal values mark a one-shot.
   */
  span: [number, number];
  amount: string;
  effects: EventEffect[];
  firesOnce?: boolean;
  enabled: boolean;
}

/** Scenario-level parameters, shown above the event list. */
export interface ScenarioParams {
  start: string;
  durationYears: number;
  /** Empty when the scenario has none; `Age` triggers require one. */
  birthDate: string;
  iterations: number;
  /** Name of the scenario's inflation profile, or `—`. */
  inflationProfile: string;
  /** Name of the scenario's tax configuration, or `—`. */
  taxConfig: string;
}

// ── return / inflation profiles ───────────────────────────────────────────
/** Discriminant of `DistributionSpec`. */
export type DistributionKind =
  | "None"
  | "Fixed"
  | "Normal"
  | "LogNormal"
  | "StudentT"
  | "RegimeSwitching"
  | "Bootstrap";

export interface ReturnProfile {
  id: ReturnProfileId;
  serverId: number;
  kind: DistributionKind;
  /** Provenance of the samples, e.g. "US total market · 1928–2024". */
  source: string;
  /**
   * Annual mean return in percent, and its standard deviation. Null where the
   * distribution has no closed-form summary — a resampled history or a regime
   * blend — so the UI shows a dash rather than inventing a figure.
   */
  mean: number | null;
  sd: number | null;
  /** Assets or accounts drawing on this profile. */
  usedBy: string[];
}

export interface InflationProfile {
  id: string;
  serverId: number;
  kind: DistributionKind;
  mean: number | null;
  sd: number | null;
  note: string;
}
