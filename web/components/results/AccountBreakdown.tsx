import { Blueprint, StatLabel } from "@/components/ui";
import { fmtCompactOrExact } from "@/lib/format";
import type { AccountBreakdown as Breakdown, AccountStanding } from "@/lib/view/results";

const MUTED = "color-mix(in srgb, var(--color-text) 50%, transparent)";
const TRACK = "color-mix(in srgb, var(--color-text) 8%, transparent)";

/**
 * A debt is not a smaller asset, so it is not drawn as one: a hatched bar reads
 * as a hole in the portfolio at a glance, where a solid bar of the same length
 * would read as another position holding it up.
 */
const HATCH = "repeating-linear-gradient(135deg, transparent 0 3px, var(--color-accent-700) 3px 4px)";

/**
 * The rail's account breakdown: the chart's year cut across the accounts.
 *
 * The chart says what the plan is worth; this says what that figure is made of
 * and what moved since the year before it. It keeps a permanent slot at the top
 * of the rail rather than appearing on hover, so the panels under it never jump
 * as the pointer crosses the plot.
 */
export function AccountBreakdown({
  breakdown,
  year,
  age,
  hint,
  pinned,
  onUnpin,
}: {
  breakdown: Breakdown;
  year: number;
  /** The plan's age at that year, or undefined without a birth date to count from. */
  age: number | undefined;
  /** Where the year came from — hover, pin, or the end of the plan. */
  hint: string;
  pinned: boolean;
  onUnpin: () => void;
}) {
  return (
    <div>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 8,
          marginBottom: 8,
        }}
      >
        <h6 style={{ margin: 0 }}>Accounts</h6>
        <span
          style={{
            fontSize: 11,
            fontFamily: "ui-monospace, Menlo, monospace",
            color: "var(--color-accent-800)",
          }}
        >
          end of {year}
          {age != null && ` · age ${age}`}
        </span>
        {pinned && (
          <button
            type="button"
            className="sbtn"
            style={{ marginLeft: "auto", padding: "2px 7px" }}
            onClick={onUnpin}
          >
            Unpin
          </button>
        )}
      </div>

      <Blueprint style={{ padding: "10px 12px 11px" }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          {breakdown.standings.map((standing) => (
            <StandingRow key={standing.accountId} standing={standing} />
          ))}
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            justifyContent: "space-between",
            gap: 10,
            borderTop: "1px solid var(--color-divider)",
            marginTop: 11,
            paddingTop: 9,
          }}
        >
          <StatLabel>net worth</StatLabel>
          <span
            style={{ fontFamily: "var(--font-heading)", fontWeight: 600, fontSize: 16 }}
          >
            {fmtCompactOrExact(breakdown.total)}
          </span>
        </div>
      </Blueprint>

      <div style={{ fontSize: 11, marginTop: 6, color: MUTED }}>
        {hint} · bars are share of the year&rsquo;s largest position, figures to the
        right are the change from the prior year
      </div>
    </div>
  );
}

function StandingRow({ standing }: { standing: AccountStanding }) {
  const negative = standing.value < 0;
  const fill = negative ? HATCH : standing.color;

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "1fr auto",
        gap: "3px 8px",
        alignItems: "baseline",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
        <span
          aria-hidden
          style={{
            width: 9,
            height: 9,
            flex: "none",
            background: fill,
            border: negative ? "1px solid var(--color-accent-700)" : undefined,
          }}
        />
        <span
          style={{
            fontSize: 12.5,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {standing.label}
        </span>
      </div>
      <span
        style={{
          fontFamily: "var(--font-heading)",
          fontWeight: 600,
          fontSize: 14,
          whiteSpace: "nowrap",
        }}
      >
        {fmtCompactOrExact(standing.value)}
      </span>
      <div
        style={{
          gridColumn: "1 / -1",
          display: "grid",
          gridTemplateColumns: "1fr auto",
          gap: 8,
          alignItems: "center",
        }}
      >
        <span style={{ display: "block", height: 5, background: TRACK }} aria-hidden>
          <i
            style={{
              display: "block",
              height: "100%",
              width: `${standing.share * 100}%`,
              background: fill,
            }}
          />
        </span>
        <Delta value={standing.delta} />
      </div>
    </div>
  );
}

/**
 * The year-on-year change. A move small enough to be noise is dimmed rather
 * than dropped: the reader still gets the sign, and the rows that actually
 * moved are the ones that catch the eye.
 */
function Delta({ value }: { value: number | undefined }) {
  const flat = value != null && Math.abs(value) < 1000;
  return (
    <span
      style={{
        fontSize: 11,
        whiteSpace: "nowrap",
        color: `color-mix(in srgb, var(--color-text) ${flat ? 42 : 68}%, transparent)`,
      }}
    >
      {value == null
        ? "first year"
        : `${value > 0 ? "+" : value < 0 ? "−" : ""}${fmtCompactOrExact(Math.abs(value))}`}
    </span>
  );
}
