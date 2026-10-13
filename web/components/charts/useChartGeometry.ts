"use client";

import { useLayoutEffect, useMemo, useState } from "react";
import { type ChartGeometry, DEFAULT_GEOMETRY } from "./geometry";

/**
 * The most a chart's drawing is scaled up to fill its frame: about what the
 * widest frame drew at before the sheet could grow past 1440px.
 */
const MAX_SCALE = 1.16;

/** Anything a geometry with a width can describe. */
type Frame = { w: number };

/**
 * A chart's geometry for the width it is drawn at.
 *
 * A viewBox scales its text with its marks, so a chart stretched across a
 * wide screen would draw its 10px ticks at 20px and stand twice as tall. Up
 * to `maxScale` the drawing is simply scaled, as it always was; past it the
 * viewBox widens instead, so the extra width becomes plot at the same scale.
 * The ref goes on what the drawing fills: the chart's `<svg>` (ChartCanvas's
 * `measureRef`), or a column of charts that share one width. It is a callback
 * ref, so an element that appears after the first render is measured too.
 */
export function useChartGeometry<G extends Frame = ChartGeometry, E extends Element = SVGSVGElement>(
  base: G = DEFAULT_GEOMETRY as unknown as G,
  maxScale = MAX_SCALE,
): [(el: E | null) => void, G] {
  const [el, setEl] = useState<E | null>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    if (!el) return;
    // It reports the starting size too, before that frame is painted.
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    observer.observe(el);
    return () => observer.disconnect();
  }, [el]);
  const wide = width / base.w > maxScale ? Math.round(width / maxScale) : null;
  return [setEl, useMemo(() => (wide == null ? base : { ...base, w: wide }), [base, wide])];
}
