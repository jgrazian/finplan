import { api } from "@/lib/api/client";
import type { Scenario } from "@/lib/api/types";

/**
 * One scenario, as a file.
 *
 * Assembled from the same endpoints the app reads rather than from an export
 * route, so what is written is exactly what the screens are showing — there is
 * no second serializer to drift. JSON, not YAML: nothing in the server speaks
 * YAML, and a file whose extension promises the CLI's format while holding
 * something else would be worse than an honest one.
 */
export interface ScenarioExport {
  format: "finplan.scenario";
  version: 1;
  exported_at: string;
  scenario: Scenario;
  assets: unknown[];
  accounts: unknown[];
  events: unknown[];
}

export async function collectScenario(scenario: Scenario): Promise<ScenarioExport> {
  const [assets, accounts, events] = await Promise.all([
    api.assets.list(scenario.id),
    api.accounts.list(scenario.id),
    api.events.list(scenario.id),
  ]);

  // Lots hang off an account rather than coming back with it, so they are
  // fetched per account — an export that dropped them would lose cost basis.
  const withPositions = await Promise.all(
    accounts.map(async (account) => ({
      ...account,
      positions: await api.accounts.positions(scenario.id, account.id),
    })),
  );

  return {
    format: "finplan.scenario",
    version: 1,
    exported_at: new Date().toISOString(),
    scenario,
    assets,
    accounts: withPositions,
    events,
  };
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
