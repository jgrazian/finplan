"use client";

import { useState } from "react";
import { Blueprint, Button, Hr, Table, Td, Th } from "@/components/ui";
import { HOME_LABEL, HomeIcon, HomeLabel, MoveLabel } from "@/components/local/HomeIcon";
import { LocalImport } from "@/components/local/LocalPlansPanel";
import type { PlanArchive } from "@/lib/api/generated/PlanArchive";
import type { Scenario, UserResponse } from "@/lib/api/types";
import { fmtPercent } from "@/lib/format";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { getLocalRuntime } from "@/lib/local/runtime";
import { type PlanHome, homeOf, planApiFor } from "@/lib/nav";
import { PanelNote } from "./chrome";
import { DeleteScenarioDialog } from "@/components/scenario/DeleteScenarioDialog";
import { DeleteAccountDialog } from "./DeleteAccountDialog";
import { download, fileNameFor } from "./download";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";
/** An action cell: as narrow as its button, which starts at the column's edge. */
const ACTION = { width: "1%", whiteSpace: "nowrap", paddingInline: 2 } as const;

/** One row of the list, whichever home it is in. */
interface Row {
  home: PlanHome;
  scenario: Scenario;
}

/**
 * Everything the account holds, and the way out of it.
 *
 * With local mode on, the plans on this device and those in the cloud are one
 * list, each row marked with where it lives and offering the move to the
 * other home. The success figure comes down on the scenario row itself rather
 * than from a results fetch per scenario: this is a list, and a list should
 * cost one request. A scenario that has never finished a run says so instead
 * of borrowing a number from a run that failed.
 */
export function DataPanel({
  user,
  scenarios,
  onDeleted,
  onScenarioDeleted,
  readOnly,
  local,
}: {
  user: UserResponse;
  /** The cloud's plans. */
  scenarios: Scenario[];
  onDeleted: () => void;
  /** One cloud scenario was deleted from the list. */
  onScenarioDeleted: (id: number) => void;
  /** No connection: exports still work, deletion cannot be offered. */
  readOnly?: boolean;
  /**
   * Plans kept on this device, when local mode is on: listed with the cloud's,
   * and the way a plan changes home from here.
   */
  local?: {
    plans: Scenario[];
    onMoveToCloud: (plan: Scenario) => void;
    onDownload: (plan: Scenario) => void;
    onImported: (scenarioIds: number[]) => void;
    /** One plan on this device was deleted. */
    onDeleted: (id: number) => void;
  };
}) {
  const [deleting, setDeleting] = useState(false);
  const [deletingRow, setDeletingRow] = useState<Row>();
  const [exporting, setExporting] = useState<string>();
  const exportJob = useSubmit();

  // This device first, as the scenario menu lists them.
  const rows: Row[] = [
    ...(local?.plans ?? []).map((scenario) => ({ home: "local" as const, scenario })),
    ...scenarios.map((scenario) => ({ home: homeOf(scenario.slug), scenario })),
  ];

  const exportOne = ({ home, scenario }: Row) => {
    setExporting(scenario.slug);
    exportJob.run(
      async () => {
        download(fileNameFor(scenario.name), await planApiFor(home).scenarios.archive(scenario.id));
        // An export is the backup the reminder asks for.
        if (home === "local") await getLocalRuntime()?.markExported([scenario.id]);
      },
      () => setExporting(undefined),
    );
  };

  // One file for every plan in both homes: the archive format has no notion of
  // where a plan lived, and importing it puts each one on this device.
  const exportAll = () => {
    setExporting("all");
    exportJob.run(
      async () => {
        const parts: PlanArchive[] = [];
        if (scenarios.length > 0) parts.push(await planApiFor("cloud").archives.exportAll());
        if (local && local.plans.length > 0) parts.push(await planApiFor("local").archives.exportAll());
        const [first] = parts;
        if (!first) return;
        download(`finplan-${new Date().toISOString().slice(0, 10)}.json`, {
          ...first,
          plans: parts.flatMap((part) => part.plans),
        });
        if (local && local.plans.length > 0) {
          await getLocalRuntime()?.markExported(local.plans.map((plan) => plan.id));
        }
      },
      () => setExporting(undefined),
    );
  };

  return (
    <div>
      {local && (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(240px, 1fr))",
            gap: 12,
            maxWidth: 720,
            marginBottom: 18,
          }}
        >
          <HomeSummary
            home="local"
            count={local.plans.length}
            where="stored in this browser"
            note="Clearing this site's data deletes these. Export JSON or move them to the cloud to keep a backup."
          />
          <HomeSummary
            home="cloud"
            count={scenarios.length}
            where={user.email}
            note="Saved to your FinPlan account: open them from any device you sign in on."
          />
        </div>
      )}

      {rows.length === 0 ? (
        <p style={{ fontSize: 12, color: MUTED }}>No scenarios yet.</p>
      ) : (
        <div style={{ maxWidth: 820 }}>
          <Table>
            <thead>
              <tr>
                <Th>Scenario</Th>
                {local && <Th>Stored</Th>}
                <Th>Last run</Th>
                <Th align="right" title="Fraction of runs ending with positive net worth; not a funding check.">Positive at end</Th>
                <Th aria-label="Export" />
                {local && <Th aria-label="Move" />}
                <Th aria-label="Delete" />
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const { home, scenario } = row;
                return (
                  <tr key={scenario.slug}>
                    <Td>{scenario.name}</Td>
                    {local && (
                      <Td>
                        <HomeLabel home={home} />
                      </Td>
                    )}
                    <Td muted>{scenario.last_run_at?.slice(0, 10) ?? "never"}</Td>
                    <Td align="right" muted={scenario.last_success_rate == null}>
                      {scenario.last_success_rate == null
                        ? "—"
                        : fmtPercent(scenario.last_success_rate)}
                    </Td>
                    <Td style={ACTION}>
                      <Button
                        variant="ghost"
                        disabled={exportJob.busy}
                        onClick={() => exportOne(row)}
                      >
                        {exporting === scenario.slug ? "…" : "Export JSON"}
                      </Button>
                    </Td>
                    {local && (
                      <Td style={ACTION}>
                        {home === "local" ? (
                          <Button
                            variant="ghost"
                            disabled={readOnly}
                            title={readOnly ? "No connection to the server." : undefined}
                            onClick={() => local.onMoveToCloud(scenario)}
                          >
                            <MoveLabel to="cloud" />
                          </Button>
                        ) : (
                          <Button variant="ghost" onClick={() => local.onDownload(scenario)}>
                            <MoveLabel to="local" />
                          </Button>
                        )}
                      </Td>
                    )}
                    <Td style={ACTION}>
                      <Button
                        variant="danger"
                        // A plan on this device is deleted here, server or no server.
                        disabled={readOnly && home === "cloud"}
                        title={readOnly && home === "cloud" ? "No connection to the server." : undefined}
                        onClick={() => setDeletingRow(row)}
                      >
                        Delete
                      </Button>
                    </Td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        </div>
      )}

      <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 14 }}>
        <Button disabled={exportJob.busy || rows.length === 0} onClick={exportAll}>
          {exporting === "all" ? "…" : "Export all"}
        </Button>
        {exportJob.error && (
          <span style={{ fontSize: 12, color: "var(--color-accent-700)" }}>{exportJob.error}</span>
        )}
      </div>

      <PanelNote>
        Download complete plan inputs, including positions, events, return and inflation profiles, and tax definitions. These archives exclude run results, login credentials, and billing records.
      </PanelNote>

      {local && <LocalImport onImported={local.onImported} readOnly={false} />}
      <Hr />

      <Blueprint style={{ padding: "12px 14px", maxWidth: 600 }}>
        <div style={{ fontFamily: "var(--font-heading)", fontWeight: 600, fontSize: 15 }}>
          Delete account
        </div>
        <p style={{ fontSize: 12, margin: "4px 0 10px", lineHeight: 1.55, color: MUTED }}>
          Removes every scenario, cached run and session. Export first — this
          cannot be undone.
        </p>
        <Button
          disabled={readOnly}
          title={readOnly ? "No connection to the server." : undefined}
          onClick={() => setDeleting(true)}
        >
          Delete account…
        </Button>
      </Blueprint>

      {deletingRow && (
        <DeleteScenarioDialog
          scenario={deletingRow.scenario}
          home={deletingRow.home}
          onClose={() => setDeletingRow(undefined)}
          onDeleted={(id) => {
            const { home } = deletingRow;
            setDeletingRow(undefined);
            if (home === "local") local?.onDeleted(id);
            else onScenarioDeleted(id);
          }}
        />
      )}

      {deleting && (
        <DeleteAccountDialog
          email={user.email}
          scenarioCount={scenarios.length}
          onClose={() => setDeleting(false)}
          onDeleted={onDeleted}
        />
      )}
    </div>
  );
}

/** One home's card above the lists: its mark, how many plans it holds, and what that means. */
function HomeSummary({
  home,
  count,
  where,
  note,
}: {
  home: "local" | "cloud";
  count: number;
  where: string;
  note: string;
}) {
  return (
    <Blueprint style={{ padding: "12px 14px", display: "flex", flexDirection: "column", gap: 2 }}>
      <span style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 600 }}>
        <HomeIcon home={home} />
        {HOME_LABEL[home]}
      </span>
      <span style={{ fontSize: 13 }}>
        {count} {count === 1 ? "scenario" : "scenarios"} · {where}
      </span>
      <span style={{ fontSize: 12, lineHeight: 1.5, color: MUTED }}>{note}</span>
    </Blueprint>
  );
}
