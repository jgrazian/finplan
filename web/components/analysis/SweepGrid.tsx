"use client";

import { Blueprint } from "@/components/ui";
import { SUCCESS_RAMP, paramTick, type SweepView } from "@/lib/view/analysis";
import { fmtPercent } from "@/lib/format";

/** Plot box inside the 510×350 viewBox, leaving room for both axis labels. */
const GEO = { w: 510, h: 350, left: 64, top: 8, right: 8, bottom: 28 } as const;

const AXIS_TEXT = {
  fontSize: 10,
  fontFamily: "Barlow, sans-serif",
  fill: "#1d1f20",
  fillOpacity: 0.55,
} as const;

export interface Focus {
  x: number;
  y: number;
}

/**
 * The sweep as a heatmap, with the safety threshold drawn as a frontier.
 *
 * Cells carry no numbers of their own — thirty-six labels is a table, not an
 * image. The shape is the message; hovering a cell reads it out in the rail and
 * clicking pins it there, and the frontier table under the grid says the same
 * thing for anyone who would rather not hover at all.
 */
export function SweepGrid({
  view,
  hover,
  onHover,
  pinned,
  onPin,
}: {
  view: SweepView;
  hover: Focus | undefined;
  onHover: (focus: Focus | undefined) => void;
  pinned: Focus | undefined;
  onPin: (focus: Focus) => void;
}) {
  const gridW = GEO.w - GEO.left - GEO.right;
  const gridH = GEO.h - GEO.top - GEO.bottom;
  const cellW = gridW / Math.max(1, view.columns);
  const cellH = gridH / Math.max(1, view.rows);

  const cellX = (x: number) => GEO.left + x * cellW;
  // The vertical axis reads upwards, the way a value axis does, so the last
  // row is drawn at the top.
  const cellY = (y: number) => GEO.top + (view.rows - 1 - y) * cellH;

  const marker = (focus: Focus) => ({
    x: cellX(focus.x),
    y: cellY(focus.y),
    width: cellW,
    height: cellH,
  });

  return (
    <Blueprint style={{ padding: "8px 10px", maxWidth: 760 }}>
      <svg
        viewBox={`0 0 ${GEO.w} ${GEO.h}`}
        style={{ width: "100%", display: "block" }}
        role="img"
        aria-label={`Success rate across ${view.xAxis.label}${view.yAxis ? ` and ${view.yAxis.label}` : ""}`}
        onMouseLeave={() => onHover(undefined)}
      >
        {view.cells.map((cell) => (
          <rect
            key={`${cell.x}:${cell.y}`}
            x={cellX(cell.x)}
            y={cellY(cell.y)}
            width={cellW}
            height={cellH}
            fill={view.ramp.fill(cell.point.success_rate)}
            style={{ cursor: "crosshair" }}
            onMouseEnter={() => onHover({ x: cell.x, y: cell.y })}
            onClick={() => onPin({ x: cell.x, y: cell.y })}
          >
            <title>
              {`${paramTick(view.xAxis.kind, view.xAxis.values[cell.x])}` +
                (view.yAxis
                  ? ` × ${paramTick(view.yAxis.kind, view.yAxis.values[cell.y])}`
                  : "") +
                ` — ${fmtPercent(cell.point.success_rate)} success`}
            </title>
          </rect>
        ))}

        {/* Crosshair: the row and column the pointer is in, so a cell can be
            read against its neighbours rather than on its own. */}
        {hover && (
          <g pointerEvents="none" stroke="#1d1f20" strokeOpacity={0.55} fill="none">
            <rect x={GEO.left} y={cellY(hover.y)} width={gridW} height={cellH} />
            <rect x={cellX(hover.x)} y={GEO.top} width={cellW} height={gridH} />
          </g>
        )}

        {view.yAxis && (
          <Frontier view={view} cellX={cellX} cellY={cellY} cellW={cellW} cellH={cellH} />
        )}

        {view.planCell && (
          <Outline {...marker(view.planCell)} dashed label="the plan" />
        )}
        {pinned && <Outline {...marker(pinned)} label="pinned cell" />}

        {/* Axis labels: every column, and every row where they fit. */}
        {view.xAxis.values.map((value, x) => (
          <text
            key={`x-${x}`}
            {...AXIS_TEXT}
            x={cellX(x) + cellW / 2}
            y={GEO.h - 14}
            textAnchor="middle"
          >
            {paramTick(view.xAxis.kind, value)}
          </text>
        ))}
        {view.yAxis?.values.map((value, y) => (
          <text
            key={`y-${y}`}
            {...AXIS_TEXT}
            x={GEO.left - 8}
            y={cellY(y) + cellH / 2 + 3.5}
            textAnchor="end"
          >
            {paramTick(view.yAxis!.kind, value)}
          </text>
        ))}
        <text {...AXIS_TEXT} x={GEO.left + gridW / 2} y={GEO.h - 2} textAnchor="middle">
          {view.xAxis.role}
        </text>
      </svg>
    </Blueprint>
  );
}

/**
 * The threshold, drawn where the grid crosses it.
 *
 * A step line along cell edges rather than a curve through cell centres: the
 * measurement is per cell, and a smooth line would claim resolution between
 * two columns that nothing was simulated at. It breaks wherever a column has
 * no safe cell at all.
 */
function Frontier({
  view,
  cellX,
  cellY,
  cellW,
  cellH,
}: {
  view: SweepView;
  cellX: (x: number) => number;
  cellY: (y: number) => number;
  cellW: number;
  cellH: number;
}) {
  const segments: string[] = [];
  let current: string[] = [];

  for (let x = 0; x < view.columns; x++) {
    const y = view.frontier[x];
    if (y == null) {
      if (current.length > 1) segments.push(current.join(" "));
      current = [];
      continue;
    }
    // The boundary runs along the far edge of the last safe cell: its top when
    // success falls upwards, its bottom when it falls downwards.
    const edge = view.fallsWithY ? cellY(y) : cellY(y) + cellH;
    const left = cellX(x);
    current.push(current.length === 0 ? `M ${left} ${edge}` : `L ${left} ${edge}`);
    current.push(`L ${left + cellW} ${edge}`);
  }
  if (current.length > 1) segments.push(current.join(" "));
  if (segments.length === 0) return null;

  const path = segments.join(" ");
  return (
    <g pointerEvents="none" fill="none">
      {/* A ring in the page's own colour, so the line stays legible over both
          ends of the ramp. */}
      <path d={path} stroke="#f2f2f3" strokeWidth={4.5} />
      <path d={path} stroke="#1d1f20" strokeWidth={1.5} />
    </g>
  );
}

function Outline({
  x,
  y,
  width,
  height,
  dashed,
  label,
}: {
  x: number;
  y: number;
  width: number;
  height: number;
  dashed?: boolean;
  label: string;
}) {
  return (
    <g pointerEvents="none" fill="none">
      <title>{label}</title>
      <rect x={x} y={y} width={width} height={height} stroke="#f2f2f3" strokeWidth={4} />
      <rect
        x={x}
        y={y}
        width={width}
        height={height}
        stroke="#1d1f20"
        strokeWidth={dashed ? 1.5 : 2.5}
        strokeDasharray={dashed ? "3 2" : undefined}
      />
    </g>
  );
}

/** The ramp, the frontier and the two markers, named. */
export function SweepLegend({ view }: { view: SweepView }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        marginTop: 8,
        flexWrap: "wrap",
        fontSize: 10.5,
        color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
      }}
    >
      <span>{fmtPercent(view.ramp.low, 0)}</span>
      <div style={{ width: 150, display: "flex", height: 9 }}>
        {SUCCESS_RAMP.map((step) => (
          <i key={step} style={{ flex: 1, background: step }} />
        ))}
      </div>
      <span>{fmtPercent(view.ramp.high, 0)} success</span>
      {view.yAxis && (
        <Key marginLeft={12}>
          <i
            style={{
              width: 18,
              borderTop: "1.5px solid var(--color-text)",
              display: "inline-block",
            }}
          />
          ≥ {fmtPercent(view.threshold, 0)} frontier
        </Key>
      )}
      <Key>
        <i
          style={{
            width: 12,
            height: 9,
            border: "1.5px dashed var(--color-text)",
            display: "inline-block",
          }}
        />
        the plan
      </Key>
      <Key>
        <i
          style={{
            width: 12,
            height: 9,
            border: "2px solid var(--color-text)",
            display: "inline-block",
          }}
        />
        pinned
      </Key>
    </div>
  );
}

function Key({ children, marginLeft }: { children: React.ReactNode; marginLeft?: number }) {
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 6, marginLeft }}>{children}</span>
  );
}
