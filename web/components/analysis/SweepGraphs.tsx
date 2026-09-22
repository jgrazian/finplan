"use client";

import { RAMP, shade, type GraphView } from "@/lib/view/sweep";
import { paramTick } from "@/lib/view/analysis";

/**
 * The three ways a graph can draw a finished sweep.
 *
 * All three read the same `GraphView` — the metric, the one or two axes it is
 * drawn over, and the slice everything else is held at — so switching between
 * them in the inspector is a change of picture and never a change of question.
 */

const AXIS_TEXT = {
  fontSize: 10,
  fontFamily: "Barlow, sans-serif",
  fill: "var(--color-text)",
  fillOpacity: 0.55,
} as const;

/** The plot's own ink, matched to the ramp's middle so a line sits in the family. */
const INK = "var(--color-accent-700)";
const DOT = "var(--color-accent-900)";
/** The page's ground, for haloing a mark drawn over the dark end of the ramp. */
const GROUND = "var(--color-bg)";

/**
 * Why every chart here has two geometries.
 *
 * An SVG with a viewBox scales its text along with its marks, so a drawing
 * stretched across both columns renders its 10px axis labels at 24px and reads
 * as a different, louder chart. A wide card is given a wider viewBox instead:
 * the extra width becomes plot area at the same scale, which is what widening a
 * graph is for.
 */
interface Plot {
  w: number;
  h: number;
  left: number;
  top: number;
  right: number;
  bottom: number;
}

// ───────────────────────────── line ─────────────────────────────

const LINE: Record<"half" | "full", Plot> = {
  half: { w: 480, h: 232, left: 58, top: 20, right: 16, bottom: 30 },
  full: { w: 1000, h: 300, left: 64, top: 22, right: 20, bottom: 34 },
};

/**
 * The metric against one variable.
 *
 * The only kind that labels its dependent axis with numbers, which is why it is
 * what a one-variable sweep opens on: a heatmap of a single column is a colour
 * bar, and a colour bar is not something you read a value off.
 */
export function LineGraph({ view }: { view: GraphView }) {
  const geo = LINE[view.spec.wide ? "full" : "half"];
  const plotW = geo.w - geo.left - geo.right;
  const plotH = geo.h - geo.top - geo.bottom;
  const values = view.xAxis.values;
  const count = values.length;
  const { low, high } = view.span;

  const x = (i: number) => geo.left + (count <= 1 ? plotW / 2 : (i / (count - 1)) * plotW);
  const y = (value: number) =>
    geo.top + (1 - clamp01((value - low) / (high - low || 1))) * plotH;

  const measured = values.map((_, i) => ({ i, value: view.sample(i) }));
  const drawn = measured.filter((p): p is { i: number; value: number } => p.value != null);
  const path = drawn.map((p, n) => `${n === 0 ? "M" : "L"} ${x(p.i)} ${y(p.value)}`).join(" ");
  const baseline = geo.top + plotH;

  return (
    <svg
      viewBox={`0 0 ${geo.w} ${geo.h}`}
      style={{ width: "100%", display: "block" }}
      role="img"
      aria-label={`${view.metric.label} against ${view.xAxis.label}`}
    >
      {[low, (low + high) / 2, high].map((tick, n) => (
        <g key={n}>
          <line
            x1={geo.left}
            x2={geo.w - geo.right}
            y1={y(tick)}
            y2={y(tick)}
            stroke="var(--color-text)"
            strokeOpacity={n === 0 ? 0.3 : 0.09}
          />
          <text {...AXIS_TEXT} x={geo.left - 7} y={y(tick) + 3.5} textAnchor="end">
            {view.metric.tick(tick)}
          </text>
        </g>
      ))}

      {/* Where the plan's own value sits on this variable. */}
      {view.planX != null && (
        <line
          x1={x(view.planX)}
          x2={x(view.planX)}
          y1={geo.top}
          y2={baseline}
          stroke="var(--color-text)"
          strokeOpacity={0.4}
          strokeDasharray="3 3"
        />
      )}

      <path d={path} fill="none" stroke={INK} strokeWidth={2} />
      {drawn.map((p) => (
        <circle key={p.i} cx={x(p.i)} cy={y(p.value)} r={3.2} fill={DOT}>
          <title>{`${paramTick(view.xAxis.kind, values[p.i])} — ${view.metric.format(p.value)}`}</title>
        </circle>
      ))}

      {/* Only the ends carry a number: six labels on a 480-wide chart is a smear. */}
      {[0, count - 1].map((i) =>
        i >= 0 && i < count ? (
          <text
            key={`t-${i}`}
            {...AXIS_TEXT}
            x={x(i)}
            y={geo.h - 12}
            textAnchor={i === 0 ? "start" : "end"}
          >
            {paramTick(view.xAxis.kind, values[i])}
          </text>
        ) : null,
      )}
      <text {...AXIS_TEXT} x={geo.left + plotW / 2} y={geo.h - 2} textAnchor="middle">
        {view.xName}
      </text>
    </svg>
  );
}

// ──────────────────────────── heatmap ────────────────────────────

const HEAT: Record<"half" | "full", Plot> = {
  half: { w: 480, h: 232, left: 62, top: 8, right: 8, bottom: 30 },
  full: { w: 1000, h: 300, left: 70, top: 10, right: 10, bottom: 34 },
};

/**
 * Two variables as a field of colour.
 *
 * Cells carry no numbers — thirty-six labels is a table, not an image. The
 * shape is the message, and hovering a cell reads the pair and the value out.
 */
export function HeatGraph({ view }: { view: GraphView }) {
  const yAxis = view.yAxis;
  if (!yAxis) return <LineGraph view={view} />;

  const geo = HEAT[view.spec.wide ? "full" : "half"];
  const gridW = geo.w - geo.left - geo.right;
  const gridH = geo.h - geo.top - geo.bottom;
  const columns = view.xAxis.values.length;
  const rows = yAxis.values.length;
  const cellW = gridW / Math.max(1, columns);
  const cellH = gridH / Math.max(1, rows);

  const cellX = (x: number) => geo.left + x * cellW;
  // The vertical axis reads upwards, the way a value axis does, so the last
  // row is drawn at the top.
  const cellY = (y: number) => geo.top + (rows - 1 - y) * cellH;

  const cells = [];
  for (let x = 0; x < columns; x++) {
    for (let y = 0; y < rows; y++) {
      const value = view.sampleColour(x, y);
      if (value == null) continue;
      cells.push({ x, y, value });
    }
  }

  return (
    <svg
      viewBox={`0 0 ${geo.w} ${geo.h}`}
      style={{ width: "100%", display: "block" }}
      role="img"
      aria-label={`${view.metric.label} across ${view.xAxis.label} and ${yAxis.label}`}
    >
      {cells.map((cell) => (
        <rect
          key={`${cell.x}:${cell.y}`}
          x={cellX(cell.x)}
          y={cellY(cell.y)}
          width={cellW}
          height={cellH}
          fill={shade(view.colourSpan, cell.value)}
        >
          <title>
            {`${paramTick(view.xAxis.kind, view.xAxis.values[cell.x])} × ` +
              `${paramTick(yAxis.kind, yAxis.values[cell.y])} — ` +
              view.colour.format(cell.value)}
          </title>
        </rect>
      ))}

      {view.planX != null && view.planY != null && (
        <Marker
          x={cellX(view.planX)}
          y={cellY(view.planY)}
          width={cellW}
          height={cellH}
          label="the plan"
        />
      )}

      {view.xAxis.values.map((value, x) => (
        <text
          key={`x-${x}`}
          {...AXIS_TEXT}
          x={cellX(x) + cellW / 2}
          y={geo.h - 14}
          textAnchor="middle"
        >
          {paramTick(view.xAxis.kind, value)}
        </text>
      ))}
      {yAxis.values.map((value, y) => (
        <text
          key={`y-${y}`}
          {...AXIS_TEXT}
          x={geo.left - 7}
          y={cellY(y) + cellH / 2 + 3.5}
          textAnchor="end"
        >
          {paramTick(yAxis.kind, value)}
        </text>
      ))}
      <text {...AXIS_TEXT} x={geo.left + gridW / 2} y={geo.h - 3} textAnchor="middle">
        {view.xName} →
      </text>
      {/* Rotated into the left gutter: two axes of "amount" have to say which
          amount each one is, and the subtitle alone cannot put a name beside
          the ticks it belongs to. */}
      <text
        {...AXIS_TEXT}
        transform={`rotate(-90 10 ${geo.top + gridH / 2})`}
        x={10}
        y={geo.top + gridH / 2 + 3.5}
        textAnchor="middle"
      >
        {view.yName} →
      </text>
    </svg>
  );
}

function Marker({
  x,
  y,
  width,
  height,
  label,
}: {
  x: number;
  y: number;
  width: number;
  height: number;
  label: string;
}) {
  return (
    <g pointerEvents="none" fill="none">
      <title>{label}</title>
      <rect x={x} y={y} width={width} height={height} stroke={GROUND} strokeWidth={3.5} />
      <rect
        x={x}
        y={y}
        width={width}
        height={height}
        stroke="var(--color-text)"
        strokeWidth={1.5}
        strokeDasharray="3 2"
      />
    </g>
  );
}

// ──────────────────────────── surface ────────────────────────────

/**
 * The side padding is a gutter for the two axis names.
 *
 * A fitted diamond fills its box corner to corner, so a label pushed clear of
 * the floor has to be pushed into room that was left for it — otherwise it
 * either overlaps the surface or leaves the frame.
 */
const SURFACE: Record<"half" | "full", Plot> = {
  half: { w: 500, h: 290, left: 52, top: 12, right: 52, bottom: 26 },
  full: { w: 1040, h: 420, left: 84, top: 14, right: 84, bottom: 30 },
};

/**
 * How tall the metric's band stands, as a fraction of the grid's width.
 *
 * Fixed rather than fitted: a surface whose height axis were stretched to fill
 * whatever room was left would make every plan's slope look the same, and the
 * slope is the whole reason to draw a surface instead of a heatmap.
 */
const RISE = 0.6;

const rad = (deg: number) => (deg * Math.PI) / 180;

interface Projected {
  x: number;
  y: number;
  /** Distance from the camera, for drawing back to front. */
  depth: number;
}

/**
 * One point of the grid, in unfitted drawing units.
 *
 * `u` and `v` run −0.5 to 0.5 across the two swept axes, so the rotation turns
 * about the middle of the grid rather than swinging the whole picture around a
 * corner. `t` is the height, 0 at the floor and 1 at the top of the band.
 *
 * Axonometric, with no perspective: a step across the grid is the same distance
 * wherever it is taken, which is what lets two slopes be compared by eye.
 */
function project(u: number, v: number, t: number, azimuth: number, elevation: number): Projected {
  const ca = Math.cos(rad(azimuth));
  const sa = Math.sin(rad(azimuth));
  const ce = Math.cos(rad(elevation));
  const se = Math.sin(rad(elevation));
  const depth = -u * sa + v * ca;
  return { x: u * ca + v * sa, y: depth * se - t * RISE * ce, depth };
}

/**
 * Fit whatever was projected into the plot box.
 *
 * The alternative is a scale per angle, and every one of those constants is a
 * chance for a rotation to push the drawing through its own frame. One uniform
 * scale off the actual bounding box holds at every angle, and keeps the two
 * axes to the same scale so the projection stays honest.
 */
function fitter(points: Projected[], box: Plot) {
  const plotW = box.w - box.left - box.right;
  const plotH = box.h - box.top - box.bottom;
  const xs = points.map((p) => p.x);
  const ys = points.map((p) => p.y);
  const minX = Math.min(...xs);
  const minY = Math.min(...ys);
  const width = Math.max(...xs) - minX || 1;
  const height = Math.max(...ys) - minY || 1;
  const scale = Math.min(plotW / width, plotH / height);
  const dx = box.left + (plotW - width * scale) / 2;
  const dy = box.top + (plotH - height * scale) / 2;
  return (p: Projected) => ({
    px: round(dx + (p.x - minX) * scale),
    py: round(dy + (p.y - minY) * scale),
  });
}

/**
 * Two variables as a height field, turnable.
 *
 * A heatmap says where the metric is high; a surface says how steeply it gets
 * there, which is the question a cliff edge in a plan is really about. Colour is
 * a second channel here rather than a restatement of the height: pointed at
 * another measure it answers where a plan is both safe and rich.
 */
export function SurfaceGraph({ view }: { view: GraphView }) {
  const yAxis = view.yAxis;
  if (!yAxis) return <LineGraph view={view} />;

  const box = SURFACE[view.spec.wide ? "full" : "half"];
  const columns = view.xAxis.values.length;
  const rows = yAxis.values.length;
  const { azimuth, elevation } = view;

  const frac = (i: number, count: number) => (count <= 1 ? 0 : i / (count - 1)) - 0.5;
  const lift = (value: number | undefined) =>
    value == null
      ? undefined
      : clamp01((value - view.span.low) / (view.span.high - view.span.low || 1));

  const at = (x: number, y: number, t: number) =>
    project(frac(x, columns), frac(y, rows), t, azimuth, elevation);

  // Every vertex, so the fit sees the whole drawing before any of it is placed.
  const vertices: Array<{ x: number; y: number; t: number | undefined }> = [];
  for (let x = 0; x < columns; x++) {
    for (let y = 0; y < rows; y++) vertices.push({ x, y, t: lift(view.sample(x, y)) });
  }

  const corners = [
    at(0, 0, 0),
    at(columns - 1, 0, 0),
    at(columns - 1, rows - 1, 0),
    at(0, rows - 1, 0),
  ];
  // Each axis is named against whichever of its two parallel floor edges faces
  // the camera.
  //
  // Not the corner where the axis ends, and not a fixed edge: either choice has
  // angles at which the two names project to the same place or sit behind the
  // surface they belong to. The two nearest edges always meet at the nearest
  // corner, so they diverge at every angle and both stay in front.
  const nearer = (a: Projected, b: Projected) => (a.depth >= b.depth ? a : b);
  const labels = {
    x: nearer(
      project(0, -0.5, 0, azimuth, elevation),
      project(0, 0.5, 0, azimuth, elevation),
    ),
    y: nearer(
      project(-0.5, 0, 0, azimuth, elevation),
      project(0.5, 0, 0, azimuth, elevation),
    ),
    middle: project(0, 0, 0, azimuth, elevation),
  };
  const place = fitter(
    [...vertices.filter((v) => v.t != null).map((v) => at(v.x, v.y, v.t as number)), ...corners],
    box,
  );

  const vertex = (x: number, y: number) => {
    const t = vertices.find((v) => v.x === x && v.y === y)?.t;
    return t == null ? undefined : place(at(x, y, t));
  };

  const floor = corners.map(place);
  const floorPath =
    floor.map((p, i) => `${i === 0 ? "M" : "L"} ${p.px} ${p.py}`).join(" ") + " Z";

  const quads: Array<{ key: string; depth: number; d: string; fill: string }> = [];
  for (let x = 0; x < columns - 1; x++) {
    for (let y = 0; y < rows - 1; y++) {
      const square = [
        [x, y],
        [x + 1, y],
        [x + 1, y + 1],
        [x, y + 1],
      ] as const;
      const points = square.map(([cx, cy]) => vertex(cx, cy));
      // A quad needs all four corners measured; a partial one would be drawn
      // as a triangle claiming a shape nothing was simulated at.
      if (points.some((point) => point == null)) continue;
      const shades = square.map(([cx, cy]) => view.sampleColour(cx, cy));
      if (shades.some((value) => value == null)) continue;

      const mean = shades.reduce<number>((total, value) => total + (value ?? 0), 0) / 4;
      quads.push({
        key: `${x}:${y}`,
        // Painter's order follows the camera, not the grid, so a rotation
        // cannot leave a near quad hidden behind a far one.
        depth: at(x + 0.5, y + 0.5, 0).depth,
        d:
          points.map((p, i) => `${i === 0 ? "M" : "L"} ${p?.px ?? 0} ${p?.py ?? 0}`).join(" ") +
          " Z",
        fill: shade(view.colourSpan, mean),
      });
    }
  }
  quads.sort((a, b) => b.depth - a.depth);

  const planTop =
    view.planX != null && view.planY != null ? vertex(view.planX, view.planY) : undefined;
  const planFloor =
    view.planX != null && view.planY != null ? place(at(view.planX, view.planY, 0)) : undefined;

  return (
    <svg
      viewBox={`0 0 ${box.w} ${box.h}`}
      style={{ width: "100%", display: "block" }}
      role="img"
      aria-label={
        `${view.metric.label} as a surface over ${view.xAxis.label} and ${yAxis.label}` +
        (view.twoMeasures ? `, coloured by ${view.colour.label}` : "") +
        `, seen from ${Math.round(azimuth)}° at ${Math.round(elevation)}° above the horizon`
      }
    >
      <path
        d={floorPath}
        fill="var(--color-text)"
        fillOpacity={0.04}
        stroke="var(--color-text)"
        strokeOpacity={0.25}
        strokeDasharray="2 2"
      />
      {quads.map((quad) => (
        <path
          key={quad.key}
          d={quad.d}
          fill={quad.fill}
          stroke={GROUND}
          strokeWidth={0.8}
          strokeLinejoin="round"
        />
      ))}

      {planTop && planFloor && (
        <g pointerEvents="none">
          <title>the plan</title>
          <line
            x1={planTop.px}
            x2={planFloor.px}
            y1={planTop.py}
            y2={planFloor.py}
            stroke="var(--color-text)"
            strokeOpacity={0.5}
            strokeDasharray="2 2"
          />
          <circle
            cx={planTop.px}
            cy={planTop.py}
            r={4}
            fill={GROUND}
            stroke="var(--color-text)"
            strokeWidth={1.5}
          />
        </g>
      )}

      {/* Each axis named against the middle of its own floor edge, pushed
          outward from the centre, so the labels follow the rotation. */}
      <EdgeLabel anchor={place(labels.x)} middle={place(labels.middle)} text={view.xName} />
      <EdgeLabel anchor={place(labels.y)} middle={place(labels.middle)} text={view.yName ?? ""} />
    </svg>
  );
}

/** An axis name pushed clear of the floor, wherever the rotation has put it. */
function EdgeLabel({
  anchor,
  middle,
  text,
}: {
  anchor: { px: number; py: number };
  middle: { px: number; py: number };
  text: string;
}) {
  const dx = anchor.px - middle.px;
  const dy = anchor.py - middle.py;
  const length = Math.hypot(dx, dy) || 1;
  const push = 15;
  const x = anchor.px + (dx / length) * push;
  const y = anchor.py + (dy / length) * push;
  return (
    <text
      {...AXIS_TEXT}
      x={round(x)}
      y={round(y) + 3.5}
      textAnchor={dx > 6 ? "start" : dx < -6 ? "end" : "middle"}
    >
      {text}
    </text>
  );
}

function round(v: number): number {
  return Math.round(v * 10) / 10;
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

/**
 * What the picture's scales mean.
 *
 * A surface carrying two measures has to name both of them, and say which is
 * which: the reader is being asked to hold "high" and "dark" apart, and an
 * unlabelled ramp beside an unlabelled height is how that gets read as one
 * doubled-up encoding.
 */
export function GraphLegend({ view }: { view: GraphView }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        rowGap: 3,
        flexWrap: "wrap",
        fontSize: 10.5,
        color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
      }}
    >
      {view.twoMeasures && (
        <span style={{ display: "flex", alignItems: "center", gap: 5 }}>
          <i
            style={{
              width: 9,
              height: 9,
              display: "inline-block",
              borderLeft: "1.5px solid var(--color-text)",
              borderBottom: "1.5px solid var(--color-text)",
            }}
          />
          height {view.metric.tick(view.span.low)}–{view.metric.tick(view.span.high)}{" "}
          {view.metric.short}
        </span>
      )}
      <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span>{view.colour.tick(view.colourSpan.low)}</span>
        <div style={{ width: 110, display: "flex", height: 8 }}>
          {RAMP.map((step) => (
            <i key={step} style={{ flex: 1, background: step }} />
          ))}
        </div>
        <span>
          {view.colour.tick(view.colourSpan.high)} {view.colour.short}
        </span>
      </span>
    </div>
  );
}
