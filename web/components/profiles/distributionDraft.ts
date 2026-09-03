/**
 * A distribution being edited, flattened.
 *
 * `DistributionSpec` is a tagged union in fractions; a form is a fixed set of
 * fields in percent. Flattening it is what lets the drawer keep the numbers a
 * kind does not take: switching Normal → Fixed and back leaves the volatility
 * where it was, so a mis-click on the Distribution select costs nothing until
 * Apply. That is only possible because every kind's terms live in one record
 * rather than in the union the server stores.
 */
import type { DistributionSpec, HistoryPreset } from "@/lib/api/types";
import type { DistributionKind } from "@/lib/types";

export interface DistributionDraft {
  kind: DistributionKind;
  /** Fixed's annual rate, in percent. */
  rate: number;
  /** Centre of a parametric kind, in percent — a median for LogNormal. */
  mean: number;
  /** Spread of a parametric kind, in percent. */
  sd: number;
  /** Student-t's scale, in percent; not a standard deviation. */
  scale: number;
  df: number;
  bullMean: number;
  bullSd: number;
  bearMean: number;
  bearSd: number;
  /** Transition chances per year, in percent. */
  bullToBear: number;
  bearToBull: number;
  preset: string;
  /** Years per resampled block; null lets the engine pick. */
  blockSize: number | null;
}

/**
 * What an untouched field holds. These are the numbers a new profile opens on
 * and the ones a kind falls back to when the stored distribution says nothing
 * about it — a Fixed profile switched to Normal has no volatility to carry.
 */
const DEFAULTS: Omit<DistributionDraft, "kind"> = {
  rate: 2,
  mean: 7,
  sd: 15,
  scale: 15,
  df: 5,
  bullMean: 15,
  bullSd: 12,
  bearMean: -8,
  bearSd: 25,
  bullToBear: 18,
  bearToBull: 35,
  preset: "",
  blockSize: null,
};

/** Centre and spread of a nested regime state, where it has them. */
function paramsOf(spec: DistributionSpec): { mean: number; sd: number } | undefined {
  if (spec.kind === "Normal" || spec.kind === "LogNormal") {
    return { mean: spec.mean * 100, sd: spec.std_dev * 100 };
  }
  if (spec.kind === "Fixed") return { mean: spec.rate * 100, sd: 0 };
  return undefined;
}

export function draftOf(spec: DistributionSpec, presets: HistoryPreset[] = []): DistributionDraft {
  const base: DistributionDraft = {
    ...DEFAULTS,
    kind: spec.kind,
    preset: presets[0]?.id ?? DEFAULTS.preset,
  };
  switch (spec.kind) {
    case "None":
      return base;
    case "Fixed":
      return { ...base, rate: spec.rate * 100 };
    case "Normal":
    case "LogNormal":
      return { ...base, mean: spec.mean * 100, sd: spec.std_dev * 100 };
    case "StudentT":
      return { ...base, mean: spec.mean * 100, scale: spec.scale * 100, df: spec.df };
    case "RegimeSwitching": {
      const bull = paramsOf(spec.bull);
      const bear = paramsOf(spec.bear);
      return {
        ...base,
        bullMean: bull?.mean ?? base.bullMean,
        bullSd: bull?.sd ?? base.bullSd,
        bearMean: bear?.mean ?? base.bearMean,
        bearSd: bear?.sd ?? base.bearSd,
        bullToBear: spec.bull_to_bear_prob * 100,
        bearToBull: spec.bear_to_bull_prob * 100,
      };
    }
    case "Bootstrap":
      return { ...base, preset: spec.preset, blockSize: spec.block_size };
  }
}

/** The other direction — only the fields the chosen kind actually takes. */
export function specOf(draft: DistributionDraft): DistributionSpec {
  switch (draft.kind) {
    case "None":
      return { kind: "None" };
    case "Fixed":
      return { kind: "Fixed", rate: draft.rate / 100 };
    case "Normal":
      return { kind: "Normal", mean: draft.mean / 100, std_dev: draft.sd / 100 };
    case "LogNormal":
      return { kind: "LogNormal", mean: draft.mean / 100, std_dev: draft.sd / 100 };
    case "StudentT":
      return {
        kind: "StudentT",
        mean: draft.mean / 100,
        scale: draft.scale / 100,
        df: draft.df,
      };
    case "RegimeSwitching":
      return {
        kind: "RegimeSwitching",
        bull: { kind: "Normal", mean: draft.bullMean / 100, std_dev: draft.bullSd / 100 },
        bear: { kind: "Normal", mean: draft.bearMean / 100, std_dev: draft.bearSd / 100 },
        bull_to_bear_prob: draft.bullToBear / 100,
        bear_to_bull_prob: draft.bearToBull / 100,
      };
    case "Bootstrap":
      return { kind: "Bootstrap", preset: draft.preset, block_size: draft.blockSize };
  }
}

/**
 * What the drawer refuses to send. The server does the real validation, but
 * these three are things it would be right to reject and the form already
 * knows — a negative spread, a df below one, a preset that was never picked.
 */
export function problemWith(draft: DistributionDraft): string | undefined {
  switch (draft.kind) {
    case "Normal":
    case "LogNormal":
      return draft.sd < 0 ? "Volatility cannot be negative." : undefined;
    case "StudentT":
      if (draft.scale < 0) return "Scale cannot be negative.";
      return draft.df < 1 ? "Degrees of freedom must be at least 1." : undefined;
    case "RegimeSwitching":
      return draft.bullSd < 0 || draft.bearSd < 0
        ? "Volatility cannot be negative."
        : undefined;
    case "Bootstrap":
      return draft.preset === "" ? "Pick a historical preset." : undefined;
    default:
      return undefined;
  }
}

/** Whether two drafts would send the same distribution. */
export function sameSpec(a: DistributionDraft, b: DistributionDraft): boolean {
  return JSON.stringify(specOf(a)) === JSON.stringify(specOf(b));
}
