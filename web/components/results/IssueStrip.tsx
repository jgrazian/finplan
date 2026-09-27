import type { CSSProperties } from "react";
import type { IssueStrip as Strip, StripTone } from "@/lib/view/issues";

/**
 * The Results screen's one-line issue summary (design 14a).
 *
 * Fill weight carries severity, as the status bar's does: a failed run is
 * paper type on an ink field, a changed plan is an accent tint with a rule,
 * and a clean run with shortfall paths is only an outline.
 */
export function IssueStrip({
  strip,
  open,
  onToggle,
}: {
  strip: Strip;
  open: boolean;
  onToggle: () => void;
}) {
  const tone = TONES[strip.tone];
  return (
    <div
      className="issue-strip"
      role={strip.tone === "failed" ? "alert" : "status"}
      style={{
        display: "flex",
        alignItems: "center",
        flexWrap: "wrap",
        gap: "6px 18px",
        padding: "10px 18px",
        fontSize: 13,
        borderBottom: "1px solid var(--color-divider)",
        ...tone,
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-heading)",
          fontWeight: 600,
          letterSpacing: ".06em",
          textTransform: "uppercase",
          color: strip.tone === "clean" ? "var(--color-accent-800)" : undefined,
        }}
      >
        {strip.label}
      </span>
      <span style={{ minWidth: 0, textWrap: "pretty" }}>{strip.message}</span>
      {strip.aside && (
        <>
          <span
            aria-hidden="true"
            style={{ width: 1, alignSelf: "stretch", background: "var(--color-accent-300)" }}
          />
          <span>{strip.aside}</span>
        </>
      )}
      <button
        className="sbtn"
        type="button"
        aria-expanded={open}
        aria-controls="results-issues"
        style={{ marginLeft: "auto" }}
        onClick={onToggle}
      >
        {open ? "Hide issues" : strip.tone === "clean" ? "Why" : "Open issues"}
      </button>
    </div>
  );
}

const TONES: Record<StripTone, CSSProperties> = {
  failed: { background: "var(--color-accent-900)", color: "var(--color-bg)" },
  changed: {
    background: "var(--color-accent-100)",
    color: "var(--color-accent-900)",
    borderLeft: "4px solid var(--color-accent-700)",
  },
  clean: {},
};
