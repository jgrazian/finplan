import { Blueprint, SectionHeading } from "@/components/ui";
import { CURVE_GEOMETRY, distributionPath, pct, quantiles } from "./distribution";

/**
 * The shape of a profile's annual returns, with the mean marked. Fixed
 * profiles collapse to a spike, and their quantile row would be three copies
 * of the mean — so it is dropped rather than repeated.
 */
export function DistributionCurve({
  kind,
  mean,
  sd,
}: {
  kind: string;
  mean: number;
  sd: number;
}) {
  const { w, h } = CURVE_GEOMETRY;
  const isFixed = sd === 0;
  const q = quantiles(mean, sd);

  return (
    <div>
      <SectionHeading className="mb-[6px]">
        {isFixed ? "Constant — no dispersion" : `Annual return distribution (${kind})`}
      </SectionHeading>

      <Blueprint style={{ padding: "6px 8px" }}>
        <svg viewBox={`0 0 ${w} ${h}`} style={{ width: "100%", display: "block" }}>
          <line
            x1={8}
            x2={w - 8}
            y1={h - 8}
            y2={h - 8}
            stroke="#1d1f20"
            strokeOpacity={0.3}
          />
          <path
            d={distributionPath(sd)}
            fill="#5980a6"
            fillOpacity={0.22}
            stroke="#41617f"
            strokeWidth={1.2}
          />
          <line
            x1={w / 2}
            x2={w / 2}
            y1={10}
            y2={h - 8}
            stroke="#1d2d3d"
            strokeOpacity={0.5}
            strokeDasharray="3 3"
          />
        </svg>
      </Blueprint>

      {!isFixed && (
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            fontSize: 11,
            marginTop: 4,
            color: "color-mix(in srgb, var(--color-text) 60%, transparent)",
          }}
        >
          <span>5th {q.p5}</span>
          <span>median {pct(mean)}</span>
          <span>95th {q.p95}</span>
        </div>
      )}
    </div>
  );
}
