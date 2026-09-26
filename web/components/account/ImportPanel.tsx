"use client";

import { useState } from "react";
import { Button, Field, Input } from "@/components/ui";
import { api } from "@/lib/api/client";
import { scenarioDestination, toHref } from "@/lib/nav/url";
import { http } from "@/lib/api/http";
import type { PlanArchive } from "@/lib/api/generated/PlanArchive";
import type { ArchivePreview } from "@/lib/api/generated/ArchivePreview";
import type { ArchiveImported } from "@/lib/api/generated/ArchiveImported";

export function ImportPanel({ disabled }: { disabled?: boolean }) {
  const [pending, setPending] = useState<{ archive: PlanArchive; preview: ArchivePreview; key: string }>();
  const [busy, setBusy] = useState(false);
  const [prefix, setPrefix] = useState("Restored — ");
  const [error, setError] = useState<string>();
  return <section style={{ marginTop: 24, maxWidth: 720 }}>
    <h3>Restore plan inputs</h3>
    <p>Choose a FinPlan version 2 or 3 JSON input export under 1.9 MB. Imports must pass plan validation. Accounts, parameters, events and referenced assumptions become independent copies; saved runs, reports, billing and unreferenced library items are not restored.</p>
    <input aria-label="Choose FinPlan archive" type="file" accept=".json,application/json" disabled={disabled || busy}
      onChange={async (event) => {
        const file = event.target.files?.[0];
        setPending(undefined); setError(undefined);
        if (!file) return;
        if (file.size > 1_900_000) { setError("Choose an archive smaller than 1.9 MB."); return; }
        setBusy(true);
        try {
          const archive: PlanArchive = JSON.parse(await file.text());
          const preview = await http.post<ArchivePreview>("/archives/preview", archive);
          setPending({ archive, preview, key: crypto.randomUUID() });
        } catch (e) { setError(e instanceof Error ? e.message : "Could not read archive."); }
        finally { setBusy(false); }
      }} />
    {pending && <div>
      <p>{pending.preview.names.join(", ")} · {pending.preview.accounts} accounts · {pending.preview.events} events · {pending.preview.assumptions} assumptions</p>
      <Field label="Imported plan name prefix"><Input aria-label="Imported plan name prefix" maxLength={80} value={prefix} disabled={busy} onChange={event => {
        setPrefix(event.target.value);
        setPending(current => current ? { ...current, key: crypto.randomUUID() } : undefined);
      }} /></Field>
      <p>New names: {pending.preview.names.map(name => `${prefix}${name}`).join(", ")}. Existing plans are never overwritten.</p>
      <Button disabled={busy || disabled} onClick={async () => {
        setBusy(true); setError(undefined);
        try {
          const restored = await http.post<ArchiveImported>("/archives/import", {
            archive: pending.archive, name_prefix: prefix, request_id: pending.key,
          });
          const scenario = await api.scenarios.get(restored.scenario_ids[0]);
          window.location.assign(toHref(scenarioDestination(scenario.slug, "plan")));
        } catch (e) { setError(e instanceof Error ? e.message : "Import failed. You can retry safely."); }
        finally { setBusy(false); }
      }}>{busy ? "Restoring…" : "Restore as new plans"}</Button>
    </div>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
