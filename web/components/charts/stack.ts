import type { AccountSeries } from "@/lib/types";
import type { ScaleKind } from "./geometry";

export interface StackBand {
  color: string;
  /** Edges are always ordered, including below-zero bands. */
  top: number[];
  bottom: number[];
}

/** Positive accounts and debt accumulate separately, never cancel visually. */
export function stackSeries(series: AccountSeries[], count: number) {
  const positive = new Array<number>(count).fill(0);
  const negative = new Array<number>(count).fill(0);
  const bands: StackBand[] = [];
  for (const account of series) {
    const above: StackBand = { color: account.color, bottom: [...positive], top: [] };
    const below: StackBand = { color: account.color, top: [...negative], bottom: [] };
    for (let i = 0; i < count; i++) {
      const value = account.values[i] ?? 0;
      positive[i] += Math.max(0, value);
      negative[i] += Math.min(0, value);
      above.top.push(positive[i]);
      below.bottom.push(negative[i]);
    }
    // Separate paths also handle accounts that cross zero during the plan.
    if (above.top.some((v, i) => v !== above.bottom[i])) bands.push(above);
    if (below.bottom.some((v, i) => v !== below.top[i])) bands.push(below);
  }
  return { bands, positive, negative, total: positive.map((v, i) => v + negative[i]) };
}

/** Never use log for nonpositive outcomes or proportional account stacks. */
export function resolveScaleKind(
  requested: ScaleKind,
  view: "fan" | "stack" | "bar",
  paths: number[][],
): { kind: ScaleKind; reason?: string } {
  const reason = view !== "fan"
    ? "Account composition uses a linear scale: positive balances above zero, debt below; the line shows net worth."
    : paths.some((values) => values.some((v) => v <= 0 || !Number.isFinite(v)))
      ? "Log scale is unavailable because a plotted path includes zero, negative, or unavailable values. Linear scale preserves these outcomes."
      : undefined;
  return { kind: reason ? "linear" : requested, reason };
}
