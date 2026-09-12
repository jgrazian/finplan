"use client";

import { useCallback, useEffect, useRef, type PointerEvent as ReactPointerEvent } from "react";
import { Blueprint } from "@/components/ui";
import {
  clampElevation,
  graphTitle,
  heldSummary,
  wrapAzimuth,
  type GraphSpec,
  type GraphView,
} from "@/lib/view/sweep";
import { GraphLegend, HeatGraph, LineGraph, SurfaceGraph } from "./SweepGraphs";

/** Degrees of turn and tilt a pixel of drag is worth. */
const TURN_PER_PX = 0.5;
const TILT_PER_PX = 0.4;

/**
 * How far the pointer must travel before it counts as a drag.
 *
 * Clicking a card selects it, and nobody presses a mouse button without moving
 * it a pixel or two. Below this the gesture is still a click, so selecting a
 * surface does not nudge it a degree on the way.
 */
const DRAG_THRESHOLD = 3;

/**
 * One graph on the workspace: a title, a picture, and what it is holding.
 *
 * Everything that changes what the graph *is* lives in the inspector rather
 * than on the card. A wall of four cards each carrying three menus is a control
 * panel with charts hidden in it. Where a graph is looked *at* from is the
 * exception, because turning a surface by dragging it is the same act as
 * turning a real object and no menu reads as that.
 */
export function GraphCard({
  view,
  position,
  selected,
  onSelect,
  onTurn,
}: {
  view: GraphView;
  /** Its place in the layout, which is how the inspector names it. */
  position: number;
  selected: boolean;
  onSelect: () => void;
  /** Only a surface has a camera to move; omitted, dragging just selects. */
  onTurn?: (azimuth: number, elevation: number) => void;
}) {
  const { title, sub } = graphTitle(view);
  const coloured = view.spec.kind !== "line";
  const turnable = view.spec.kind === "surface" && onTurn != null;

  // The gesture is held in refs, not state: a pointer moving across a card
  // should not re-render it on the way to deciding whether it is a drag.
  const from = useRef<{ x: number; y: number; azimuth: number; elevation: number } | null>(null);
  const latest = useRef<{ dx: number; dy: number } | null>(null);
  const frame = useRef<number | null>(null);
  const dragging = useRef(false);

  const flush = useCallback(() => {
    frame.current = null;
    const start = from.current;
    const move = latest.current;
    if (!start || !move || !onTurn) return;
    // Both axes follow the hand, which is what every orbit control does and
    // what grabbing a real object does: dragging right swings the near face of
    // the floor to the right, dragging down tips the top of it toward you.
    onTurn(
      wrapAzimuth(start.azimuth + move.dx * TURN_PER_PX),
      clampElevation(start.elevation + move.dy * TILT_PER_PX),
    );
  }, [onTurn]);

  const end = useCallback(() => {
    from.current = null;
    latest.current = null;
    dragging.current = false;
    if (frame.current != null) cancelAnimationFrame(frame.current);
    frame.current = null;
  }, []);

  // A pointer released outside the window never reaches the element, and a
  // gesture left open would resume on the next move across the card.
  useEffect(() => () => end(), [end]);

  const down = (event: ReactPointerEvent<HTMLDivElement>) => {
    onSelect();
    if (!turnable || event.button !== 0) return;
    // Otherwise the browser starts its own text-selection drag over the card.
    event.preventDefault();
    from.current = {
      x: event.clientX,
      y: event.clientY,
      azimuth: view.azimuth,
      elevation: view.elevation,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const move = (event: ReactPointerEvent<HTMLDivElement>) => {
    const start = from.current;
    if (!start) return;
    const dx = event.clientX - start.x;
    const dy = event.clientY - start.y;
    if (!dragging.current && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    dragging.current = true;
    latest.current = { dx, dy };
    // One update a frame. A pointer reports far faster than the page can
    // redraw, and every report would otherwise be a render of every card.
    if (frame.current == null) frame.current = requestAnimationFrame(flush);
  };

  return (
    <Blueprint
      className={selected ? "graph-card is-selected" : "graph-card"}
      style={{
        gridColumn: view.spec.wide ? "1 / -1" : "auto",
        padding: "10px 12px",
        display: "flex",
        flexDirection: "column",
        gap: 6,
        position: "relative",
      }}
    >
      {/* Selection, for the keyboard and for the card's own margins. It sits
          under the chart rather than over it, so the chart keeps its hover
          readouts and can run a gesture of its own. */}
      <button
        type="button"
        aria-pressed={selected}
        onClick={onSelect}
        style={{
          position: "absolute",
          inset: 0,
          background: "none",
          border: 0,
          padding: 0,
          cursor: "pointer",
        }}
      >
        <span className="sr-only">{`Select graph ${position}: ${title}, ${sub}`}</span>
      </button>

      <div style={{ display: "flex", alignItems: "baseline", gap: 8, position: "relative" }}>
        <span className="stat-l" style={{ minWidth: 14 }}>
          {position}
        </span>
        <h4 style={{ margin: 0, fontSize: 17 }}>
          {title}{" "}
          <span className="text-muted" style={{ fontSize: 12, letterSpacing: 0 }}>
            {sub}
          </span>
        </h4>
      </div>

      <div
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={end}
        onPointerCancel={end}
        style={{
          position: "relative",
          cursor: turnable ? "grab" : "pointer",
          // Keeps a drag from scrolling the page on a touch screen.
          touchAction: turnable ? "none" : undefined,
        }}
        title={turnable ? "Drag to turn" : undefined}
      >
        <Chart view={view} />
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          fontSize: 10.5,
          position: "relative",
          color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
        }}
      >
        {coloured && <GraphLegend view={view} />}
        <span style={{ marginLeft: "auto", textAlign: "right", textWrap: "pretty" }}>
          {heldSummary(view)}
        </span>
      </div>
    </Blueprint>
  );
}

function Chart({ view }: { view: GraphView }) {
  switch (view.spec.kind) {
    case "heatmap":
      return <HeatGraph view={view} />;
    case "surface":
      return <SurfaceGraph view={view} />;
    default:
      return <LineGraph view={view} />;
  }
}

/**
 * A graph whose variable this sweep does not carry.
 *
 * Only reachable for a moment — the layout is reconciled against every finished
 * sweep — but a frame that says why it is empty beats one that silently is.
 */
export function GraphGap({ spec, position }: { spec: GraphSpec; position: number }) {
  return (
    <Blueprint style={{ padding: "12px 14px", minHeight: 120 }}>
      <span className="stat-l">{position}</span>
      <p style={{ margin: "6px 0 0", fontSize: 12.5 }}>
        This graph is drawn over <Mono>{spec.x}</Mono>, which the current sweep
        does not carry. Add it back as a variable, or remove the graph.
      </p>
    </Blueprint>
  );
}

function Mono({ children }: { children: React.ReactNode }) {
  return (
    <span style={{ fontFamily: "ui-monospace, Menlo, monospace", fontSize: 12 }}>{children}</span>
  );
}
