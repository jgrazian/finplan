import { Blueprint, SectionHeading } from "@/components/ui";
import type { SimulationWarning } from "@/lib/types";

/** The rail's warnings, counted in the heading as the canvas has them. */
export function WarningList({ warnings }: { warnings: SimulationWarning[] }) {
  return (
    <div>
      <SectionHeading className="mb-[6px]">
        Warnings{" "}
        {warnings.length > 0 && (
          <span className="text-muted" style={{ letterSpacing: 0 }}>
            {warnings.length}
          </span>
        )}
      </SectionHeading>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {warnings.map((w) => (
          <Blueprint key={w.id} style={{ padding: "9px 11px", fontSize: 12 }}>
            <strong style={{ fontFamily: "var(--font-heading)" }}>{w.title}</strong> — {w.detail}
          </Blueprint>
        ))}
        {warnings.length === 0 && (
          <p className="text-muted" style={{ fontSize: 12, margin: 0 }}>
            No warnings recorded for this selected path.
          </p>
        )}
      </div>
    </div>
  );
}
