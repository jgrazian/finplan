/**
 * The What-if stack → what the override cards, the waterfall and the fan draw.
 *
 * The only place the what-if API shapes are read. A stack is an ordered list of
 * layers, each switched on or off; the enabled ones are applied top to bottom
 * on a copy of the plan, and the analysis answers with one step per prefix —
 * `steps[0]` the plan alone, `steps[i]` the plan plus the first `i` layers — so
 * what one layer costs or buys is the difference between its step and the one
 * before it.
 *
 * Pure: no React, no requests. The screen owns the stack and the job; this owns
 * how both read.
 */
import type {
  AnalysisParameter,
  WhatIfEntry,
  WhatIfFan,
  WhatIfLayer,
  WhatIfOutcome,
} from "@/lib/api/types";
import { fmtAxis, fmtCompact, fmtCurrency, fmtPercent } from "../format.ts";
import { parameterDate, parameterDay, paramValue } from "./analysis.ts";

/** The server's cap on layers in one run, restated so the screen can say so first. */
export const MAX_RUN_LAYERS = 8;
/** The server's cap on a stored stack. */
export const MAX_STORED_ENTRIES = 16;

/** A market shock's drop is stepped within this band, as fractions. */
export const SHOCK_MIN = 0.05;
export const SHOCK_MAX = 0.9;
const SHOCK_STEP = 0.05;
/** A one-off amount steps by this much, and never below it. */
const ONE_OFF_STEP = 5_000;
/** A rate parameter steps by a quarter point. */
const RATE_STEP = 0.0025;

/** What the stack is read against: the plan's own inputs and horizon. */
export interface WhatIfContext {
  parameters: AnalysisParameter[];
  /**
   * The owner's whole-year age at the plan's start and end; `null` when the
   * scenario has no birth date, which is also what disables the age layers.
   */
  ages: { start: number; end: number } | null;
}

// ──────────────────────────── formatting ────────────────────────────

/** Compact money that stops at zero, the way a balance that ran dry reads. */
export function moneyFloor(value: number): string {
  return value <= 0 ? "$0" : fmtCompact(value);
}

/** Points of success, signed: `+2.1 pts`, `−0.4 pts`. */
export function signedPoints(deltaFraction: number): string {
  const pts = deltaFraction * 100;
  if (Math.abs(pts) < 0.05) return "±0.0 pts";
  return `${pts > 0 ? "+" : "−"}${Math.abs(pts).toFixed(1)} pts`;
}

/** Against the plan: `+2.1 pts vs plan`, or `same as plan` within a twentieth of a point. */
export function pointsVsPlan(deltaFraction: number): string {
  const pts = deltaFraction * 100;
  if (Math.abs(pts) < 0.05) return "same as plan";
  return `${pts > 0 ? "+" : "−"}${Math.abs(pts).toFixed(1)} pts vs plan`;
}

/** Money against the plan, with $5k of slack before it counts as a change. */
export function moneyVsPlan(delta: number): string {
  if (Math.abs(delta) < 5_000) return "same as plan";
  return `${delta > 0 ? "+" : "−"}${fmtCompact(Math.abs(delta))} vs plan`;
}

// ──────────────────────────── step sizes ────────────────────────────

/**
 * A round step near `magnitude`: 1, 2, 2.5 or 5 times a power of ten, the
 * nearest on a log scale. `2_400` → `2_500`, `6_000` → `5_000`.
 */
export function niceStep(magnitude: number): number {
  if (!(magnitude > 0) || !Number.isFinite(magnitude)) return 1;
  const pow = 10 ** Math.floor(Math.log10(magnitude));
  let best = pow;
  for (const m of [1, 2, 2.5, 5, 10]) {
    const candidate = m * pow;
    if (Math.abs(Math.log(candidate / magnitude)) < Math.abs(Math.log(best / magnitude))) {
      best = candidate;
    }
  }
  return best;
}

/**
 * How far one press of − or + moves an amount parameter: about 5% of the
 * plan's own value, rounded. Taken from the plan rather than from the layer,
 * so ten presses up and ten down land back where they started.
 */
export function amountStep(planValue: number): number {
  const magnitude = Math.abs(planValue) * 0.05;
  return magnitude === 0 ? 1_000 : niceStep(magnitude);
}

function clamp(value: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, value));
}

/** One calendar year either way, on the API's epoch-day dates. */
function shiftYear(epochDays: number, years: number): number {
  const iso = parameterDate(epochDays);
  const [y, m, d] = iso.split("-").map(Number);
  const target = y + years;
  const leap = target % 4 === 0 && (target % 100 !== 0 || target % 400 === 0);
  const day = m === 2 && d === 29 && !leap ? 28 : d;
  return parameterDay(`${target}-${String(m).padStart(2, "0")}-${String(day).padStart(2, "0")}`);
}

// ──────────────────────────── the layers ────────────────────────────

/** One of a card's steppable values. */
export type LayerSlot = "value" | "drop" | "amount" | "age";

/** A piece of a card's sentence: words, or a value between − and +. */
export type SentencePart =
  | { type: "text"; text: string; /** An aside — the plan's own value — set smaller. */ muted?: boolean }
  | { type: "stepper"; slot: LayerSlot; value: string; ariaLabel: string; canDec: boolean; canInc: boolean };

/** One override card, as it reads. */
export interface LayerView {
  id: string;
  enabled: boolean;
  /** The card's small label: `plan input`, `market shock`, `one-off cost`, `windfall`. */
  kind: string;
  parts: SentencePart[];
  /** The waterfall's name for this layer — short, one line. */
  short: string;
  /**
   * The layer cannot run: a parameter the plan no longer has, or an age with
   * no birth date to hang it on. Shown, never sent.
   */
  problem: string | null;
}

function parameterOf(ctx: WhatIfContext, id: number): AnalysisParameter | undefined {
  return ctx.parameters.find((p) => p.parameter_id === id);
}

/** The whole-year ages an age layer may sit at. */
function ageBounds(ctx: WhatIfContext): [number, number] {
  return ctx.ages ? [ctx.ages.start, ctx.ages.end] : [0, 120];
}

/** How a parameter's value reads on its card. */
function parameterText(kind: string, value: number): string {
  if (kind === "rate") return `${Number((value * 100).toFixed(4))}%`;
  if (kind === "age") {
    const months = Math.round(value * 12);
    return months % 12 === 0 ? String(months / 12) : paramValue("age", value);
  }
  return paramValue(kind, value);
}

/** Why a layer cannot be run as it stands, or null. */
export function layerProblem(layer: WhatIfLayer, ctx: WhatIfContext): string | null {
  if (layer.kind === "parameter") {
    return parameterOf(ctx, layer.parameter_id) ? null : "This parameter is no longer in the plan.";
  }
  return ctx.ages ? null : "Needs a birth date on the scenario to place an age.";
}

/** The card's sentence and labels. */
export function layerView(entry: WhatIfEntry, ctx: WhatIfContext): LayerView {
  const { layer } = entry;
  const problem = layerProblem(layer, ctx);
  const [ageLo, ageHi] = ageBounds(ctx);
  const text = (t: string): SentencePart => ({ type: "text", text: t });
  const ageStepper = (age: number): SentencePart => ({
    type: "stepper",
    slot: "age",
    value: String(age),
    ariaLabel: "age",
    canDec: age > ageLo,
    canInc: age < ageHi,
  });

  switch (layer.kind) {
    case "parameter": {
      const parameter = parameterOf(ctx, layer.parameter_id);
      if (!parameter) {
        return {
          id: entry.id,
          enabled: entry.enabled,
          kind: "plan input",
          parts: [text("A parameter no longer in the plan")],
          short: "removed",
          problem,
        };
      }
      const moved = Math.abs(layer.value - parameter.current) > 1e-9;
      return {
        id: entry.id,
        enabled: entry.enabled,
        kind: "plan input",
        parts: [
          text(parameter.name),
          {
            type: "stepper",
            slot: "value",
            value: parameterText(parameter.kind, layer.value),
            ariaLabel: parameter.name,
            canDec: canStepParameter(parameter, layer.value, -1),
            canInc: canStepParameter(parameter, layer.value, 1),
          },
          ...(moved
            ? [{ type: "text" as const, text: `plan: ${parameterText(parameter.kind, parameter.current)}`, muted: true }]
            : []),
        ],
        short: parameter.name,
        problem,
      };
    }
    case "market-shock":
      return {
        id: entry.id,
        enabled: entry.enabled,
        kind: "market shock",
        parts: [
          text("Markets fall"),
          {
            type: "stepper",
            slot: "drop",
            value: `${Math.round(layer.drop * 100)}%`,
            ariaLabel: "market drop",
            canDec: layer.drop > SHOCK_MIN + 1e-9,
            canInc: layer.drop < SHOCK_MAX - 1e-9,
          },
          text("at age"),
          ageStepper(layer.age),
        ],
        short: `−${Math.round(layer.drop * 100)}% at ${layer.age}`,
        problem,
      };
    case "one-off": {
      const cost = layer.amount < 0;
      const magnitude = Math.abs(layer.amount);
      return {
        id: entry.id,
        enabled: entry.enabled,
        kind: cost ? "one-off cost" : "windfall",
        parts: [
          text(cost ? "One-off cost of" : "Windfall of"),
          {
            type: "stepper",
            slot: "amount",
            value: fmtCompact(magnitude),
            ariaLabel: cost ? "cost" : "windfall",
            canDec: magnitude > ONE_OFF_STEP,
            canInc: true,
          },
          text("at age"),
          ageStepper(layer.age),
        ],
        short: `${cost ? "−" : "+"}${fmtCompact(magnitude)} at ${layer.age}`,
        problem,
      };
    }
  }
}

function canStepParameter(parameter: AnalysisParameter, value: number, dir: 1 | -1): boolean {
  const next = stepParameter(parameter, value, dir);
  return next !== value;
}

function stepParameter(parameter: AnalysisParameter, value: number, dir: 1 | -1): number {
  switch (parameter.kind) {
    case "age":
      return clamp(value + dir, 0, 120);
    case "rate":
      // Rounded to the quarter point's own precision, so repeated presses do
      // not pile up floating error into a `4.2500000001%`.
      return Math.round((value + dir * RATE_STEP) * 1e6) / 1e6;
    case "date":
      return shiftYear(value, dir);
    case "amount": {
      const step = amountStep(parameter.current);
      const next = value + dir * step;
      // A spending figure the plan has as positive does not step through zero.
      return parameter.current >= 0 ? Math.max(0, next) : next;
    }
    default:
      return value + dir;
  }
}

/** The layer after one press of − (`-1`) or + (`1`) on one of its values. */
export function stepLayer(
  layer: WhatIfLayer,
  slot: LayerSlot,
  dir: 1 | -1,
  ctx: WhatIfContext,
): WhatIfLayer {
  const [ageLo, ageHi] = ageBounds(ctx);
  switch (layer.kind) {
    case "parameter": {
      const parameter = parameterOf(ctx, layer.parameter_id);
      if (!parameter || slot !== "value") return layer;
      return { ...layer, value: stepParameter(parameter, layer.value, dir) };
    }
    case "market-shock":
      if (slot === "drop") {
        const drop = Math.round((layer.drop + dir * SHOCK_STEP) * 100) / 100;
        return { ...layer, drop: clamp(drop, SHOCK_MIN, SHOCK_MAX) };
      }
      if (slot === "age") return { ...layer, age: clamp(layer.age + dir, ageLo, ageHi) };
      return layer;
    case "one-off":
      if (slot === "amount") {
        const sign = layer.amount < 0 ? -1 : 1;
        const magnitude = Math.max(ONE_OFF_STEP, Math.abs(layer.amount) + dir * ONE_OFF_STEP);
        return { ...layer, amount: sign * magnitude };
      }
      if (slot === "age") return { ...layer, age: clamp(layer.age + dir, ageLo, ageHi) };
      return layer;
  }
}

// ──────────────────────────── adding layers ────────────────────────────

/** What the Add override menu offers. */
export type NewLayerChoice =
  | { kind: "parameter"; parameterId: number }
  | { kind: "market-shock" }
  | { kind: "cost" }
  | { kind: "windfall" };

export interface AddOption {
  key: string;
  label: string;
  /** Right-aligned hint: the parameter's plan value. */
  detail?: string;
  choice: NewLayerChoice;
  /** Why it cannot be added, as its tooltip; null when it can. */
  disabled: string | null;
}

/** Every override that can be added: unused parameters first, then the events. */
export function addOptions(entries: WhatIfEntry[], ctx: WhatIfContext): AddOption[] {
  const used = new Set(
    entries.flatMap((e) => (e.layer.kind === "parameter" ? [e.layer.parameter_id] : [])),
  );
  const full = entries.length >= MAX_STORED_ENTRIES
    ? `A stack holds at most ${MAX_STORED_ENTRIES} overrides.`
    : null;
  const noAge = ctx.ages ? null : "Set a birth date on the scenario to place events at an age.";
  const out: AddOption[] = ctx.parameters
    .filter((p) => !used.has(p.parameter_id))
    .map((p) => ({
      key: `parameter:${p.parameter_id}`,
      label: p.name,
      detail: parameterText(p.kind, p.current),
      choice: { kind: "parameter", parameterId: p.parameter_id },
      disabled: full,
    }));
  out.push(
    { key: "market-shock", label: "Market shock", choice: { kind: "market-shock" }, disabled: full ?? noAge },
    { key: "cost", label: "One-off cost", choice: { kind: "cost" }, disabled: full ?? noAge },
    { key: "windfall", label: "Windfall", choice: { kind: "windfall" }, disabled: full ?? noAge },
  );
  return out;
}

/**
 * The layer a menu choice starts as. A parameter starts at the plan's own
 * value, so adding it changes nothing until it is stepped; an event starts at
 * a round figure a few years out, or at retirement when the plan names one.
 */
export function newLayer(
  choice: NewLayerChoice,
  ctx: WhatIfContext,
  retirementAge?: number | null,
): WhatIfLayer {
  const [lo, hi] = ageBounds(ctx);
  const at = (offset: number) => clamp(Math.round((ctx.ages?.start ?? 0) + offset), lo, hi);
  switch (choice.kind) {
    case "parameter": {
      const parameter = parameterOf(ctx, choice.parameterId);
      return { kind: "parameter", parameter_id: choice.parameterId, value: parameter?.current ?? 0 };
    }
    case "market-shock":
      return {
        kind: "market-shock",
        age: retirementAge != null ? clamp(Math.round(retirementAge), lo, hi) : at(10),
        drop: 0.3,
      };
    case "cost":
      return { kind: "one-off", age: at(5), amount: -40_000, account_id: null };
    case "windfall":
      return { kind: "one-off", age: at(10), amount: 50_000, account_id: null };
  }
}

// ──────────────────────────── what runs ────────────────────────────

/** The enabled, runnable layers in order, with the entry each came from. */
export function runnableEntries(entries: WhatIfEntry[], ctx: WhatIfContext): WhatIfEntry[] {
  return entries.filter((e) => e.enabled && layerProblem(e.layer, ctx) == null);
}

/**
 * The run's identity: two stacks that would send the same request share it,
 * so toggling a layer that cannot run, or reordering nothing, starts nothing.
 */
export function runKey(entries: WhatIfEntry[]): string {
  return JSON.stringify(entries.map((e) => e.layer));
}

/** The header line beside the mode switch. */
export function changedSummary(entries: WhatIfEntry[], ctx: WhatIfContext): string {
  if (entries.length === 0) return "No overrides — this is the plan as saved.";
  const on = runnableEntries(entries, ctx).length;
  const off = entries.length - on;
  const head = on === 0
    ? "Every override is off — this is the plan as saved"
    : `${on} override${on === 1 ? "" : "s"} on`;
  return `${head}${off > 0 && on > 0 ? `, ${off} off` : ""} · the plan itself is unchanged until applied`;
}

/** What Apply or Save will write, one line per runnable layer. */
export function applyLines(entries: WhatIfEntry[], ctx: WhatIfContext): string[] {
  return runnableEntries(entries, ctx).map(({ layer }) => {
    switch (layer.kind) {
      case "parameter": {
        const parameter = parameterOf(ctx, layer.parameter_id);
        return parameter
          ? `Set ${parameter.name} to ${parameterText(parameter.kind, layer.value)} (now ${parameterText(parameter.kind, parameter.current)})`
          : "";
      }
      case "market-shock":
        return `Add an event: markets fall ${Math.round(layer.drop * 100)}% at age ${layer.age}`;
      case "one-off":
        return layer.amount < 0
          ? `Add an event: one-off cost of ${fmtCurrency(-layer.amount)} at age ${layer.age}`
          : `Add an event: windfall of ${fmtCurrency(layer.amount)} at age ${layer.age}`;
    }
  });
}

// ──────────────────────────── the answer ────────────────────────────

/**
 * Each entry's contribution, keyed by entry id, given the entries the outcome
 * was run for (in order): the change in success from the step before it.
 */
export function layerImpacts(outcome: WhatIfOutcome, ranFor: string[]): Map<string, number> {
  const out = new Map<string, number>();
  ranFor.forEach((id, i) => {
    const before = outcome.steps[i];
    const after = outcome.steps[i + 1];
    if (before && after) out.set(id, after.point.success_rate - before.point.success_rate);
  });
  return out;
}

/** The impact column of a card. */
export function impactText(
  entry: WhatIfEntry,
  impacts: Map<string, number> | undefined,
  problem: string | null,
): string {
  if (!entry.enabled) return "off";
  if (problem) return "—";
  const delta = impacts?.get(entry.id);
  return delta == null ? "…" : signedPoints(delta);
}

export interface WhatIfStats {
  success: string;
  successDelta: string;
  /** `median at 95`, or `median in 2071` without a birth date. */
  endLabel: string;
  end: string;
  endDelta: string;
  dry: string;
  dryDelta: string;
}

function dryText(at: number | null, byAge: boolean): string {
  if (at == null) return "never";
  return byAge ? `age ${Math.round(at)}` : String(Math.round(at));
}

/** The three figures over the charts, each against the plan. */
export function whatIfStats(outcome: WhatIfOutcome): WhatIfStats {
  const plan = outcome.steps[0];
  const last = outcome.steps[outcome.steps.length - 1];
  const byAge = outcome.ages != null;
  const axis = outcome.ages ?? outcome.years;
  const endAt = axis.length > 0 ? Math.round(axis[axis.length - 1]) : undefined;
  return {
    success: fmtPercent(last.point.success_rate),
    successDelta: pointsVsPlan(last.point.success_rate - plan.point.success_rate),
    endLabel: endAt == null ? "median at end" : byAge ? `median at ${endAt}` : `median in ${endAt}`,
    end: moneyFloor(last.median_end_real),
    endDelta: moneyVsPlan(last.median_end_real - plan.median_end_real),
    dry: dryText(last.p10_dry_at, byAge),
    dryDelta: `plan: ${dryText(plan.p10_dry_at, byAge)}`,
  };
}

// ──────────────────────────── the waterfall ────────────────────────────

export const WATERFALL = { w: 760, h: 210, left: 44, right: 12, top: 22, bottom: 40 } as const;

export type BarTone = "plan" | "gain" | "loss" | "flat" | "total";

export interface WaterfallBar {
  key: string;
  x: number;
  y: number;
  w: number;
  h: number;
  tone: BarTone;
  /** Above the bar: the rate for the two totals, the change for a layer. */
  label: string;
  labelY: number;
  /** Under the axis. */
  name: string;
  /** Full name, for the tooltip. */
  title: string;
  cx: number;
  /** The dashed carry-over to the next bar, at this bar's running total. */
  connector: { x1: number; x2: number; y: number } | null;
}

export interface WaterfallView {
  bars: WaterfallBar[];
  grid: Array<{ y: number; label: string }>;
  /** The plot's floor, where the two total bars stand. */
  base: number;
}

/**
 * Plan, one bar per applied layer, and the what-if total.
 *
 * Drawn against the band of success the stack moves through, not 0–100%:
 * every real plan sits in the last few points of the scale, and a full axis
 * draws a three-point layer as a hairline. The two totals stand on the axis
 * floor, which the gridline labels name.
 */
export function waterfallView(
  outcome: WhatIfOutcome,
  names: string[],
  titles: string[] = names,
): WaterfallView {
  const G = WATERFALL;
  const rates = outcome.steps.map((s) => s.point.success_rate * 100);
  const lo0 = Math.min(...rates);
  const hi0 = Math.max(...rates);
  let lo = Math.max(0, Math.floor((lo0 - 5) / 5) * 5);
  let hi = Math.min(100, Math.ceil((hi0 + 2) / 5) * 5);
  if (hi - lo < 20) {
    hi = Math.min(100, Math.max(hi, lo + 20));
    lo = Math.max(0, hi - 20);
  }
  const plotH = G.h - G.top - G.bottom;
  const base = G.h - G.bottom;
  const y = (pct: number) => base - ((clamp(pct, lo, hi) - lo) / (hi - lo)) * plotH;

  const count = rates.length + 1;
  const slot = (G.w - G.left - G.right) / count;
  const barW = Math.min(64, slot * 0.56);
  const cx = (i: number) => G.left + slot * (i + 0.5);
  const maxChars = Math.max(6, Math.floor(slot / 6.2));
  const clip = (s: string) => (s.length > maxChars ? `${s.slice(0, maxChars - 1)}…` : s);

  const bars: WaterfallBar[] = [];
  const planRate = rates[0];
  bars.push({
    key: "plan",
    x: cx(0) - barW / 2,
    y: y(planRate),
    w: barW,
    h: base - y(planRate),
    tone: "plan",
    label: `${planRate.toFixed(1)}%`,
    labelY: y(planRate) - 6,
    name: "Plan",
    title: "The plan as saved",
    cx: cx(0),
    connector: null,
  });
  for (let i = 1; i < rates.length; i += 1) {
    const from = rates[i - 1];
    const to = rates[i];
    const top = y(Math.max(from, to));
    const bottom = y(Math.min(from, to));
    const delta = to - from;
    const tone: BarTone = Math.abs(delta) < 0.05 ? "flat" : delta > 0 ? "gain" : "loss";
    bars.push({
      key: `layer-${i}`,
      x: cx(i) - barW / 2,
      y: top,
      // A layer that changes nothing still draws a line where it sits.
      w: barW,
      h: Math.max(1.5, bottom - top),
      tone,
      label: signedPoints(delta / 100).replace(" pts", ""),
      labelY: top - 6,
      name: clip(names[i - 1] ?? `Layer ${i}`),
      title: titles[i - 1] ?? names[i - 1] ?? `Layer ${i}`,
      cx: cx(i),
      connector: null,
    });
  }
  const final = rates[rates.length - 1];
  bars.push({
    key: "total",
    x: cx(count - 1) - barW / 2,
    y: y(final),
    w: barW,
    h: base - y(final),
    tone: "total",
    label: `${final.toFixed(1)}%`,
    labelY: y(final) - 6,
    name: "What-if",
    title: "The plan with every override on",
    cx: cx(count - 1),
    connector: null,
  });
  // Each bar hands its running total to the next.
  for (let i = 0; i < bars.length - 1; i += 1) {
    const level = rates[Math.min(i, rates.length - 1)];
    bars[i].connector = {
      x1: bars[i].x + bars[i].w,
      x2: bars[i + 1].x,
      y: y(level),
    };
  }

  const grid: Array<{ y: number; label: string }> = [];
  const stepPct = hi - lo > 40 ? 20 : hi - lo > 20 ? 10 : 5;
  for (let v = lo; v <= hi + 1e-9; v += stepPct) grid.push({ y: y(v), label: `${v}%` });
  return { bars, grid, base };
}

// ──────────────────────────── the fan ────────────────────────────

export const FAN = { w: 760, h: 220, left: 52, right: 12, top: 10, bottom: 24 } as const;

export interface FanView {
  /** P25–P75, pointwise across iterations. */
  planBand: string;
  /** P50, pointwise across iterations: the centreline. */
  planMedian: string;
  whatIfBand: string;
  whatIfMedian: string;
  xTicks: Array<{ x: number; label: string }>;
  yTicks: Array<{ y: number; label: string; zero: boolean }>;
  /** Vertical rules at each retirement age, where the plan names one. */
  planRetire: number | null;
  whatIfRetire: number | null;
  top: number;
  bottom: number;
}

const f1 = (n: number) => n.toFixed(1);

/**
 * Plan ghosted behind the what-if, today's dollars: the P25–P75 band around a
 * P50 centreline, both pointwise across iterations.
 */
export function fanView(outcome: WhatIfOutcome): FanView | null {
  const axis = outcome.ages ?? outcome.years;
  const n = axis.length;
  if (n < 2) return null;
  const G = FAN;
  const x0 = axis[0];
  const x1 = axis[n - 1];
  const spanX = x1 - x0 || 1;
  const peak = Math.max(
    ...outcome.plan_fan.p50,
    ...outcome.what_if_fan.p50,
    0,
  );
  const top75 = Math.max(...outcome.plan_fan.p75, ...outcome.what_if_fan.p75, 0);
  // Room above the centrelines, but not so much that a wide upper quartile
  // flattens them: the band is clipped at the top instead.
  const ymax = Math.max(1, peak > 0 ? Math.min(top75, peak * 1.4) : top75);
  const x = (v: number) => G.left + ((v - x0) / spanX) * (G.w - G.left - G.right);
  const y = (v: number) =>
    G.h - G.bottom - (clamp(v, 0, ymax) / ymax) * (G.h - G.bottom - G.top);

  const line = (values: number[]) =>
    values.map((v, i) => `${i ? "L" : "M"}${f1(x(axis[i]))} ${f1(y(v))}`).join(" ");
  const band = (hi: number[], lo: number[]) =>
    `${line(hi)} ${lo
      .map((v, i) => `L${f1(x(axis[i]))} ${f1(y(v))}`)
      .reverse()
      .join(" ")} Z`;
  const layers = (fan: WhatIfFan) => ({
    band: band(fan.p75, fan.p25),
    median: line(fan.p50),
  });
  const plan = layers(outcome.plan_fan);
  const whatIf = layers(outcome.what_if_fan);

  const xTicks: FanView["xTicks"] = [];
  const every = spanX > 45 ? 10 : 5;
  for (let t = Math.ceil(x0 / every) * every; t <= x1 + 1e-9; t += every) {
    xTicks.push({ x: x(t), label: String(t) });
  }
  const yTicks = [0, 0.5, 1].map((k) => ({ y: y(ymax * k), label: fmtAxis(ymax * k), zero: k === 0 }));

  const rule = (age: number | null) =>
    age == null || outcome.ages == null || age < x0 || age > x1 ? null : x(age);

  return {
    planBand: plan.band,
    planMedian: plan.median,
    whatIfBand: whatIf.band,
    whatIfMedian: whatIf.median,
    xTicks,
    yTicks,
    planRetire: rule(outcome.plan_retirement_age),
    whatIfRetire: rule(outcome.what_if_retirement_age),
    top: G.top,
    bottom: G.h - G.bottom,
  };
}
