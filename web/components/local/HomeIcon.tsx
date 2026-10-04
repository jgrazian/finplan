import type { PlanHome } from "@/lib/nav";

/** What each home is called wherever a plan's location is named. */
export const HOME_LABEL: Record<PlanHome, string> = {
  local: "This device",
  cloud: "Cloud",
};

/** The one-line explanation that goes with each label. */
export const HOME_DETAIL: Record<PlanHome, string> = {
  local: "Stored in this browser only",
  cloud: "Saved to your FinPlan account",
};

/**
 * The mark every plan carries for where it lives: a device outline for a plan
 * kept in this browser, a filled cloud for one saved to the account. The
 * outline is quiet and the fill is the accent, so the two read apart at a
 * glance and in greyscale, not by colour alone.
 */
export function HomeIcon({ home, size = 14 }: { home: PlanHome; size?: number }) {
  const common = {
    width: size,
    height: size,
    viewBox: "0 0 16 16",
    "aria-hidden": true,
    focusable: false,
    style: { flex: "none", display: "block" },
  } as const;
  if (home === "local") {
    return (
      <svg
        {...common}
        fill="none"
        stroke="color-mix(in srgb, var(--color-text) 70%, transparent)"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <rect x="3" y="3.5" width="10" height="7" rx="1" />
        <path d="M1.5 12.75h13" />
      </svg>
    );
  }
  return (
    <svg {...common} fill="var(--color-accent)">
      <path d="M4.5 12.75a3 3 0 0 1-.4-5.97 4 4 0 0 1 7.7-1.1 3.4 3.4 0 0 1-.05 7.07z" />
    </svg>
  );
}

/** The mark and its label, as a cell or a line of text. */
export function HomeLabel({ home }: { home: PlanHome }) {
  // Inline text with the mark set into it, not a flex box: a flex box takes
  // its baseline from the icon, which lifts the words above the rest of a
  // table row.
  return (
    <span style={{ whiteSpace: "nowrap" }}>
      <span style={{ display: "inline-block", verticalAlign: "-0.15em", marginRight: 7 }}>
        <HomeIcon home={home} />
      </span>
      {HOME_LABEL[home]}
    </span>
  );
}

/**
 * The label of a button that moves a plan to `to`: the words, then the mark of
 * where it lands, so the action reads as its destination.
 */
export function MoveLabel({ to }: { to: PlanHome }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
      {to === "local" ? "Move to device" : "Move to cloud"}
      <HomeIcon home={to} />
    </span>
  );
}
