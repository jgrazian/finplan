"use client";

import { type ReactNode, useId } from "react";
import { SectionHeading } from "@/components/ui";
import { effortLabel, effortStops, type RunEffort } from "@/lib/view/effort";

export { EFFORT_STOPS, effortLabel, nearestStop } from "@/lib/view/effort";
export type { RunEffort } from "@/lib/view/effort";

/**
 * The iteration count, as the rail's first section.
 *
 * It sits on the Results tab rather than with the scenario's parameters
 * because it is not one: it says how precisely to answer, not what to ask. In
 * the rail it reads against the run summary below it — the count you asked for
 * above the count the last run actually took — and above the Re-run button it
 * governs, so moving it and spending it are the same column.
 */
export function EffortPanel({
  value,
  onChange,
  disabled,
  maxIterations,
  note,
}: {
  value: RunEffort;
  onChange: (effort: RunEffort) => void;
  disabled?: boolean;
  /** The most iterations the account may ask for; the stops above it are not offered. */
  maxIterations?: number;
  /** Under the dial, e.g. what a free account would add. */
  note?: ReactNode;
}) {
  const listId = useId();
  const stops = effortStops(maxIterations);
  const found = stops.findIndex(
    (stop) => stop.converge === value.converge && stop.iterations === value.iterations,
  );
  const index = found === -1 ? 0 : found;

  return (
    <div>
      <SectionHeading
        className="mb-[10px]"
        action={
          <span style={{ fontSize: 12, fontVariantNumeric: "tabular-nums" }}>
            {effortLabel(value)}
          </span>
        }
      >
        Iterations
      </SectionHeading>

      <input
        className="input range"
        type="range"
        list={listId}
        min={0}
        max={stops.length - 1}
        step={1}
        value={index}
        disabled={disabled || stops.length < 2}
        aria-label="Monte Carlo iterations"
        aria-valuetext={effortLabel(value)}
        onChange={(e) => onChange(stops[Number(e.target.value)])}
        style={{ width: "100%", display: "block" }}
      />
      {/* The stops are unevenly spaced in iterations but evenly spaced on the
          track, so the two ends are labelled and the rest read off the value
          above rather than as six numbers crowding a 296px rail. */}
      <datalist id={listId}>
        {stops.map((stop, i) => (
          <option key={effortLabel(stop)} value={i} label={effortLabel(stop)} />
        ))}
      </datalist>
      <div
        className="text-muted"
        style={{ display: "flex", justifyContent: "space-between", fontSize: 10.5 }}
      >
        <span>{effortLabel(stops[0])}</span>
        <span>{effortLabel(stops[stops.length - 1])}</span>
      </div>
      {note && <div style={{ marginTop: 8, fontSize: 12 }}>{note}</div>}
    </div>
  );
}

/**
 * The same dial folded behind a button, for the phone: the rail it sits at the
 * top of on desktop is stacked below the chart there, so it rides beside the
 * success figure it governs instead. A `<details>` keeps it a plain disclosure
 * with no open/close state to hold.
 */
export function EffortMenu({
  value,
  onChange,
  disabled,
  maxIterations,
  note,
}: {
  value: RunEffort;
  onChange: (effort: RunEffort) => void;
  disabled?: boolean;
  maxIterations?: number;
  note?: ReactNode;
}) {
  return (
    <details className="effort-menu">
      <summary className="sbtn" aria-label="Monte Carlo iterations for the next run">
        {value.converge ? effortLabel(value) : `${effortLabel(value)} runs`}{" "}
        <span aria-hidden>▾</span>
      </summary>
      <div className="effort-menu-panel">
        <EffortPanel
          value={value}
          onChange={onChange}
          disabled={disabled}
          maxIterations={maxIterations}
          note={note}
        />
      </div>
    </details>
  );
}
