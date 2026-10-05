import type {
  ComparisonRow,
  ConversionChoice,
  DrawdownBody,
  DrawdownChoice,
  DrawdownComparison,
  DrawdownYear,
  FundingPolicySpec,
  RetirementSource,
  StrategyChoice,
  WithdrawalStrategy,
} from "@/lib/api/types";
import { fmtCompact, fmtCurrency } from "../format.ts";

/**
 * The Drawdown screen's view model (spec 20, part 3).
 *
 * Everything here is pure: a `DrawdownBody`, the choice picked, and the two
 * display toggles go in; series, stacks, ticks, panel rows and CSV come out.
 * The chart and the side panel only draw what this returns.
 *
 * How a bar is built. The engine reports gross sale proceeds per account, tax
 * withheld on them, and any money taken beyond need (RMDs). Above zero a bar
 * stacks, bottom to top:
 *
 *   1. the sources that paid for spending: income, accounts (net of the tax
 *      and surplus carved off them, in proportion), bank cash
 *   2. a hatched cap for any shortfall, so parts 1 and 2 reach the target line
 *   3. surplus: withdrawals beyond need, outlined
 *
 * so only a real surplus rises past the line. The tax withheld on the sales
 * hangs below zero: it is what the strategy costs, not money that was spent,
 * and stacked above the line it read as overspending. Share mode leaves it
 * out, since it is not a share of spending.
 *
 * Roth conversions (spec 21) are not spending either: a converting year gets
 * a hollow marker at the amount converted, and the conversion's tax hangs
 * below the tax on withdrawals, drawn apart from it. Whatever paid that tax
 * (a taxable sale, the part a conversion withheld, else bank cash) is carved
 * off the sources, so they still meet the line.
 */

export type DrawdownUnit = "usd" | "share";
export type DrawdownBasis = "nominal" | "real";

/** What a series is, which is what it is coloured by. */
export type SeriesRole = "income" | "tax-deferred" | "taxable" | "tax-free" | "cash" | "taxes";

export interface DrawdownSeries {
  key: string;
  kind: "income" | "account" | "cash" | "taxes";
  name: string;
  role: SeriesRole;
  color: string;
  /** Index into the year's `income` (kind income) or `withdrawals`/`balances` (account). */
  index: number;
}

/** Surplus is not a source: it is money taken beyond need. */
export const SURPLUS_COLOR = "color-mix(in srgb, var(--color-text) 14%, transparent)";
export const SHORTFALL_COLOR = "var(--color-danger)";
/**
 * Tax on withdrawals, as a CSS background for swatches: red stripes at 45°,
 * the opposite slant to the shortfall's hatch, so the two reds stay apart.
 * The chart draws the same stripes as an SVG pattern.
 */
export const TAX_STRIPES =
  "repeating-linear-gradient(45deg, color-mix(in srgb, var(--color-danger) 60%, transparent) 0 2px, color-mix(in srgb, var(--color-danger) 12%, transparent) 2px 5px)";
/**
 * Tax on Roth conversions: the same red in level bands, so it reads as tax
 * but apart from the tax on withdrawals beside it. The chart draws the same
 * bands as an SVG pattern.
 */
export const CONVERSION_TAX_STRIPES =
  "repeating-linear-gradient(0deg, color-mix(in srgb, var(--color-danger) 55%, transparent) 0 1.5px, color-mix(in srgb, var(--color-danger) 8%, transparent) 1.5px 4px)";
/** The conversion marker: the Roth's colour, hollow. */
export const CONVERSION_COLOR = "var(--color-series-4)";

const ROLE_COLOR: Record<SeriesRole, string> = {
  income: "var(--color-series-5)",
  "tax-deferred": "var(--color-series-2)",
  taxable: "var(--color-series-1)",
  "tax-free": "var(--color-series-4)",
  cash: "var(--color-series-3)",
  taxes: TAX_STRIPES,
};

/** The `n`th series of a role: the role's colour, then paler steps of it. */
function shade(role: SeriesRole, n: number): string {
  const base = ROLE_COLOR[role];
  if (n === 0) return base;
  const keep = Math.max(35, 100 - n * 22);
  return `color-mix(in srgb, ${base} ${keep}%, var(--color-raised))`;
}

function roleOfAccount(taxStatus: string | null | undefined, flavor: string): SeriesRole {
  if (flavor === "Bank") return "cash";
  if (taxStatus === "TaxDeferred") return "tax-deferred";
  if (taxStatus === "TaxFree") return "tax-free";
  return "taxable";
}

// ── strategy choices ────────────────────────────────────────────────

export const STRATEGY_LABEL: Record<WithdrawalStrategy, string> = {
  TaxEfficientEarly: "Tax-efficient early",
  TaxDeferredFirst: "Tax-deferred first",
  TaxFreeFirst: "Tax-free first",
  ProRata: "Pro rata",
  PenaltyAware: "Penalty aware",
  BracketFilling: "Bracket filling",
};

const STRATEGY_SLUG: Record<WithdrawalStrategy, string> = {
  TaxEfficientEarly: "tax-efficient",
  TaxDeferredFirst: "tax-deferred",
  TaxFreeFirst: "tax-free",
  ProRata: "pro-rata",
  PenaltyAware: "penalty-aware",
  BracketFilling: "bracket",
};

export const STRATEGY_ORDER: WithdrawalStrategy[] = [
  "TaxEfficientEarly",
  "TaxDeferredFirst",
  "TaxFreeFirst",
  "ProRata",
  "PenaltyAware",
  "BracketFilling",
];

export const BRACKET_CEILINGS = [0.1, 0.12, 0.22] as const;
export const DEFAULT_CEILING = 0.12;

/** `AsPlanned` → `planned`, bracket filling to 22% → `bracket-22`. */
export function choiceKey(choice: StrategyChoice): string {
  if (choice.kind === "AsPlanned") return "planned";
  const slug = STRATEGY_SLUG[choice.strategy];
  return choice.strategy === "BracketFilling"
    ? `${slug}-${Math.round((choice.bracket_ceiling ?? DEFAULT_CEILING) * 100)}`
    : slug;
}

/** The choice a URL key names; undefined for a key nothing answers to. */
export function choiceOfKey(key: string | undefined): StrategyChoice | undefined {
  if (key == null) return undefined;
  if (key === "planned") return { kind: "AsPlanned" };
  const bracket = /^bracket-(\d{1,2})$/.exec(key);
  if (bracket) {
    return {
      kind: "Strategy",
      strategy: "BracketFilling",
      bracket_ceiling: Number(bracket[1]) / 100,
    };
  }
  const found = STRATEGY_ORDER.find((s) => STRATEGY_SLUG[s] === key);
  return found ? { kind: "Strategy", strategy: found } : undefined;
}

export function choiceLabel(choice: StrategyChoice): string {
  return choice.kind === "AsPlanned" ? "As planned" : STRATEGY_LABEL[choice.strategy];
}

export function ceilingOf(choice: StrategyChoice): number | undefined {
  return choice.kind === "Strategy" && choice.strategy === "BracketFilling"
    ? (choice.bracket_ceiling ?? DEFAULT_CEILING)
    : undefined;
}

export function strategyDescription(choice: StrategyChoice): string {
  if (choice.kind === "AsPlanned") {
    return "Your plan's own rules: its funding policy, and the sweeps its events make.";
  }
  switch (choice.strategy) {
    case "TaxEfficientEarly":
      return "Taxable first, then tax-deferred, then tax-free. Tax-deferred money compounds longest.";
    case "TaxDeferredFirst":
      return "401(k)/IRA first, then taxable, then Roth. Shrinks later RMDs.";
    case "TaxFreeFirst":
      return "Roth first, then taxable, then tax-deferred.";
    case "ProRata":
      return "Each account in proportion to its balance.";
    case "PenaltyAware":
      return "Before 59½ avoids tax-deferred accounts; after, tax-efficient.";
    case "BracketFilling":
      return `Tax-deferred up to the top of the ${Math.round((ceilingOf(choice) ?? DEFAULT_CEILING) * 100)}% bracket, then taxable, then Roth.`;
  }
}

/** True when the plan's own funding policy is this choice (its chip says "current"). */
export function isCurrentChoice(
  choice: StrategyChoice,
  planFunding: FundingPolicySpec | null | undefined,
): boolean {
  if (choice.kind === "AsPlanned") return planFunding == null;
  if (planFunding == null || planFunding.strategy !== choice.strategy) return false;
  return choice.strategy !== "BracketFilling"
    || Math.abs((planFunding.bracket_ceiling ?? DEFAULT_CEILING) - (ceilingOf(choice) ?? 0)) < 1e-9;
}

/** The funding policy Apply to plan would write for a choice, keeping the plan's exclusions. */
export function policyFor(
  choice: StrategyChoice,
  current: FundingPolicySpec | null | undefined,
): FundingPolicySpec | undefined {
  if (choice.kind === "AsPlanned") return undefined;
  return {
    strategy: choice.strategy,
    bracket_ceiling: choice.strategy === "BracketFilling" ? ceilingOf(choice) : undefined,
    exclude_accounts: current?.exclude_accounts ?? [],
  };
}

/** The request list with the bracket choice's ceiling replaced. */
export function withCeiling(choices: StrategyChoice[], ceiling: number): StrategyChoice[] {
  return choices.map((c) =>
    c.kind === "Strategy" && c.strategy === "BracketFilling"
      ? { ...c, bracket_ceiling: ceiling }
      : c,
  );
}

// ── the conversion toggle ───────────────────────────────────────────

/** The toggle's setting; undefined runs the plan's own conversions ("As planned"). */
export type ConversionSetting = ConversionChoice | undefined;

export const CONVERSION_CEILINGS = [0.12, 0.22, 0.24] as const;

/** `planned`, `none`, or the ceiling in whole percents (`22`). */
export function conversionKey(setting: ConversionSetting): string {
  if (setting == null) return "planned";
  return setting.kind === "Off" ? "none" : String(Math.round(setting.ceiling_rate * 100));
}

export function conversionOfKey(key: string): ConversionSetting {
  if (key === "none") return { kind: "Off" };
  const percent = /^(\d{1,2})$/.exec(key);
  return percent ? { kind: "UpTo", ceiling_rate: Number(percent[1]) / 100 } : undefined;
}

export function conversionLabel(setting: ConversionSetting): string {
  if (setting == null) return "As planned";
  return setting.kind === "Off" ? "None" : `Up to ${Math.round(setting.ceiling_rate * 100)}%`;
}

export interface ConversionOption {
  value: string;
  label: string;
  disabled?: boolean;
  title?: string;
}

/**
 * The toggle's options: As planned · None · up to 12% / 22% / 24%. Without
 * an account to convert from or into, only As planned is on, and every
 * other option says why.
 */
export function conversionOptions(body: DrawdownBody): ConversionOption[] {
  const why = body.conversions.unavailable ?? undefined;
  const planned = body.conversions.events.length > 0
    ? "The plan's own Roth conversions, as they are."
    : "The plan has no Roth conversions.";
  return [
    { value: "planned", label: "As planned", title: planned },
    { value: "none", label: "None", disabled: why != null, title: why ?? "Switch the plan's conversions off." },
    ...CONVERSION_CEILINGS.map((rate) => ({
      value: conversionKey({ kind: "UpTo", ceiling_rate: rate }),
      label: `${Math.round(rate * 100)}%`,
      disabled: why != null,
      title: why ?? `Each year, convert pre-tax money to the Roth up to the top of the ${Math.round(rate * 100)}% bracket.`,
    })),
  ];
}

/** Every choice with the toggle's setting. */
export function withConversion(choices: StrategyChoice[], setting: ConversionSetting): StrategyChoice[] {
  return setting == null ? choices : choices.map((c) => ({ ...c, conversion: setting }));
}

export function retirementHint(source: RetirementSource): string | undefined {
  switch (source) {
    case "income":
      return "from income";
    case "start":
      return "from plan start";
    case "parameter":
      return "from a plan parameter";
    default:
      return undefined;
  }
}

// ── the view ────────────────────────────────────────────────────────

/** One segment of a bar, `from`..`to` in the unit on screen. */
export interface Segment {
  key: string;
  from: number;
  to: number;
}

export interface Column {
  year: number;
  age?: number;
  /** Sources, bottom-up. */
  stack: Segment[];
  /** Shortfall, hatched, up to the target line. */
  cap?: Segment;
  /** Tax withheld on the year's sales, from zero down; dollars only. */
  taxes?: Segment;
  /** Tax on the year's Roth conversions, below the tax on withdrawals; dollars only. */
  conversionTax?: Segment;
  /** Roth conversions, gross, where the hollow marker sits; dollars only. */
  conversion?: number;
  surplus?: Segment;
  /** Target spend in the unit on screen; undefined in Share, where bars are 100%. */
  target?: number;
  /**
   * In an RMD year, the required distributions after their tax, measured from
   * zero like everything above the axis. Read against the target: above it,
   * the RMD alone forces out more than the year spends. Dollars only.
   */
  rmd?: number;
  /** The height of everything drawn. */
  top: number;
}

export interface PanelRow {
  key: string;
  name: string;
  sub: string;
  color: string;
  /** Share of the target spend met by this source, 0..1. */
  share: number;
  amount: number;
  dim: boolean;
}

export interface YearPanel {
  year: number;
  age?: number;
  target: number;
  rows: PanelRow[];
  funded: number;
  fundedAmount: number;
  /** Tax withheld on the year's sales, outside the funded total. */
  taxes?: { amount: number; ofSpending: number; color: string };
  /** The year's required minimum distributions, gross and after tax, and the accounts they came from. */
  rmd?: { amount: number; afterTax: number; accounts: string[] };
  /** The year's Roth conversions, gross; their tax; the Roth accounts' year-end balance. */
  conversion?: { amount: number; tax: number; roth: number; accounts: string[] };
  note?: { tone: "bad" | "info"; text: string };
}

export interface LifetimeShare {
  key: string;
  name: string;
  color: string;
  share: number;
}

export interface MarkerPosition {
  index: number;
  year: number;
  label: string;
}

export interface DrawdownView {
  series: DrawdownSeries[];
  columns: Column[];
  years: number[];
  /** Top of the y domain. */
  max: number;
  /** Bottom of the y domain: 0, or below it by the deepest year's tax. */
  min: number;
  hasShortfall: boolean;
  hasSurplus: boolean;
  /** Any year carries a required minimum distribution. */
  hasRmd: boolean;
  /** Any year converts to a Roth. */
  hasConversion: boolean;
  /** Converted over the years shown, and the tax on it. */
  lifetimeConversion: number;
  lifetimeConversionTax: number;
  markers: MarkerPosition[];
  /** Sources only; the tax is reported beside them, not as a share. */
  lifetime: LifetimeShare[];
  lifetimeTax: number;
  /** Lifetime tax on withdrawals over lifetime spending. */
  lifetimeTaxShare: number;
  endBalance: number;
  /** The same with tax-deferred balances net of the plan's deferred tax rate. */
  endBalanceAfterTax: number;
  endAge?: number;
  endYear: number;
  unit: DrawdownUnit;
  basis: DrawdownBasis;
}

export function ageIn(body: DrawdownBody, year: number): number | undefined {
  if (body.birth_year != null) return year - body.birth_year;
  const { age, year: start } = body.retirement;
  return age != null ? age + (year - start) : undefined;
}

interface YearParts {
  income: number[];
  accounts: number[];
  cash: number;
  taxes: number;
  surplus: number;
  gap: number;
  spending: number;
  /** Required distributions, gross, as the RMD rules state them. */
  rmd: number;
  /** The same after the tax withheld on them, at the year's average rate. */
  rmdNet: number;
  /** Roth conversions, gross, and their tax. */
  conversion: number;
  conversionTax: number;
}

/**
 * A year's parts after the tax and surplus are carved off the sales; `f`
 * converts the basis. The conversion tax comes off the sales first (a taxable
 * sale paid it, or the conversion withheld it from the pre-tax account), then
 * off bank cash.
 */
function partsOf(year: DrawdownYear, f: number): YearParts {
  const sold = year.withdrawals.reduce((a, b) => a + b, 0);
  const net = Math.max(0, sold - year.withdrawal_taxes - year.surplus);
  const fromSales = Math.min(net, year.conversion_tax);
  const fromCash = Math.min(year.cash, year.conversion_tax - fromSales);
  const keep = sold > 0 ? (net - fromSales) / sold : 0;
  const afterTax = sold > 0 ? Math.max(0, sold - year.withdrawal_taxes) / sold : 0;
  const rmd = year.rmd.reduce((a, b) => a + b, 0) * f;
  const income = year.income.map((v) => v * f);
  const accounts = year.withdrawals.map((v) => v * keep * f);
  const cash = (year.cash - fromCash) * f;
  const spending = year.spending * f;
  const paid = income.reduce((a, b) => a + b, 0) + accounts.reduce((a, b) => a + b, 0) + cash;
  return {
    income,
    accounts,
    cash,
    taxes: year.withdrawal_taxes * f,
    surplus: year.surplus * f,
    gap: Math.max(0, spending - paid),
    spending,
    rmd,
    rmdNet: rmd * afterTax,
    conversion: year.conversion * f,
    conversionTax: year.conversion_tax * f,
  };
}

const factor = (year: DrawdownYear, basis: DrawdownBasis) =>
  basis === "real" && year.inflation > 0 ? 1 / year.inflation : 1;

export function seriesOf(body: DrawdownBody, choice: DrawdownChoice): DrawdownSeries[] {
  const out: DrawdownSeries[] = [];
  const count = new Map<SeriesRole, number>();
  const next = (role: SeriesRole) => {
    const n = count.get(role) ?? 0;
    count.set(role, n + 1);
    return shade(role, n);
  };
  body.income_sources.forEach((source, index) => {
    if (!choice.years.some((y) => (y.income[index] ?? 0) > 0.5)) return;
    out.push({
      key: `income:${source.event_id}`,
      kind: "income",
      name: source.name,
      role: "income",
      color: next("income"),
      index,
    });
  });
  body.accounts.forEach((account, index) => {
    if (!choice.years.some((y) => (y.withdrawals[index] ?? 0) > 0.5)) return;
    const role = roleOfAccount(account.tax_status, account.flavor);
    out.push({
      key: `account:${account.id}`,
      kind: "account",
      name: account.name,
      role,
      color: next(role),
      index,
    });
  });
  if (choice.years.some((y) => y.cash > 0.5)) {
    out.push({ key: "cash", kind: "cash", name: "Cash", role: "cash", color: ROLE_COLOR.cash, index: 0 });
  }
  if (choice.years.some((y) => y.withdrawal_taxes > 0.5)) {
    out.push({
      key: "taxes",
      kind: "taxes",
      name: "Tax on withdrawals",
      role: "taxes",
      color: ROLE_COLOR.taxes,
      index: 0,
    });
  }
  return out;
}

function amountOf(series: DrawdownSeries, parts: YearParts): number {
  switch (series.kind) {
    case "income":
      return parts.income[series.index] ?? 0;
    case "account":
      return parts.accounts[series.index] ?? 0;
    case "cash":
      return parts.cash;
    case "taxes":
      return parts.taxes;
  }
}

function markerLabel(body: DrawdownBody, kind: "income" | "rmd", id: number): string {
  if (kind === "income") {
    return body.income_sources.find((s) => s.event_id === id)?.name ?? "Income starts";
  }
  const account = body.accounts.find((a) => a.id === id);
  return account ? `${account.name} RMD` : "First RMD";
}

export function drawdownView(
  body: DrawdownBody,
  choice: DrawdownChoice,
  unit: DrawdownUnit,
  basis: DrawdownBasis,
): DrawdownView {
  const series = seriesOf(body, choice);
  const sources = series.filter((s) => s.kind !== "taxes");
  const years = choice.years.map((y) => y.year);
  let max = 0;
  let min = 0;

  const columns: Column[] = choice.years.map((y) => {
    const parts = partsOf(y, factor(y, basis));
    const total = parts.spending + parts.surplus;
    const scale = unit === "share" ? (total > 0 ? 1 / total : 0) : 1;
    let at = 0;
    const stack: Segment[] = [];
    for (const s of sources) {
      const value = amountOf(s, parts) * scale;
      if (value > 0) stack.push({ key: s.key, from: at, to: at + value });
      at += value;
    }
    const column: Column = { year: y.year, age: ageIn(body, y.year), stack, top: 0 };
    if (parts.gap > 0.5 * factor(y, basis)) {
      column.cap = { key: "shortfall", from: at, to: at + parts.gap * scale };
      at += parts.gap * scale;
    }
    if (parts.surplus > 0.5 * factor(y, basis)) {
      column.surplus = { key: "surplus", from: at, to: at + parts.surplus * scale };
      at += parts.surplus * scale;
    }
    column.top = at;
    if (unit === "usd") {
      column.target = parts.spending;
      if (parts.taxes > 0) column.taxes = { key: "taxes", from: 0, to: -parts.taxes };
      const below = parts.taxes + parts.conversionTax;
      if (parts.conversionTax > 0.5 * factor(y, basis)) {
        column.conversionTax = { key: "conversion-tax", from: -parts.taxes, to: -below };
      }
      min = Math.min(min, -below);
      if (parts.rmd > 0.5 * factor(y, basis)) {
        column.rmd = parts.rmdNet;
      }
      if (parts.conversion > 0.5 * factor(y, basis)) {
        column.conversion = parts.conversion;
      }
    }
    max = Math.max(max, at, column.target ?? 0, column.rmd ?? 0, column.conversion ?? 0);
    return column;
  });
  if (unit === "share") max = 1;

  // Lifetime shares: what each source paid over the years shown.
  const lifetimeOf = (s: DrawdownSeries) =>
    choice.years.reduce((sum, y) => sum + amountOf(s, partsOf(y, factor(y, basis))), 0);
  const totals = sources.map(lifetimeOf);
  const all = totals.reduce((a, b) => a + b, 0);
  const lifetime = sources
    .map((s, i) => ({ key: s.key, name: s.name, color: s.color, share: all > 0 ? totals[i] / all : 0 }))
    .filter((l) => l.share > 0);
  const taxSeries = series.find((s) => s.kind === "taxes");
  const lifetimeTax = taxSeries ? lifetimeOf(taxSeries) : 0;
  const lifetimeSpending = choice.years.reduce((sum, y) => sum + y.spending * factor(y, basis), 0);
  const lifetimeConversion = choice.years.reduce((sum, y) => sum + y.conversion * factor(y, basis), 0);
  const lifetimeConversionTax = choice.years.reduce((sum, y) => sum + y.conversion_tax * factor(y, basis), 0);

  const last = choice.years[choice.years.length - 1];
  const markers: MarkerPosition[] = [];
  for (const m of choice.summary.markers) {
    const index = years.indexOf(m.year);
    if (index >= 0) markers.push({ index, year: m.year, label: markerLabel(body, m.kind, m.id) });
  }

  return {
    series,
    columns,
    years,
    max,
    min,
    hasShortfall: columns.some((c) => c.cap != null),
    hasSurplus: columns.some((c) => c.surplus != null),
    hasRmd: choice.years.some((y) => y.rmd.some((v) => v > 0.5)),
    hasConversion: choice.years.some((y) => y.conversion > 0.5),
    lifetimeConversion,
    lifetimeConversionTax,
    markers,
    lifetime,
    lifetimeTax,
    lifetimeTaxShare: lifetimeSpending > 0 ? lifetimeTax / lifetimeSpending : 0,
    endBalance: basis === "real" ? choice.summary.ending_balance_real : choice.summary.ending_balance,
    endBalanceAfterTax:
      basis === "real" ? choice.summary.after_tax_ending_balance_real : choice.summary.after_tax_ending_balance,
    endAge: last ? ageIn(body, last.year) : undefined,
    endYear: last?.year ?? body.retirement.year,
    unit,
    basis,
  };
}

/**
 * Axis label: dollars compact, share as whole percents. Below zero is tax,
 * an amount rather than a debt, so it is labelled without the sign.
 */
export function axisLabel(unit: DrawdownUnit): (v: number) => string {
  return unit === "share"
    ? (v) => `${Math.round(v * 100)}%`
    : (v) => fmtCompact(Math.abs(v)).replace(/\.0+(?=[kM])/, "");
}

// ── the selected year ───────────────────────────────────────────────

export function yearPanel(
  body: DrawdownBody,
  choice: DrawdownChoice,
  view: DrawdownView,
  index: number,
): YearPanel | undefined {
  const y = choice.years[index];
  if (!y) return undefined;
  const parts = partsOf(y, factor(y, view.basis));
  const f = factor(y, view.basis);
  const paid = parts.spending - parts.gap;

  const rows: PanelRow[] = view.series.filter((s) => s.kind !== "taxes").map((s) => {
    const amount = amountOf(s, parts);
    let sub: string;
    switch (s.kind) {
      case "income":
        sub = "Income";
        break;
      case "cash":
        sub = "From bank balances";
        break;
      case "taxes":
        // Filtered out above: the tax is reported beside the funded total.
        sub = "";
        break;
      case "account": {
        const left = fmtCompact((y.balances[s.index] ?? 0) * f);
        const rmd = (y.rmd[s.index] ?? 0) * f;
        sub = rmd > 0.5 ? `${left} left · RMD ${fmtCompact(rmd)}` : `${left} left`;
        break;
      }
    }
    return {
      key: s.key,
      name: s.name,
      sub,
      color: s.color,
      share: parts.spending > 0 ? amount / parts.spending : 0,
      amount,
      dim: amount < 0.5 * f,
    };
  });

  const rmdAccounts = body.accounts.filter((_, i) => (y.rmd[i] ?? 0) > 0.5).map((a) => a.name);
  const roths = body.accounts
    .map((a, i) => ({ a, i }))
    .filter(({ a }) => a.tax_status === "TaxFree");
  const taxSeries = view.series.find((s) => s.kind === "taxes");
  let note: YearPanel["note"];
  if (parts.gap > 0.5 * f) {
    note = {
      tone: "bad",
      text: `Accounts cover ${fmtCurrency(paid)} of ${fmtCurrency(parts.spending)}. Short ${fmtCurrency(parts.gap)}.`,
    };
  } else if (parts.surplus > 0.5 * f) {
    note = {
      tone: "info",
      text: parts.rmd > 0.5 * f
        ? `The required ${fmtCurrency(parts.rmd)} RMD exceeds the need by ${fmtCurrency(parts.surplus)}; the excess lands as cash in the account the RMD pays into.`
        : `Withdrawals exceed the need by ${fmtCurrency(parts.surplus)}; the excess stays as cash.`,
    };
  }

  return {
    year: y.year,
    age: ageIn(body, y.year),
    target: parts.spending,
    rows,
    funded: parts.spending > 0 ? Math.min(1, Math.max(0, paid / parts.spending)) : 1,
    fundedAmount: paid,
    rmd: parts.rmd > 0.5 * f
      ? { amount: parts.rmd, afterTax: parts.rmdNet, accounts: rmdAccounts }
      : undefined,
    conversion: parts.conversion > 0.5 * f
      ? {
          amount: parts.conversion,
          tax: parts.conversionTax,
          roth: roths.reduce((sum, { i }) => sum + (y.balances[i] ?? 0) * f, 0),
          accounts: roths.map(({ a }) => a.name),
        }
      : undefined,
    taxes: taxSeries && parts.taxes > 0.5 * f
      ? {
          amount: parts.taxes,
          ofSpending: parts.spending > 0 ? parts.taxes / parts.spending : 0,
          color: taxSeries.color,
        }
      : undefined,
    note,
  };
}

// ── comparison strip ────────────────────────────────────────────────

export interface ComparisonLine {
  key: string;
  label: string;
  choice: StrategyChoice;
  success: number;
  /** Median ending balance with tax-deferred money net of the plan's rate: the headline. */
  afterTaxEndingBalance: number;
  /** The same before tax, for the hover. */
  endingBalance: number;
  tax?: number;
  bestSuccess: boolean;
  bestEnding: boolean;
  bestTax: boolean;
}

export function comparisonLines(comparison: DrawdownComparison, basis: DrawdownBasis): ComparisonLine[] {
  const preTax = (r: ComparisonRow) =>
    basis === "real" ? (r.median_final_net_worth_real ?? r.median_final_net_worth) : r.median_final_net_worth;
  // Judged after tax: before tax, paying tax early (a conversion, a
  // tax-deferred-first draw) always looks like losing money.
  const ending = (r: ComparisonRow) =>
    basis === "real"
      ? (r.median_after_tax_ending_balance_real ?? r.median_after_tax_ending_balance)
      : r.median_after_tax_ending_balance;
  const rows = comparison.rows;
  const bestSuccess = Math.max(...rows.map((r) => r.success_rate));
  const bestEnding = Math.max(...rows.map(ending));
  const taxes = rows.map((r) => r.median_path_tax).filter((t): t is number => t != null);
  const bestTax = taxes.length ? Math.min(...taxes) : undefined;
  // A tie across every row picks nothing out.
  const spread = <T,>(values: T[]) => new Set(values).size > 1;
  const successVaries = spread(rows.map((r) => r.success_rate));
  const endingVaries = spread(rows.map(ending));
  const taxVaries = spread(taxes);
  return rows.map((r) => ({
    key: choiceKey(r.choice),
    label: choiceLabel(r.choice),
    choice: r.choice,
    success: r.success_rate,
    afterTaxEndingBalance: ending(r),
    endingBalance: preTax(r),
    tax: r.median_path_tax ?? undefined,
    bestSuccess: successVaries && r.success_rate === bestSuccess,
    bestEnding: endingVaries && ending(r) === bestEnding,
    bestTax: taxVaries && r.median_path_tax != null && r.median_path_tax === bestTax,
  }));
}

// ── CSV ─────────────────────────────────────────────────────────────

function cell(value: string | number | undefined): string {
  if (value == null) return "";
  if (typeof value === "number") return String(Math.round(value * 100) / 100);
  return /[",\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

/** Dollars by year as the chart shows them (sources net of withheld tax), in the chosen basis. */
export function drawdownCsv(
  body: DrawdownBody,
  choice: DrawdownChoice,
  basis: DrawdownBasis,
): string {
  const view = drawdownView(body, choice, "usd", basis);
  const head = [
    "Year",
    "Age",
    "Target spend",
    ...view.series.map((s) => s.name),
    "Shortfall",
    "Surplus",
    "Required RMD",
    "Roth conversion",
    "Tax on conversions",
    "Total tax",
  ];
  const lines = [head.map(cell).join(",")];
  choice.years.forEach((y) => {
    const f = factor(y, basis);
    const parts = partsOf(y, f);
    lines.push(
      [
        y.year,
        ageIn(body, y.year),
        parts.spending,
        ...view.series.map((s) => amountOf(s, parts)),
        parts.gap,
        parts.surplus,
        parts.rmd,
        parts.conversion,
        parts.conversionTax,
        y.total_tax * f,
      ]
        .map(cell)
        .join(","),
    );
  });
  return lines.join("\n") + "\n";
}

/**
 * The explicit request list for a bracket ceiling: As planned, then each
 * strategy, all with the conversion toggle's setting.
 */
export function requestChoices(ceiling: number, conversion?: ConversionSetting): StrategyChoice[] {
  return withConversion(
    [
      { kind: "AsPlanned" },
      ...STRATEGY_ORDER.map((strategy): StrategyChoice =>
        strategy === "BracketFilling"
          ? { kind: "Strategy", strategy, bracket_ceiling: ceiling }
          : { kind: "Strategy", strategy },
      ),
    ],
    conversion,
  );
}
