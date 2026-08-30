import type { InflationProfile, ReturnProfile } from "@/lib/types";

export const MOCK_RETURN_PROFILES: ReturnProfile[] = [
  { id: "us-equity-hist", kind: "Bootstrap", source: "US total market · 1928–2024", mean: 10.2, sd: 18.6, usedBy: ["VTI", "AVUV"] },
  { id: "us-60-40", kind: "Normal", source: "60/40 blend", mean: 7.1, sd: 11.4, usedBy: ["401(k) blend"] },
  { id: "intl-equity", kind: "LogNormal", source: "MSCI ex-US · 1970–2024", mean: 8.4, sd: 20.1, usedBy: ["VXUS"] },
  { id: "bonds-agg", kind: "Normal", source: "Aggregate bond", mean: 4.3, sd: 5.2, usedBy: ["BND"] },
  { id: "cash-4pct", kind: "Fixed", source: "constant", mean: 4.0, sd: 0, usedBy: ["HYSA"] },
  { id: "housing-hist", kind: "Historical", source: "Case–Shiller · 1987–2024", mean: 4.6, sd: 6.9, usedBy: ["Home"] },
  { id: "fixed-6.1pct", kind: "Fixed", source: "mortgage rate", mean: 6.1, sd: 0, usedBy: ["Mortgage"] },
];

export const MOCK_INFLATION_PROFILES: InflationProfile[] = [
  { id: "us-cpi-hist", kind: "Bootstrap", mean: 3.2, sd: 2.4, note: "CPI-U · 1928–2024" },
  { id: "fixed-2.5pct", kind: "Fixed", mean: 2.5, sd: 0, note: "planning assumption" },
  { id: "inflation-stress", kind: "Normal", mean: 4.5, sd: 3.0, note: "stress variant" },
];

/** Historical presets are read-only; duplicating one makes it editable. */
export function isReadOnlyPreset(profile: ReturnProfile): boolean {
  return profile.kind === "Historical" || profile.kind === "Bootstrap";
}
