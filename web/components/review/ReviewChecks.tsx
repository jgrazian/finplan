"use client";

import type { ChecksView } from "@/lib/view/review";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/**
 * "What Review checks", collapsed under the board: every standard check by
 * layer, and what each came to in this review, so a rule that looked and
 * found nothing reads differently from a check no rule makes.
 */
export function ReviewChecks({ view }: { view: ChecksView }) {
  return (
    <details style={{ borderTop: "1px solid var(--color-divider)" }}>
      <summary style={{ padding: "10px 20px", cursor: "pointer", fontSize: 13, color: MUTED }}>
        <span style={{ fontWeight: 600, color: "var(--color-text)" }}>What Review checks</span>
        {" · "}
        {view.summary}
      </summary>
      <div style={{ display: "flex", flexDirection: "column", gap: 18, padding: "4px 20px 18px", maxWidth: 860 }}>
        {view.groups.map((group) => (
          <section key={group.layer} aria-label={group.heading}>
            <div
              style={{
                fontSize: 10.5,
                fontWeight: 600,
                letterSpacing: ".06em",
                textTransform: "uppercase",
                color: MUTED,
                paddingBottom: 4,
              }}
            >
              {group.heading}
            </div>
            <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
              {group.rows.map((row) => (
                <li
                  key={row.id}
                  style={{
                    display: "grid",
                    gridTemplateColumns: "minmax(0, 1fr) auto",
                    gap: "2px 16px",
                    padding: "8px 0",
                    borderBottom: "1px solid var(--color-divider)",
                  }}
                >
                  <span style={{ fontSize: 13, lineHeight: 1.45 }}>{row.text}</span>
                  <span
                    style={{
                      fontSize: 12,
                      whiteSpace: "nowrap",
                      fontWeight: row.found ? 600 : undefined,
                      color: row.found ? "var(--color-accent-700)" : MUTED,
                    }}
                  >
                    {row.state}
                  </span>
                  <span style={{ gridColumn: "1 / -1", fontSize: 11.5, lineHeight: 1.45, color: MUTED }}>
                    {row.meta}
                    {row.offers && ` · Offers: ${row.offers}`}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </details>
  );
}
