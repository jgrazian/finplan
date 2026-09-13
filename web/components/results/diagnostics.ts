import type { SimulationWarning } from "../../lib/types.ts";

/** Select observations by their stored kind, never by interpreting warning prose. */
export function fundingDiagnostics(warnings: SimulationWarning[]) {
  return {
    shortfalls: warnings.filter((warning) => warning.kind === "CashShortfall"),
    processing: warnings.filter((warning) =>
      warning.kind === "EffectSkipped" || warning.kind === "EvaluationFailed"),
  };
}
