"use client";

import { Blueprint, StatLabel } from "@/components/ui";
import { fmtCurrency, fmtShareFine } from "@/lib/format";
import type { ReturnProfileId } from "@/lib/types";
import type { AssetMix, AssetMixSlice } from "@/lib/view/assets";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/** A legend entry is a control, not a button: no box, no padding, no chrome. */
const BARE = {
  appearance: "none",
  background: "none",
  border: 0,
  padding: 0,
  margin: 0,
  font: "inherit",
  color: "inherit",
  textAlign: "left",
} as const;

/**
 * What the portfolio is worth, and which return profiles carry it.
 *
 * The table below is an itemisation of one figure; this is that figure, split
 * by the thing that actually moves it. Eight holdings can be one bet, and no
 * amount of reading down a ticker column says so — the bar does, in one line.
 *
 * Both the bar and the legend select the profile they name, which puts the
 * library row and its drawer one click from the concentration that sent you
 * looking. The share column in the table is drawn from the same ramp, so a
 * colour means the same profile in both.
 */
export function AssetMixCard({
  mix,
  selectedProfileId,
  onSelect,
}: {
  mix: AssetMix;
  /** The profile the drawer is on, if it is a profile at all. */
  selectedProfileId?: ReturnProfileId;
  onSelect: (profileId: ReturnProfileId) => void;
}) {
  return (
    <Blueprint
      style={{
        padding: "14px 16px 15px",
        display: "flex",
        flexDirection: "column",
        gap: 10,
        marginBottom: 18,
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
        <div>
          <StatLabel>Portfolio value by return profile</StatLabel>
          <div
            style={{
              fontFamily: "var(--font-heading)",
              fontWeight: 600,
              fontSize: 30,
              lineHeight: 1.05,
              marginTop: 2,
            }}
          >
            {fmtCurrency(mix.total)}
          </div>
        </div>
        <span style={{ fontSize: 11.5, color: MUTED }}>{carriedBy(mix.slices)}</span>
      </div>

      {/* Clickable, but not a tab stop: every segment is named in the legend
          below, and that is where the keyboard reaches them. */}
      <div style={{ display: "flex", height: 10, gap: 1 }} aria-hidden>
        {mix.slices.map((slice) => {
          const profileId = slice.profileId;
          return (
            <i
              key={key(slice)}
              title={`${slice.label} — ${fmtCurrency(slice.value)}`}
              onClick={profileId == null ? undefined : () => onSelect(profileId)}
              style={{
                width: `${slice.share * 100}%`,
                background: slice.color,
                display: "block",
                cursor: profileId == null ? "default" : "pointer",
              }}
            />
          );
        })}
      </div>

      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: "5px 16px",
          fontSize: 11,
          color: MUTED,
        }}
      >
        {mix.slices.map((slice) => (
          <LegendEntry
            key={key(slice)}
            slice={slice}
            selected={slice.profileId != null && slice.profileId === selectedProfileId}
            onSelect={onSelect}
          />
        ))}
      </div>
    </Blueprint>
  );
}

function LegendEntry({
  slice,
  selected,
  onSelect,
}: {
  slice: AssetMixSlice;
  selected: boolean;
  onSelect: (profileId: ReturnProfileId) => void;
}) {
  const body = (
    <>
      <i
        style={{ width: 8, height: 8, background: slice.color, display: "block", flex: "none" }}
        aria-hidden
      />
      {slice.label}
      <span style={{ fontFamily: "ui-monospace, Menlo, monospace", color: "var(--color-text)" }}>
        {fmtShareFine(slice.share)}
      </span>
    </>
  );
  const layout = {
    display: "flex",
    alignItems: "center",
    gap: 5,
    // The selected profile is the one the drawer is on: it stops being muted.
    ...(selected ? { color: "var(--color-text)" } : null),
  } as const;

  // Nothing to open for the holdings pointing at nothing — an unmapped slice
  // names an absence, and there is no library row behind it.
  if (slice.profileId == null) return <span style={layout}>{body}</span>;

  const profileId = slice.profileId;
  return (
    <button
      type="button"
      style={{ ...BARE, ...layout, cursor: "pointer" }}
      title={`${slice.assets} asset${slice.assets === 1 ? "" : "s"} · ${fmtCurrency(slice.value)}`}
      onClick={() => onSelect(profileId)}
    >
      {body}
    </button>
  );
}

/** A slice is keyed by the profile it names; the unmapped one has no id. */
function key(slice: AssetMixSlice): string {
  return slice.profileServerId == null ? "unmapped" : String(slice.profileServerId);
}

/** `5 profiles carry the portfolio` — the unmapped slice carries nothing. */
function carriedBy(slices: AssetMixSlice[]): string {
  const mapped = slices.filter((s) => s.profileServerId != null).length;
  if (mapped === 0) return "nothing here is mapped to a profile";
  return `${mapped} profile${mapped === 1 ? "" : "s"} carr${mapped === 1 ? "ies" : "y"} the portfolio`;
}
