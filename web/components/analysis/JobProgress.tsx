"use client";

import { Button } from "@/components/ui";
import { fmtInt } from "@/lib/format";
import type { Analysis } from "@/lib/api/types";

/**
 * An analysis in flight.
 *
 * The bar is drawn against simulations rather than grid points, because a
 * sweep's points finish at wildly different speeds and a bar that jumps a sixth
 * at a time says nothing while it is between jumps.
 */
export function JobProgress({
  job,
  onCancel,
  label,
}: {
  job: Analysis | undefined;
  onCancel: () => void;
  /** What is happening — "Sweeping", "Solving", "Ranking". */
  label: string;
}) {
  const done = job?.completed ?? 0;
  const total = job?.total ?? 0;
  const percent = total > 0 ? Math.min(100, (done / total) * 100) : 0;

  return (
    <div style={{ padding: "40px 24px", maxWidth: 520 }}>
      <h4 style={{ margin: "0 0 8px" }}>
        {job?.status === "queued" ? "Queued…" : `${label}…`}
      </h4>
      <div style={{ display: "flex", height: 10, border: "1px solid var(--color-divider)" }}>
        <div style={{ width: `${percent}%`, background: "var(--color-accent)" }} />
      </div>
      <p
        style={{
          margin: "8px 0 14px",
          fontSize: 12,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        {fmtInt(done)} of about {fmtInt(total)} simulations
      </p>
      <Button onClick={onCancel}>Cancel</Button>
    </div>
  );
}
