/**
 * View models for the screens.
 *
 * The API's own shapes live in `lib/api/generated`, written straight from the
 * Rust structs. These are the presentation-side types the components consume:
 * flattened, pre-formatted and ordered for display. `lib/view/*` is the only
 * place that maps one onto the other, so a server change surfaces there as a
 * type error rather than as a wrong number on screen.
 */
import type { AssetClass, DistributionSpec } from "@/lib/api/types";

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
  /** Database id of the position row, for delete. */
  positionId: number;
  assetId: AssetId;
  /** Database id of the asset behind `assetId`, for the add-position form. */
  assetServerId: number;
  purchaseDate: string; // ISO date
  units: number;
  costBasis: number;
  /** Units marked at the asset's opening price — what the balance counts. */
  value: number;
}

/**
 * The other side of a property/debt pair.
 *
 * The schema has no edge between the two — what pairs them is an event that
 * settles both, so the link is read back off the plan rather than stored.
 */
export interface LinkedAccount {
  accountId: AccountId;
  name: string;
  flavor: AccountFlavorKind;
  /** Signed, as the list shows it: a debt is negative. */
  balance: number;
  /** Annual rate as a fraction; liabilities only. */
  interestRate?: number;
  /** Names of the events that pair them. */
  through: EventId[];
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
  /**
   * The profile row the account itself owns — a bank account's, or an
   * investment account's *cash* profile. Absent for property and liability,
   * whose growth comes from an asset or an interest rate instead, so this is
   * also the test for whether the profile field can be edited at all.
   */
  returnProfileServerId?: number;
  /** Untracked cash: the whole balance of a bank account, nothing elsewhere. */
  cashValue: number;
  /** The asset a property is marked against. */
  assetServerId?: number;
  /** Annual rate as a fraction; liabilities only. */
  interestRate?: number;
  /** The property or debt on the other side of this one, if an event pairs them. */
  linked: LinkedAccount[];
  /** What the account holds, in one line — the list's widest column. */
  holdings: string;
  contributionLimit?: ContributionLimit;
  positions: AssetLot[];
  /** Names of events that read or write this account. */
  referencedBy: EventId[];
}

// ── results ───────────────────────────────────────────────────────────────
export type Percentile = "p5" | "p50" | "p95";

/**
 * Real terminal aggregates measured after deflating EACH iteration. Unmeasured
 * legacy values are NaN (displayed as unavailable), never nominal approximations.
 * Lifetime taxes are separately attributed to the selected path and its units.
 */
export interface MonteCarloStats {
  numIterations: number;
  /** Fraction in [0,1] of runs ending with positive net worth. */
  successRate: number;
  /** No settled cash shortfalls or event warnings; absent on older runs. */
  fundingSuccessRate?: number;
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

/** Pointwise real quantiles, NOT representative paths. Empty if unmeasured. */
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

/**
 * One year of the cash-flow table.
 *
 * Every figure is real — restated in the plan's first-year dollars — so a row
 * 30 years out can be read against the one above it. `lib/view/results.ts` is
 * where the deflation happens; nothing downstream of it is nominal.
 */
export interface YearlyCashFlow {
  year: number;
  /** Age at that year, or the year again when the scenario has no birth date. */
  age: number;
  income: number;
  expenses: number;
  contributions: number;
  withdrawals: number;
  appreciation: number;
  netCashFlow: number;
  taxes: number;
  /** Net worth at the end of that year, on the same path. */
  netWorth: number;
  /**
   * Cumulative inflation at this year, so the entries fetched when the row is
   * expanded can be deflated to match the totals already on it.
   */
  inflationFactor: number;
  /** What the ledger holds for the year, before anyone expands it. */
  ledger: LedgerSummary;
}

/** Ledger entry counts for one year, per filter bucket. */
export interface LedgerSummary {
  total: number;
  cash: number;
  asset: number;
  tax: number;
  event: number;
  /** The year's most notable entry kind — `Penalty`, `RMD`, `Sell` — if any. */
  tag?: string;
}

/** The buckets the ledger filter chips offer, plus the unfiltered view. */
export type LedgerCategory = "cash" | "asset" | "tax" | "event";
export type LedgerFilter = "all" | LedgerCategory;

/** One itemised effect behind a year's totals, in real dollars. */
export interface LedgerEntry {
  id: string;
  date: string;
  category: LedgerCategory;
  /** Short label: `Income`, `Contribution`, `RMD`, `Sell`, `Penalty`… */
  kind: string;
  /** Prose naming the accounts and events involved; carries no figures. */
  detail: string;
  /** Signed against the plan: money in is positive, money out negative. */
  amount?: number;
  /** The gross a tax was charged on, the gain in a sale, an RMD's requirement. */
  basis?: number;
  basisLabel?: string;
}

export interface SimulationWarning {
  id: string;
  /** `WarningKind` from the engine; treated as opaque text here. */
  kind: string;
  title: string;
  detail: string;
}

export interface ResultsData {
  runId: number;
  /** Actual server-resolved path ID, shared by every detail panel and ledger. */
  pathId: string;
  pathLabel: string;
  pathValues: number[];
  hasEnvelope: boolean;
  baseDate: string;
  dollarLabel: string;
  stats: MonteCarloStats;
  bands: NetWorthBands;
  accountSeries: AccountSeries[];
  cashFlows: YearlyCashFlow[];
  warnings: SimulationWarning[];
  /** End of the plan horizon, e.g. `age 81` or `2061` without a birth date. */
  horizonLabel: string;
  /**
   * The plan's first year — the one every figure on the screen is stated in.
   * Named so the screen can say whose dollars these are.
   */
  baseYear: number;
  /**
   * Selected path's cumulative inflation, e.g. 2.4 for prices multiplying by
   * 2.4. NaN when unmeasured, or for a synthetic nominal mean.
   */
  totalInflation: number;
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
  /** What the user wrote about it, blank where nothing was written. */
  description: string;
  /**
   * What kind of holding this profile is for, where anyone has said. Null is
   * the ordinary state: it only means a ticker will never auto-select this
   * profile, not that anything is missing.
   */
  assetClass: AssetClass | null;
  /**
   * The distribution itself, not just its summary. The row draws the profile's
   * shape, and a Student-t and a normal with the same mean and spread are
   * different pictures — which is the whole reason to pick one over the other.
   */
  distribution: DistributionSpec;
  /**
   * Annual mean return in percent, and its standard deviation. Null where the
   * distribution has no closed-form summary — a resampled history or a regime
   * blend — so the UI shows a dash rather than inventing a figure.
   */
  mean: number | null;
  sd: number | null;
  /**
   * The years behind a `Bootstrap` profile, as fractions. Attached once the
   * preset table has loaded; until then, and for every other kind, absent.
   * A resampled history is the one shape that cannot be drawn from its
   * parameters, because it has none — these are its parameters.
   */
  history?: readonly number[];
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
  distribution: DistributionSpec;
}
