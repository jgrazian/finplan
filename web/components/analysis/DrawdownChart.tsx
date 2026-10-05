"use client";

import { useId, type KeyboardEvent } from "react";
import { ChartCanvas, EventMarkers } from "@/components/charts";
import { makeScale, barColumn, type ChartGeometry } from "@/components/charts/geometry";
import {
  CONVERSION_COLOR,
  CONVERSION_TAX_STRIPES,
  SURPLUS_COLOR,
  TAX_STRIPES,
  axisLabel,
  type DrawdownView,
} from "@/lib/view/drawdown";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/**
 * The stacked bars of Drawdown: one column per year, sources bottom-up, a
 * hatched cap for any shortfall up to the target line, and any surplus
 * (outlined) above it. The tax withheld on the sales hangs below zero, and
 * under it the tax on Roth conversions, in level bands. A converting year
 * gets a hollow marker at the amount converted. Hover selects a year, click
 * pins it.
 */
export function DrawdownChart({
  view,
  hover,
  onHover,
  pinned,
  onPin,
  narrow,
}: {
  view: DrawdownView;
  hover: number | null;
  onHover: (index: number | null) => void;
  pinned: number | null;
  onPin: (index: number | null) => void;
  narrow?: boolean;
}) {
  const id = useId().replace(/:/g, "");
  const hatch = `dd-hatch-${id}`;
  const stripes = `dd-tax-${id}`;
  const bands = `dd-conversion-tax-${id}`;
  const geo: ChartGeometry = narrow
    ? { w: 560, h: 340, left: 52, right: 8, top: 8, bottom: 28 }
    : { w: 920, h: 380, left: 60, right: 12, top: 8, bottom: 28 };
  const count = view.columns.length;
  const scale = makeScale(
    count,
    { min: view.min * 1.05, max: view.max * 1.05 || 1 },
    { geo, mode: "band" },
  );
  const color = new Map(view.series.map((s) => [s.key, s.color]));
  const active = hover ?? pinned;

  const target = view.columns
    .map((c, i) => (c.target == null ? null : `${i ? "L" : "M"}${scale.x(i).toFixed(1)} ${scale.y(c.target).toFixed(1)}`))
    .filter(Boolean)
    .join(" ");

  const onKey = (e: KeyboardEvent) => {
    if (count === 0) return;
    const at = active ?? 0;
    if (e.key === "ArrowRight") onPin(Math.min(count - 1, at + 1));
    else if (e.key === "ArrowLeft") onPin(Math.max(0, at - 1));
    else if (e.key === "Escape") onPin(null);
    else return;
    e.preventDefault();
  };

  return (
    <div
      role="group"
      tabIndex={0}
      aria-label="Spending and sources by year. Left and right arrow keys move between years; Escape clears the pin."
      onKeyDown={onKey}
      style={{ outlineOffset: 2 }}
    >
      <ChartCanvas
        scale={scale}
        years={view.years}
        format={axisLabel(view.unit)}
        hoverIndex={hover}
        onHoverChange={onHover}
        pinnedIndex={pinned}
        onSelect={(i) => onPin(i)}
        zeroLabel={
          view.hasConversion
            ? "Above: what paid for spending, and Roth conversions (hollow markers). Below: tax on withdrawals, then tax on conversions."
            : "Above: what paid for spending. Below: tax withheld on withdrawals."
        }
      >
        <defs>
          <pattern id={hatch} width={6} height={6} patternUnits="userSpaceOnUse" patternTransform="rotate(135)">
            <rect width={6} height={6} fill="var(--color-danger)" fillOpacity={0.08} />
            <line x1={0} y1={0} x2={0} y2={6} stroke="var(--color-danger)" strokeOpacity={0.45} strokeWidth={2} />
          </pattern>
          {/* The opposite slant to the shortfall hatch, and denser, so the two reds read apart. */}
          <pattern id={stripes} width={5} height={5} patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <rect width={5} height={5} fill="var(--color-danger)" fillOpacity={0.12} />
            <line x1={0} y1={0} x2={0} y2={5} stroke="var(--color-danger)" strokeOpacity={0.6} strokeWidth={2} />
          </pattern>
          {/* Level bands: tax all the same, but the conversion's, not the withdrawals'. */}
          <pattern id={bands} width={4} height={4} patternUnits="userSpaceOnUse">
            <rect width={4} height={4} fill="var(--color-danger)" fillOpacity={0.08} />
            <line x1={0} y1={0.75} x2={4} y2={0.75} stroke="var(--color-danger)" strokeOpacity={0.55} strokeWidth={1.5} />
          </pattern>
        </defs>
        {active != null && (
          <rect
            x={barColumn(active, scale).x - 1}
            y={geo.top}
            width={barColumn(active, scale).w + 2}
            height={scale.baseline - geo.top}
            fill="var(--color-accent)"
            fillOpacity={0.09}
            pointerEvents="none"
          />
        )}
        {view.columns.map((c, i) => {
          const { x, w } = barColumn(i, scale);
          // Either way from zero: the tax segment runs downward.
          const rect = (from: number, to: number) => ({
            x,
            width: w,
            y: Math.min(scale.y(from), scale.y(to)),
            height: Math.abs(scale.y(from) - scale.y(to)),
          });
          return (
            <g key={c.year} pointerEvents="none">
              {c.stack.map((s) => (
                <rect
                  key={s.key}
                  {...rect(s.from, s.to)}
                  fill={color.get(s.key)}
                  stroke="var(--color-raised)"
                  strokeWidth={0.75}
                />
              ))}
              {c.cap && (
                <rect
                  {...rect(c.cap.from, c.cap.to)}
                  fill={`url(#${hatch})`}
                  stroke="var(--color-danger)"
                  strokeWidth={1}
                  strokeDasharray="3 2"
                />
              )}
              {c.taxes && (
                <rect
                  {...rect(c.taxes.from, c.taxes.to)}
                  fill={`url(#${stripes})`}
                  stroke="var(--color-danger)"
                  strokeOpacity={0.7}
                  strokeWidth={1}
                />
              )}
              {c.conversionTax && (
                <rect
                  {...rect(c.conversionTax.from, c.conversionTax.to)}
                  fill={`url(#${bands})`}
                  stroke="var(--color-danger)"
                  strokeOpacity={0.7}
                  strokeWidth={1}
                  strokeDasharray="1 1.5"
                />
              )}
              {c.surplus && (
                <rect
                  {...rect(c.surplus.from, c.surplus.to)}
                  fill={SURPLUS_COLOR}
                  stroke="var(--color-text)"
                  strokeOpacity={0.55}
                  strokeWidth={1}
                  strokeDasharray="2 2"
                />
              )}
            </g>
          );
        })}
        {view.columns.map((c, i) => {
          if (c.rmd == null) return null;
          // A dash across the bar: per year, not joined, since it only
          // means something in the years an RMD falls due.
          const { x, w } = barColumn(i, scale);
          const y = scale.y(c.rmd);
          return (
            <line
              key={`rmd-${c.year}`}
              x1={x - 2}
              x2={x + w + 2}
              y1={y}
              y2={y}
              stroke="var(--color-text)"
              strokeWidth={1.5}
              strokeDasharray="3 2"
              pointerEvents="none"
            />
          );
        })}
        {view.columns.map((c, i) => {
          if (c.conversion == null) return null;
          // Hollow: money moved between accounts, not spent.
          const { x, w } = barColumn(i, scale);
          return (
            <circle
              key={`conversion-${c.year}`}
              cx={x + w / 2}
              cy={scale.y(c.conversion)}
              r={Math.max(2.5, Math.min(4.5, w / 2.5))}
              fill="var(--color-raised)"
              stroke={CONVERSION_COLOR}
              strokeWidth={1.75}
              pointerEvents="none"
            />
          );
        })}
        {target && (
          <path
            d={target}
            fill="none"
            stroke="var(--color-text)"
            strokeWidth={2.25}
            strokeLinejoin="round"
            pointerEvents="none"
          >
            <title>Target spend</title>
          </path>
        )}
        <EventMarkers markers={view.markers} scale={scale} />
      </ChartCanvas>
    </div>
  );
}

/** The key under the chart: the target line, each source, the special parts when present, then the tax. */
export function DrawdownLegend({ view }: { view: DrawdownView }) {
  // Last, since it is drawn apart from the bars, below zero; dollars only.
  const tax = view.unit === "usd" ? view.series.find((s) => s.kind === "taxes") : undefined;
  const swatch = (style: React.CSSProperties) => (
    <i aria-hidden style={{ width: 10, height: 10, display: "block", ...style }} />
  );
  return (
    <div
      style={{ display: "flex", flexWrap: "wrap", gap: "6px 14px", fontSize: 12, color: MUTED }}
    >
      {view.unit === "usd" && (
        <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <i aria-hidden style={{ width: 18, height: 2, background: "var(--color-text)", display: "block" }} />
          Target spend
        </span>
      )}
      {view.series
        .filter((s) => s.kind !== "taxes")
        .map((s) => (
          <span key={s.key} style={{ display: "flex", alignItems: "center", gap: 6 }}>
            {swatch({ background: s.color })}
            {s.name}
          </span>
        ))}
      {view.hasShortfall && (
        <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {swatch({
            border: "1px dashed var(--color-danger)",
            background:
              "repeating-linear-gradient(135deg, color-mix(in srgb, var(--color-danger) 40%, transparent) 0 2px, transparent 2px 5px)",
          })}
          Shortfall
        </span>
      )}
      {view.unit === "usd" && view.hasRmd && (
        <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <i
            aria-hidden
            style={{ width: 18, height: 0, borderTop: "1.5px dashed var(--color-text)", display: "block" }}
          />
          Required RMD, after tax
        </span>
      )}
      {view.hasSurplus && (
        <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {swatch({ background: SURPLUS_COLOR, border: "1px dashed color-mix(in srgb, var(--color-text) 55%, transparent)" })}
          Surplus (beyond need)
        </span>
      )}
      {view.unit === "usd" && view.hasConversion && (
        <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {swatch({ borderRadius: 999, border: `1.75px solid ${CONVERSION_COLOR}`, boxSizing: "border-box" })}
          Roth conversion
        </span>
      )}
      {tax && (
        <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {swatch({ background: TAX_STRIPES, border: "1px solid color-mix(in srgb, var(--color-danger) 70%, transparent)" })}
          {tax.name}
        </span>
      )}
      {view.unit === "usd" && view.hasConversion && view.lifetimeConversionTax > 0.5 && (
        <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {swatch({
            background: CONVERSION_TAX_STRIPES,
            border: "1px dotted color-mix(in srgb, var(--color-danger) 70%, transparent)",
          })}
          Tax on conversions
        </span>
      )}
    </div>
  );
}
