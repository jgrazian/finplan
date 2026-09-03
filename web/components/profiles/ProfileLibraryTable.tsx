"use client";

import type { KeyboardEvent } from "react";
import { Tag, type TagTone, rowStyle } from "@/components/ui";
import { CLASS_LABEL } from "@/lib/tickers";
import type { DistributionKind, ReturnProfile } from "@/lib/types";
import { KIND_LABEL } from "./DistributionTerms";
import { ShapeAxis, ShapeSpark } from "./Shape";
import { RETURN_SCALE, bandIsFigures, bandLabel, pct } from "./distribution";

const FAINT = "color-mix(in srgb, var(--color-text) 40%, transparent)";
const SHAPE_WIDTH = 250;
const COLUMNS = "minmax(170px, 1fr) 104px 250px 66px 116px 158px";

/**
 * How loudly a kind announces itself. A parametric distribution is the normal
 * case and takes the accent; the two with no parameters are quieter, and None
 * is an outline because it is the absence of a distribution rather than one
 * more of them.
 */
export function kindTone(kind: DistributionKind): TagTone {
  if (kind === "None") return "outline";
  if (kind === "Fixed" || kind === "Bootstrap") return "neutral";
  return "accent";
}

/**
 * The return profile library, below the assets it drives.
 *
 * Every row is drawn on one return scale, so the middle of the row — which in
 * a name/kind/mean table is dead space — carries the thing the numbers are a
 * summary of. A profile with no assets is unremarkable here: an account can
 * point at one directly for its cash or its property value.
 */
export function ProfileLibraryTable({
  profiles,
  selectedId,
  onSelect,
}: {
  profiles: ReturnProfile[];
  /** Name of the profile driving the drawer, or undefined. */
  selectedId: string | undefined;
  onSelect: (profile: ReturnProfile) => void;
}) {
  return (
    <div role="listbox" aria-label="Return profile library">
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
          label="Annual return"
          width={SHAPE_WIDTH}
          scale={RETURN_SCALE}
          ticks={[-40, -20, 0, 20, 40]}
        />
        <span className="stat-l" style={{ textAlign: "right" }}>
          Mean
        </span>
        <span className="stat-l" style={{ textAlign: "right" }}>
          5th … 95th
        </span>
        <span className="stat-l" style={{ textAlign: "right" }}>
          Used by
        </span>
      </div>

      {profiles.map((profile) => {
        const selected = profile.id === selectedId;
        const used = profile.usedBy.join(" · ");
        return (
          <div
            key={profile.id}
            className="rowsel"
            role="option"
            aria-selected={selected}
            tabIndex={0}
            style={rowStyle(selected)}
            onClick={() => onSelect(profile)}
            onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(profile);
              }
            }}
          >
            <div
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
                title={
                  profile.assetClass
                    ? `${profile.id} · ${CLASS_LABEL[profile.assetClass]}`
                    : profile.id
                }
              >
                {profile.id}
                {/* What a ticker of this class resolves to. Unclassified rows
                    say nothing rather than saying "none": the absence is the
                    ordinary case, and a column of dashes would read as a
                    defect. */}
                {profile.assetClass && (
                  <span style={{ fontSize: 11, marginLeft: 6, color: FAINT }}>
                    {CLASS_LABEL[profile.assetClass]}
                  </span>
                )}
              </span>
              <span>
                <Tag tone={kindTone(profile.kind)}>{KIND_LABEL[profile.kind]}</Tag>
              </span>
              <ShapeSpark
                spec={profile.distribution}
                width={SHAPE_WIDTH}
                scale={RETURN_SCALE}
                history={profile.history}
              />
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
                  ...(bandIsFigures(profile.distribution, profile.history)
                    ? null
                    : { color: FAINT }),
                }}
              >
                {bandLabel(profile.distribution, profile.history)}
              </span>
              <span
                style={{
                  textAlign: "right",
                  fontSize: 11.5,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  ...(used === "" ? { color: FAINT } : null),
                }}
                title={used}
              >
                {used === "" ? "—" : used}
              </span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
