import type { Scenario, Results } from "../api/types";
import type { RunInputs } from "../api/generated/RunInputs";

/** Historical axes must never use the current plan's birth date or horizon. */
export function snapshotScenario(inputs: RunInputs, raw: Results): Scenario {
  const graph = inputs.snapshot as { scenario?: Partial<Scenario> } | null;
  const source = graph?.scenario;
  const dates = raw.bands.flatMap((band) => band.dates).sort();
  return {
    id: raw.scenario_id, name: source?.name ?? "Historical plan (inputs unavailable)",
    description: source?.description ?? null,
    start_date: source?.start_date ?? dates[0] ?? "2000-01-01",
    birth_date: source?.birth_date ?? null,
    duration_years: source?.duration_years ?? Math.max(1, Number(dates.at(-1)?.slice(0, 4)) - Number(dates[0]?.slice(0, 4))),
    inflation_profile_id: source?.inflation_profile_id ?? null,
    tax_config_id: source?.tax_config_id ?? null,
    collect_ledger: Boolean(source?.collect_ledger),
    created_at: source?.created_at ?? "", updated_at: source?.updated_at ?? "",
    last_run_at: null, last_success_rate: null,
  };
}

export function comparisonWarnings(left: RunInputs, right: RunInputs): string[] {
  const a = (left.snapshot as {scenario?: Partial<Scenario>} | null)?.scenario;
  const b = (right.snapshot as {scenario?: Partial<Scenario>} | null)?.scenario;
  const warnings: string[] = [];
  if (!a || !b) warnings.push("Historical inputs are unavailable; assumption differences cannot be verified.");
  if (a && b && (a.start_date !== b.start_date || a.duration_years !== b.duration_years)) warnings.push("Plan dates or horizons differ; terminal balances describe different periods.");
  if (left.seed != null && right.seed != null && left.seed !== right.seed) warnings.push("Seeds differ; differences include Monte Carlo sampling variation.");
  if (left.model_version && right.model_version && left.model_version !== right.model_version) warnings.push("Model versions differ.");
  if (left.iterations !== right.iterations || left.converge !== right.converge) warnings.push("Sampling policies differ; compare the actual completed iteration counts.");
  return warnings;
}

/** Compare saved definitions, never the editable workspace. */
export function changedInputSections(left: RunInputs, right: RunInputs): string[] {
  if (!left.snapshot || !right.snapshot) return [];
  const a=left.snapshot as Record<string,unknown>, b=right.snapshot as Record<string,unknown>;
  const sections: Record<string,string> = {scenario:"Plan dates and household",accounts:"Account definitions",bank:"Cash balances and returns",investment:"Investment account settings",positions:"Holdings and cost basis",assets:"Asset prices and mappings",events:"Events",triggers:"Event schedules",effects:"Event effects",amounts:"Income and spending amounts",return_profiles:"Return profiles",distributions:"Market and inflation distributions",tax_config:"Tax assumptions",tax_brackets:"Tax brackets"};
  return Object.entries(sections).filter(([key])=>JSON.stringify(a[key]) !== JSON.stringify(b[key])).map(([,label])=>label);
}
