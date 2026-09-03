"use client";

import { Tag } from "@/components/ui";
import type { InflationProfile } from "@/lib/types";
import { KIND_LABEL } from "./DistributionTerms";
import { ShapeAxis, ShapeSpark } from "./Shape";
import { kindTone } from "./ProfileLibraryTable";
import { CPI_SCALE, bandIsFigures, bandLabel, pct } from "./distribution";

const FAINT = "color-mix(in srgb, var(--color-text) 40%, transparent)";
const SHAPE_WIDTH = 250;
const COLUMNS = "minmax(170px, 1fr) 104px 250px 66px 116px 158px";

/**
 * Inflation, in the same row grammar as the return library — one profile is
 * active per scenario and the rest stay as variants for the What-if panel, so
 * the trailing cell is the switch between them rather than a tag.
 *
 * Drawn on its own scale: a few percent of CPI plotted against a −45…+55%
 * return axis would be a flat line in every row.
 */
export function InflationProfilesTable({
  profiles,
  activeId,
  onActivate,
}: {
  profiles: InflationProfile[];
  activeId: string;
  /** Omitted while no scenario is loaded to attach the profile to. */
  onActivate?: (profile: InflationProfile) => void;
}) {
  return (
    <div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: COLUMNS,
          gap: "0 14px",
          alignItems: "end",
          padding: "0 10px 5px",
          borderBottom: "1px solid var(--color-text)",
        }}
      >
        <span className="stat-l">Profile</span>
        <span className="stat-l">Kind</span>
        <ShapeAxis
          label="Annual CPI"
          width={SHAPE_WIDTH}
          scale={CPI_SCALE}
          ticks={[0, 4, 8]}
        />
        <span className="stat-l" style={{ textAlign: "right" }}>
          Mean
        </span>
        <span className="stat-l" style={{ textAlign: "right" }}>
          5th … 95th
        </span>
        <span className="stat-l" style={{ textAlign: "right" }}>
          In use
        </span>
      </div>

      {profiles.map((profile) => (
        <div
          key={profile.id}
          style={{
            display: "grid",
            gridTemplateColumns: COLUMNS,
            gap: "0 14px",
            alignItems: "center",
            padding: "7px 10px",
            borderBottom: "1px solid var(--color-divider)",
          }}
        >
          <span
            style={{
              fontSize: 13,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
            title={profile.note}
          >
            {profile.id}
          </span>
          <span>
            <Tag tone={kindTone(profile.kind)}>{KIND_LABEL[profile.kind]}</Tag>
          </span>
          <ShapeSpark spec={profile.distribution} width={SHAPE_WIDTH} scale={CPI_SCALE} />
          <span
            style={{
              textAlign: "right",
              fontFamily: "var(--font-heading)",
              fontWeight: 600,
              fontSize: 15,
            }}
          >
            {pct(profile.mean)}
          </span>
          <span
            style={{
              textAlign: "right",
              fontSize: 11.5,
              fontFamily: "ui-monospace, Menlo, monospace",
              ...(bandIsFigures(profile.distribution) ? null : { color: FAINT }),
            }}
          >
            {bandLabel(profile.distribution)}
          </span>
          <span style={{ display: "flex", justifyContent: "flex-end" }}>
            <label className="check" title={`Use ${profile.id} for this scenario`}>
              <input
                type="radio"
                name="inflation-profile"
                checked={profile.id === activeId}
                disabled={!onActivate}
                onChange={() => onActivate?.(profile)}
                aria-label={`Use ${profile.id}`}
              />
              <span className="box" />
            </label>
          </span>
        </div>
      ))}
    </div>
  );
}
