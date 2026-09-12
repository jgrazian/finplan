import type { CSSProperties, ReactNode } from "react";
import { Blueprint } from "@/components/ui";
import type { DistributionSpec } from "@/lib/api/types";
import {
  type QuantileMark,
  type ShapeBox,
  type ShapePaths,
  bandLabel,
  pct,
  shapePaths,
} from "./distribution";

/** The span a shape is drawn across, in percent. */
export interface Scale {
  lo: number;
  hi: number;
}

const INK = "var(--color-text)";
const CURVE = "var(--color-accent-700)";
const WASH = "var(--color-accent)";

/** Room under the baseline for the quantile figures, in viewBox units. */
const LABEL_BAND = 14;
const LABEL_SIZE = 8;
/** Barlow's digits at 8px; near enough to keep two labels from touching. */
const CHAR = 4.5;

/**
 * The scale, drawn once in the column header rather than in every row.
 *
 * The rows below it carry no labels of their own — eleven copies of the same
 * axis would be eleven times the ink for one fact — so the header is what
 * makes a row's shape readable, and the two must be the same width.
 */
export function ShapeAxis({
  label,
  width,
  scale,
  ticks,
  fontSize = 8.5,
}: {
  label: string;
  width: number;
  scale: Scale;
  /** Values, in percent, to mark. Zero is drawn taller than the rest. */
  ticks: number[];
  fontSize?: number;
}) {
  const h = 12;
  const base = h - 1;
  const x = (v: number) => ((v - scale.lo) / (scale.hi - scale.lo)) * width;
  return (
    <div>
      <span className="stat-l" style={{ display: "block", marginBottom: 3 }}>
        {label}
      </span>
      <svg
        viewBox={`0 0 ${width} ${h}`}
        style={{ width, display: "block", overflow: "visible" }}
        aria-hidden
      >
        <g stroke={INK} strokeOpacity={0.35}>
          <line x1={0} x2={width} y1={base} y2={base} />
          {ticks.map((v) => (
            <line
              key={v}
              x1={x(v)}
              x2={x(v)}
              y1={v === 0 ? base - 7 : base - 4}
              y2={base}
              strokeOpacity={v === 0 ? 0.6 : undefined}
            />
          ))}
        </g>
        <g
          fontSize={fontSize}
          fontFamily="Barlow, sans-serif"
          fill={INK}
          fillOpacity={0.55}
          textAnchor="middle"
        >
          {ticks.map((v) => (
            <text key={v} x={x(v)} y={base - 6}>
              {v === 0 ? "0" : v < 0 ? `−${-v}` : `+${v}`}
            </text>
          ))}
        </g>
      </svg>
    </div>
  );
}

/**
 * One profile's shape at row size, with the 5th, 50th and 95th marked on it.
 *
 * The marks are what make a row comparable at a glance: two profiles with the
 * same mean can put their bad year 15 points apart, and the tick at the 5th is
 * where that shows. There is no room for the figures at this size — the band
 * column beside it carries those.
 *
 * Bootstrap draws nothing: the client is sent the name of a history, never the
 * observations, so any curve here would be invented. The empty cell is the
 * honest reading, and the Kind tag beside it already says why.
 */
export function ShapeSpark({
  spec,
  width,
  height = 26,
  scale,
  history,
}: {
  spec: DistributionSpec;
  width: number;
  height?: number;
  scale: Scale;
  /** The observed years, where the profile resamples a history. */
  history?: readonly number[];
}) {
  const box: ShapeBox = { w: width, h: height, ...scale };
  const paths = shapePaths(spec, box, history);
  const zero = ((0 - scale.lo) / (scale.hi - scale.lo)) * width;
  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      style={{ width, display: "block" }}
      role="img"
      aria-label={ariaLabel(spec, paths, history)}
    >
      <line
        x1={0}
        x2={width}
        y1={height - 2}
        y2={height - 2}
        stroke={INK}
        strokeOpacity={0.14}
      />
      <line x1={zero} x2={zero} y1={2} y2={height - 2} stroke={INK} strokeOpacity={0.18} />
      <ShapeInk paths={paths} thin />
    </svg>
  );
}

/**
 * The same shape at drawer size, with the three quantiles written under the
 * ticks that mark them. The scale's ends sit below, since the drawer has no
 * column header to carry them.
 */
export function ShapePanel({
  spec,
  height = 80,
  scale,
  footer = true,
  frame = true,
  history,
}: {
  spec: DistributionSpec;
  height?: number;
  scale: Scale;
  /** Drop it where the surrounding block already states the scale. */
  footer?: boolean;
  /** Drop the hairline box where this already sits inside one. */
  frame?: boolean;
  /** The observed years, where the profile resamples a history. */
  history?: readonly number[];
}) {
  const width = 300;
  const box: ShapeBox = { w: width, h: height, ...scale };
  const paths = shapePaths(spec, box, history);
  const zero = ((0 - scale.lo) / (scale.hi - scale.lo)) * width;
  const labels = quantileLabels(paths.points, width);
  const full = height + (labels.length > 0 ? LABEL_BAND : 0);
  const Frame = frame ? Blueprint : Bare;
  return (
    <div>
      <Frame style={{ padding: "6px 8px" }}>
        <svg
          viewBox={`0 0 ${width} ${full}`}
          style={{ width: "100%", display: "block" }}
          role="img"
          aria-label={ariaLabel(spec, paths, history)}
        >
          <line
            x1={0}
            x2={width}
            y1={height - 2}
            y2={height - 2}
            stroke={INK}
            strokeOpacity={0.3}
          />
          <line x1={zero} x2={zero} y1={6} y2={height - 2} stroke={INK} strokeOpacity={0.22} />
          <ShapeInk paths={paths} />
          <g fontSize={LABEL_SIZE} fontFamily="Barlow, sans-serif" textAnchor="middle">
            {labels.map((label) => (
              <text key={label.q} x={label.x} y={height + 8} fill={CURVE}>
                {label.rank && (
                  <tspan fillOpacity={0.55}>
                    {label.rank}
                    {" "}
                  </tspan>
                )}
                <tspan fillOpacity={0.9}>{pct(label.v)}</tspan>
              </text>
            ))}
          </g>
        </svg>
      </Frame>
      {footer && (
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            gap: 8,
            fontSize: 11,
            marginTop: 4,
            color: "color-mix(in srgb, var(--color-text) 60%, transparent)",
          }}
        >
          <span>{scale.lo}%</span>
          {/* Once the figures are on the chart, the only thing left to say
              here is the kinds that have no figures at all. */}
          {labels.length === 0 && <span>{bandLabel(spec, history)}</span>}
          <span>+{scale.hi}%</span>
        </div>
      )}
    </div>
  );
}

/** Stands in for the frame where the surrounding block is already one. */
function Bare({ children }: { children?: ReactNode; style?: CSSProperties }) {
  return <>{children}</>;
}

/** One quantile figure, and whether there is room to say which one it is. */
interface QuantileLabel {
  q: number;
  v: number;
  x: number;
  /** `5th`, or empty once the labels are close enough to need the room. */
  rank: string;
}

const RANK: Record<number, string> = { 5: "5th", 50: "med", 95: "95th" };

/**
 * Lays the three figures out under the ticks, dropping detail as they crowd
 * rather than letting them overlap: first the rank words go, then the median
 * figure — which is the one the row above it and the drawer heading already
 * carry. A narrow profile like an aggregate bond fund puts its 5th and 95th
 * 18 points apart, which is 45px of a 300px box.
 */
function quantileLabels(points: QuantileMark[], width: number): QuantileLabel[] {
  if (points.length === 0) return [];

  const laid = (rank: boolean, drop50: boolean): QuantileLabel[] =>
    points
      .filter((m) => !(drop50 && m.q === 50))
      .map((m) => {
        const half = halfWidth(m.q, m.v, rank);
        return {
          q: m.q,
          v: m.v,
          rank: rank ? RANK[m.q] : "",
          // Keep the figure inside the box; a label half off the edge reads
          // as a different number.
          x: clamp(m.x, half, width - half),
        };
      });

  for (const attempt of [laid(true, false), laid(false, false), laid(false, true)]) {
    if (!collides(attempt)) return attempt;
  }
  return laid(false, true);
}

/** Half a label's drawn width, which is what a centred label needs each side. */
function halfWidth(q: number, v: number, rank: boolean): number {
  return (((rank ? `${RANK[q]} ` : "") + pct(v)).length * CHAR) / 2;
}

function collides(labels: QuantileLabel[]): boolean {
  for (let i = 1; i < labels.length; i++) {
    const a = labels[i - 1];
    const b = labels[i];
    const half = (l: QuantileLabel) => halfWidth(l.q, l.v, l.rank !== "");
    if (b.x - a.x < half(a) + half(b) + 3) return true;
  }
  return false;
}

function clamp(v: number, lo: number, hi: number): number {
  return hi < lo ? (lo + hi) / 2 : Math.min(hi, Math.max(lo, v));
}

/** What a screen reader is told, since the picture carries the rest. */
function ariaLabel(
  spec: DistributionSpec,
  paths: ShapePaths,
  history?: readonly number[],
): string {
  const median = paths.points.find((m) => m.q === 50);
  const middle = median ? `, median ${pct(median.v)}` : "";
  return `${spec.kind} · ${bandLabel(spec, history)}${middle}`;
}

/** The curves and the ticks, in the order they stack. */
function ShapeInk({ paths, thin }: { paths: ShapePaths; thin?: boolean }) {
  return (
    <>
      {paths.fill && <path d={paths.fill} fill={WASH} fillOpacity={thin ? 0.2 : 0.22} />}
      {/* Bars carry their own outline: a histogram read as a filled area
          would blur the fact that each bar is a count of whole years. */}
      {paths.bars && (
        <path
          d={paths.bars}
          fill={WASH}
          fillOpacity={thin ? 0.28 : 0.3}
          stroke={CURVE}
          strokeWidth={thin ? 0.6 : 0.8}
          strokeOpacity={0.85}
        />
      )}
      {paths.band && (
        <path
          d={paths.band}
          fill="none"
          stroke={CURVE}
          strokeWidth={thin ? 0.9 : 1.1}
          strokeOpacity={0.5}
        />
      )}
      {paths.median && (
        <path
          d={paths.median}
          fill="none"
          stroke={CURVE}
          strokeWidth={thin ? 1 : 1.2}
          strokeOpacity={0.65}
        />
      )}
      {paths.line && (
        <path d={paths.line} fill="none" stroke={CURVE} strokeWidth={thin ? 1 : 1.2} />
      )}
      {paths.line2 && (
        <path
          d={paths.line2}
          fill="none"
          stroke={CURVE}
          strokeWidth={1}
          strokeDasharray={thin ? "2 2" : "3 3"}
          strokeOpacity={0.75}
        />
      )}
      {paths.marks && (
        <path
          d={paths.marks}
          fill="none"
          stroke={CURVE}
          strokeWidth={thin ? 1 : 1.4}
          strokeOpacity={thin ? 0.45 : 0.6}
        />
      )}
    </>
  );
}
