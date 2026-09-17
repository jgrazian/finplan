"use client";

import { http } from "@/lib/api/http";
import type { PlanArchive } from "@/lib/api/generated/PlanArchive";
import { ImportPanel } from "./ImportPanel";
import { useState } from "react";
import { Blueprint, Button, Hr, Table, Td, Th } from "@/components/ui";
import type { Scenario, UserResponse } from "@/lib/api/types";
import { fmtPercent } from "@/lib/format";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { PanelNote } from "./chrome";
import { DeleteAccountDialog } from "./DeleteAccountDialog";
import { collectScenario, download, fileNameFor } from "./download";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/**
 * Everything the account holds, and the way out of it.
 *
 * The success figure comes down on the scenario row itself rather than from a
 * results fetch per scenario: this is a list, and a list should cost one
 * request. A scenario that has never finished a run says so instead of
 * borrowing a number from a run that failed.
 */
export function DataPanel({
  user,
  scenarios,
  onDeleted,
  readOnly,
}: {
  user: UserResponse;
  scenarios: Scenario[];
  onDeleted: () => void;
  /** No connection: exports still work, deletion cannot be offered. */
  readOnly?: boolean;
}) {
  const [deleting, setDeleting] = useState(false);
  const [exporting, setExporting] = useState<number | "all">();
  const exportJob = useSubmit();

  const exportOne = (scenario: Scenario) => {
    setExporting(scenario.id);
    exportJob.run(
      async () => download(fileNameFor(scenario.name), await collectScenario(scenario)),
      () => setExporting(undefined),
    );
  };

  const exportAll = () => {
    setExporting("all");
    exportJob.run(
      async () =>
        download(`finplan-${new Date().toISOString().slice(0, 10)}.json`, await http.get<PlanArchive>("/archives")),
      () => setExporting(undefined),
    );
  };

  return (
    <div>
      {scenarios.length === 0 ? (
        <p style={{ fontSize: 12, color: MUTED }}>No scenarios yet.</p>
      ) : (
        <div style={{ maxWidth: 720 }}>
          <Table>
            <thead>
              <tr>
                <Th>Scenario</Th>
                <Th>Last run</Th>
                <Th align="right" title="Fraction of runs ending with positive net worth; not a funding check.">Positive at end</Th>
                <Th align="right" />
              </tr>
            </thead>
            <tbody>
              {scenarios.map((scenario) => (
                <tr key={scenario.id}>
                  <Td>{scenario.name}</Td>
                  <Td muted>{scenario.last_run_at?.slice(0, 10) ?? "never"}</Td>
                  <Td align="right" muted={scenario.last_success_rate == null}>
                    {scenario.last_success_rate == null
                      ? "—"
                      : fmtPercent(scenario.last_success_rate)}
                  </Td>
                  <Td align="right">
                    <Button
                      variant="ghost"
                      disabled={exportJob.busy}
                      onClick={() => exportOne(scenario)}
                    >
                      {exporting === scenario.id ? "…" : "Export JSON"}
                    </Button>
                  </Td>
                </tr>
              ))}
            </tbody>
          </Table>
        </div>
      )}

      <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 14 }}>
        <Button disabled={exportJob.busy || scenarios.length === 0} onClick={exportAll}>
          {exporting === "all" ? "…" : "Export all"}
        </Button>
        {exportJob.error && (
          <span style={{ fontSize: 12, color: "var(--color-accent-700)" }}>{exportJob.error}</span>
        )}
      </div>

      <PanelNote>
        Download complete plan inputs, including positions, events, return and inflation profiles, and tax definitions. Import restores independent copies. These archives exclude run results, login credentials, and billing records.
      </PanelNote>

      <ImportPanel disabled={readOnly} />
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
