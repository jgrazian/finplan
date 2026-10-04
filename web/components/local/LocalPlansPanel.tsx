"use client";

import { useState } from "react";
import { PanelNote } from "@/components/account/chrome";
import { download } from "@/components/account/download";
import { Button, Table, Td, Th } from "@/components/ui";
import type { ArchivePreview } from "@/lib/api/generated/ArchivePreview";
import type { PlanArchive } from "@/lib/api/generated/PlanArchive";
import { localApi } from "@/lib/api/local";
import type { Scenario } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import {
  exportLocal,
  importIntoDevice,
  parseArchive,
  previewLocalArchive,
} from "@/lib/local/homes";
import { getLocalRuntime } from "@/lib/local/runtime";

/** The largest archive the import takes, as on the cloud side. */
const MAX_ARCHIVE_BYTES = 1_900_000;

/**
 * The plans kept in this browser: export one or all as a file, import a file
 * back, and move a plan to the cloud. It is the whole of the no-account backup
 * story, so it works with nothing but the local store behind it.
 */
export function LocalPlansPanel({
  plans,
  onMoveToCloud,
  onImported,
  onExported,
  readOnly,
}: {
  /** Local plans only. */
  plans: Scenario[];
  /** Offered when the person can move a plan to an account; omitted otherwise. */
  onMoveToCloud?: (plan: Scenario) => void;
  /** Plans were imported into the local store; the list is stale. */
  onImported: (scenarioIds: number[]) => void;
  /** A plan was exported; its backup reminder is stale. */
  onExported?: () => void;
  readOnly?: boolean;
}) {
  const exportJob = useSubmit();
  const [exporting, setExporting] = useState<number | "all">();
  const [pending, setPending] = useState<{
    archive: PlanArchive;
    preview?: ArchivePreview;
    key: string;
  }>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const runExport = (target: number | "all", action: () => Promise<void>) => {
    setExporting(target);
    exportJob.run(action, () => {
      setExporting(undefined);
      onExported?.();
    });
  };

  const exportOne = (plan: Scenario) =>
    runExport(plan.id, () =>
      exportLocal(
        { local: localApi },
        getLocalRuntime(),
        { kind: "one", id: plan.id, name: plan.name },
        download,
      ),
    );

  const exportAll = () =>
    runExport("all", () =>
      exportLocal(
        { local: localApi },
        getLocalRuntime(),
        { kind: "all", ids: plans.map((plan) => plan.id) },
        download,
      ),
    );

  const chooseFile = async (file: File | undefined) => {
    setPending(undefined);
    setError(undefined);
    if (!file) return;
    if (file.size > MAX_ARCHIVE_BYTES) {
      setError("Choose an archive smaller than 1.9 MB.");
      return;
    }
    setBusy(true);
    try {
      const archive = parseArchive(await file.text());
      const preview = await previewLocalArchive({ local: localApi }, archive);
      setPending({ archive, preview, key: crypto.randomUUID() });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not read the file.");
    } finally {
      setBusy(false);
    }
  };

  const runImport = async () => {
    if (!pending) return;
    setBusy(true);
    setError(undefined);
    try {
      const ids = await importIntoDevice({ local: localApi }, pending.archive, pending.key);
      setPending(undefined);
      onImported(ids);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Import failed. You can retry safely.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      {plans.length === 0 ? (
        <p style={{ fontSize: 12 }} className="ns-mut">
          No plans on this device yet.
        </p>
      ) : (
        <div style={{ maxWidth: 720 }}>
          <Table>
            <thead>
              <tr>
                <Th>Plan</Th>
                <Th>Last changed</Th>
                <Th align="right" />
              </tr>
            </thead>
            <tbody>
              {plans.map((plan) => (
                <tr key={plan.slug}>
                  <Td>{plan.name}</Td>
                  <Td muted>{plan.updated_at.slice(0, 10)}</Td>
                  <Td align="right">
                    <Button
                      variant="ghost"
                      disabled={exportJob.busy}
                      onClick={() => exportOne(plan)}
                    >
                      {exporting === plan.id ? "…" : "Export JSON"}
                    </Button>
                    {onMoveToCloud && (
                      <Button
                        variant="ghost"
                        disabled={readOnly}
                        title={readOnly ? "No connection to the server." : undefined}
                        onClick={() => onMoveToCloud(plan)}
                      >
                        Move to cloud…
                      </Button>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </Table>
        </div>
      )}

      <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 14 }}>
        <Button disabled={exportJob.busy || plans.length === 0} onClick={exportAll}>
          {exporting === "all" ? "…" : "Export all"}
        </Button>
        {exportJob.error && (
          <span style={{ fontSize: 12, color: "var(--color-accent-700)" }}>{exportJob.error}</span>
        )}
      </div>
      <PanelNote>
        The file holds the plan&rsquo;s inputs only, with no run results. It is the backup for
        plans kept on this device: nothing is sent anywhere.
      </PanelNote>

      <div style={{ marginTop: 18 }}>
        <h4 style={{ margin: "0 0 6px" }}>Import a file</h4>
        <input
          aria-label="Choose a FinPlan archive to import to this device"
          type="file"
          accept=".json,application/json"
          disabled={busy || readOnly}
          onChange={(event) => {
            void chooseFile(event.target.files?.[0]);
            // So choosing the same file again after a fix is noticed.
            event.target.value = "";
          }}
        />
        {pending && (
          <div style={{ marginTop: 10 }}>
            {pending.preview ? (
              <p style={{ fontSize: 12.5, margin: "0 0 8px" }}>
                {pending.preview.names.join(", ")} · {pending.preview.accounts} accounts ·{" "}
                {pending.preview.events} events
              </p>
            ) : (
              <p style={{ fontSize: 12.5, margin: "0 0 8px" }}>
                {pending.archive.plans.length}{" "}
                {pending.archive.plans.length === 1 ? "plan" : "plans"} in this file.
              </p>
            )}
            <p className="ns-mut" style={{ fontSize: 12, margin: "0 0 8px" }}>
              Imported plans are added as new plans on this device; none is overwritten.
            </p>
            <Button variant="primary" disabled={busy} onClick={() => void runImport()}>
              {busy ? "Importing…" : "Import to this device"}
            </Button>
          </div>
        )}
        {error && (
          <p role="alert" style={{ fontSize: 12, color: "var(--color-accent-700)" }}>
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
