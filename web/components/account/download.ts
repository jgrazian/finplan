import type { Scenario } from "@/lib/api/types";

import { http } from "@/lib/api/http";
import type { PlanArchive } from "@/lib/api/generated/PlanArchive";

/** Consistent server snapshot, including all referenced assumptions. */
export async function collectScenario(scenario: Scenario): Promise<PlanArchive> {
  return http.get<PlanArchive>(`/scenarios/${scenario.id}/archive`);
}

/** Hand the browser a file. Nothing leaves the machine. */
export function download(filename: string, payload: unknown): void {
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}

/** `retirement-base` → `retirement-base-2026-08-30.json`. */
export function fileNameFor(name: string): string {
  const slug = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  return `${slug || "scenario"}-${new Date().toISOString().slice(0, 10)}.json`;
}
