/**
 * The `Spending` effect's rule (spec 22), for the screens: its names, the
 * one-line description the event rail prints, and switching between rules
 * while keeping the starting amount.
 *
 * Dollar figures on a rule are in today's (plan-start) dollars; rates are
 * fractions of the portfolio.
 */
import type {
  Account,
  Asset,
  EffectSpec,
  Event,
  EventBody,
  SpendingComparison,
  SpendingComparisonRow,
  SpendingPortfolioSpec,
  SpendingResults,
  SpendingRuleSpec,
} from "@/lib/api/types";
import { money } from "./format.ts";

export type SpendingRuleKind = SpendingRuleSpec["rule"];
export type GuardrailsRule = Extract<SpendingRuleSpec, { rule: "Guardrails" }>;

/** The rule menu's choices. "Fixed" is not a rule: it is a plain Expense. */
export type SpendingChoice = SpendingRuleKind | "Fixed";

export const SPENDING_CHOICES: { value: SpendingChoice; label: string; detail: string }[] = [
  { value: "Guardrails", label: "Guardrails", detail: "cut or raise 10% at the edges" },
  { value: "PercentOfPortfolio", label: "Percent of portfolio", detail: "a share every year" },
  { value: "CappedChange", label: "Capped change", detail: "follows the market slowly" },
  { value: "Vpw", label: "VPW", detail: "spends down by an age" },
  { value: "Fixed", label: "Fixed", detail: "the same every year" },
];

export const RULE_LABEL: Record<SpendingRuleKind, string> = {
  Guardrails: "Guardrails",
  PercentOfPortfolio: "Percent of portfolio",
  CappedChange: "Capped change",
  Vpw: "VPW",
};

/** One sentence on what each choice does, for the setup step and the editor. */
export const CHOICE_EXPLAINED: Record<SpendingChoice, string> = {
  Fixed: "The same amount every year, raised for inflation, whatever the market does.",
  Guardrails:
    "Starts at your amount and rises with inflation; cuts 10% when withdrawals grow 20% past where they started, raises 10% when they fall 20% below.",
  PercentOfPortfolio: "A fixed share of the portfolio each year: never runs out, but swings with every market.",
  CappedChange: "Aims for a share of the portfolio but moves at most 2.5% down or 5% up a year.",
  Vpw: "Spends the portfolio down by an age you choose, more each year as fewer remain.",
};

/** Guardrails at their published defaults (Guyton-Klinger), from `amount`. */
export function guardrails(amount: number): GuardrailsRule {
  return {
    rule: "Guardrails",
    initial: { kind: "Amount", amount },
    band: 0.2,
    step: 0.1,
    skip_inflation_after_loss: true,
    protect_final_years: 15,
    floor: null,
    ceiling: null,
  };
}

/** The portfolio's opening value, at the assets' opening prices: what a rate is of. */
export function portfolioValue(
  accounts: Account[],
  assets: Asset[],
  portfolio: SpendingPortfolioSpec = { kind: "Liquid" },
): number {
  const price = new Map(assets.map((a) => [a.id, a.initial_price]));
  const included = (id: number) => portfolio.kind === "Liquid" || portfolio.account_ids.includes(id);
  return accounts
    .filter((a) => included(a.id))
    .reduce((sum, a) => {
      if (a.flavor === "Bank") return sum + a.cash_value;
      if (a.flavor === "Investment")
        return sum + a.cash_value + a.positions.reduce((s, p) => s + p.units * (price.get(p.asset_id) ?? 1), 0);
      return sum;
    }, 0);
}

/**
 * The yearly amount a rule starts at, in today's dollars: its own amount, or
 * its rate of `portfolio`; a VPW rule has none of its own.
 */
export function startingAmount(rule: SpendingRuleSpec, portfolio: number): number | null {
  switch (rule.rule) {
    case "Guardrails":
      return rule.initial.kind === "Amount" ? rule.initial.amount : rule.initial.rate * portfolio;
    case "PercentOfPortfolio":
    case "CappedChange":
      return rule.rate * portfolio;
    case "Vpw":
      return null;
  }
}

/** `amount` as a rate of `portfolio`, kept inside what validation accepts. */
function rateOf(amount: number | null, portfolio: number, fallback: number): number {
  if (amount == null || !(portfolio > 0) || !(amount > 0)) return fallback;
  return Math.min(0.2, Math.max(0.001, Number((amount / portfolio).toFixed(4))));
}

/**
 * `rule` turned into `to`, keeping the starting amount: Guardrails start at
 * it, the rate rules at the rate it is of the portfolio. The clamps stay
 * where both rules have them.
 */
export function switchRule(rule: SpendingRuleSpec, to: SpendingRuleKind, portfolio: number): SpendingRuleSpec {
  if (rule.rule === to) return rule;
  const amount = startingAmount(rule, portfolio);
  const floor = "floor" in rule ? rule.floor : null;
  const ceiling = "ceiling" in rule ? rule.ceiling : null;
  switch (to) {
    case "Guardrails":
      return { ...guardrails(Math.round(amount ?? 0.045 * portfolio)), floor, ceiling };
    case "PercentOfPortfolio":
      return { rule: to, rate: rateOf(amount, portfolio, 0.04), floor, ceiling };
    case "CappedChange":
      return { rule: to, rate: rateOf(amount, portfolio, 0.05), max_cut: 0.025, max_raise: 0.05 };
    case "Vpw":
      return { rule: to, real_return: null, final_age: 100 };
  }
}

const pct = (rate: number) => `${Number((rate * 100).toFixed(2))}%`;

/** The rule in a line: "guardrails from $80,000 a year, ±20%, steps of 10%". */
export function describeRule(rule: SpendingRuleSpec): string {
  const clamps = (floor: number | null, ceiling: number | null) =>
    [floor != null ? `floor ${money(floor)}` : "", ceiling != null ? `ceiling ${money(ceiling)}` : ""]
      .filter(Boolean)
      .map((s) => ` · ${s}`)
      .join("");
  switch (rule.rule) {
    case "Guardrails": {
      const start = rule.initial.kind === "Amount"
        ? `${money(rule.initial.amount)} a year`
        : `${pct(rule.initial.rate)} of the portfolio`;
      return `guardrails from ${start}, ±${pct(rule.band)}, steps of ${pct(rule.step)}${clamps(rule.floor, rule.ceiling)}`;
    }
    case "PercentOfPortfolio":
      return `${pct(rule.rate)} of the portfolio${clamps(rule.floor, rule.ceiling)}`;
    case "CappedChange":
      return `toward ${pct(rule.rate)} of the portfolio, −${pct(rule.max_cut)}/+${pct(rule.max_raise)} a year`;
    case "Vpw":
      return `VPW to age ${rule.final_age}${rule.real_return != null ? ` at ${pct(rule.real_return)} real` : ""}`;
  }
}

/** Why a rule cannot be saved as it stands, or null. Mirrors the server's checks. */
export function ruleProblem(rule: SpendingRuleSpec): string | null {
  const rateOk = (r: number) => r > 0 && r <= 0.2;
  const fraction = (v: number) => v > 0 && v < 1;
  const clampsOk = (floor: number | null, ceiling: number | null) =>
    floor == null || ceiling == null || floor <= ceiling;
  switch (rule.rule) {
    case "Guardrails":
      if (rule.initial.kind === "Amount" ? !(rule.initial.amount > 0) : !rateOk(rule.initial.rate))
        return "needs a starting amount above zero, or a rate up to 20%";
      if (!fraction(rule.band) || !fraction(rule.step)) return "needs a band and a step between 0% and 100%";
      return clampsOk(rule.floor, rule.ceiling) ? null : "has its floor above its ceiling";
    case "PercentOfPortfolio":
      if (!rateOk(rule.rate)) return "needs a rate above 0% and at most 20%";
      return clampsOk(rule.floor, rule.ceiling) ? null : "has its floor above its ceiling";
    case "CappedChange":
      if (!rateOk(rule.rate)) return "needs a rate above 0% and at most 20%";
      return rule.max_cut >= 0 && rule.max_cut < 1 && rule.max_raise >= 0 && rule.max_raise <= 1
        ? null
        : "needs its caps between 0% and 100%";
    case "Vpw":
      return rule.final_age > 0 ? null : "needs a final age";
  }
}

/** What the Results screen's Spending card draws: a run's `spending` block. */
export interface SpendingView {
  /** Full calendar years only, so a part year at either end cannot read as a cut. */
  years: number[];
  /** The plan's age at each year, or the year again without a birth date. */
  ages: number[];
  /** Real total spending (P10–P90, P25–P75 and P50) by year. */
  low: number[];
  high: number[];
  lowerQuartile: number[];
  upperQuartile: number[];
  p50: number[];
  medianLifetime: number;
  p10LowestDraw: number;
  /** Share of markets, 0–1, with a cut in the draw deeper than 10%. */
  cutRate: number;
  medianYearsBelowFirst: number;
  medianFirstDraw: number;
  /** The rules the figures describe, by name; empty on older runs. */
  rules: string[];
  /**
   * Whether a cut deeper than 10% can happen at all: not under rules that
   * each cap their own cut at 10% or less (Capped change).
   */
  cutsCanExceed10: boolean;
}

/**
 * A run's spending block for the card, or null when the plan has no rule.
 * `startDate` is the plan's: its first year is a part year unless it starts
 * on January 1; the last point is one unless it closes on December 31.
 */
export function toSpendingView(
  spending: SpendingResults | undefined | null,
  startDate: string,
  ageAt: (date: string) => number,
): SpendingView | null {
  if (!spending) return null;
  const full = spending.points.filter((point, i, all) => {
    if (i === 0 && !startDate.endsWith("-01-01")) return false;
    if (i === all.length - 1 && !point.date.endsWith("-12-31")) return false;
    return true;
  });
  const pick = (key: "p10" | "p25" | "p75" | "p90", fallback: "p5" | "p95") =>
    full.map((p) => p[key] ?? p[fallback]);
  return {
    years: full.map((p) => Number(p.date.slice(0, 4))),
    ages: full.map((p) => ageAt(p.date)),
    low: pick("p10", "p5"),
    high: pick("p90", "p95"),
    lowerQuartile: pick("p25", "p5"),
    upperQuartile: pick("p75", "p95"),
    p50: full.map((p) => p.p50),
    medianLifetime: spending.median_lifetime_spending,
    p10LowestDraw: spending.p10_lowest_draw,
    cutRate: spending.cut_rate,
    medianYearsBelowFirst: spending.median_years_below_first,
    medianFirstDraw: spending.median_first_draw,
    rules: spending.rules ?? [],
    cutsCanExceed10: spending.cut_cap == null || spending.cut_cap > 0.1,
  };
}

/** One row of the Spending rules comparison, with the best of each column marked. */
export interface RuleLine {
  key: string;
  label: string;
  row: SpendingComparisonRow;
  bestFunding: boolean;
  bestLifetime: boolean;
  bestLowest: boolean;
  fewestCuts: boolean;
}

/** The comparison's rows, the best figure in each column marked. */
export function ruleLines(comparison: SpendingComparison): RuleLine[] {
  const rows = comparison.rows;
  const max = (pick: (r: SpendingComparisonRow) => number) => Math.max(...rows.map(pick));
  const min = (pick: (r: SpendingComparisonRow) => number) => Math.min(...rows.map(pick));
  const funding = max((r) => r.funding_success_rate ?? 0);
  const lifetime = max((r) => r.median_lifetime_spending);
  const lowest = max((r) => r.p10_lowest_draw);
  const cuts = min((r) => r.cut_rate);
  const close = (a: number, b: number) => Math.abs(a - b) <= Math.max(1e-9, Math.abs(b) * 1e-9);
  return rows.map((row, i) => ({
    key: `${i}:${row.label}`,
    label: row.label,
    row,
    bestFunding: close(row.funding_success_rate ?? 0, funding),
    bestLifetime: close(row.median_lifetime_spending, lifetime),
    bestLowest: close(row.p10_lowest_draw, lowest),
    fewestCuts: close(row.cut_rate, cuts),
  }));
}

/**
 * `event` with its spending (its `Spending` effect, else its first
 * `Expense`) replaced by `effect`, as the body that saves it.
 */
export function withSpending(event: Event, effect: EffectSpec): EventBody {
  const at = event.effects.findIndex((e) => e.kind === "Spending");
  const index = at >= 0 ? at : event.effects.findIndex((e) => e.kind === "Expense");
  const effects = event.effects.map((e, i) => (i === index ? effect : e));
  return {
    name: event.name,
    description: event.description,
    fires_once: event.fires_once,
    enabled: event.enabled,
    sort_order: null,
    trigger: event.trigger,
    effects,
  };
}
