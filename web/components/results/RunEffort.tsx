"use client";

import { useId } from "react";
import { SectionHeading } from "@/components/ui";
import { fmtInt } from "@/lib/format";

/**
 * How hard the next run should work.
 *
 * `converge` turns `iterations` from the count into the minimum sample the
 * server takes before it starts testing whether the median has settled; it
 * keeps going, up to its own ceiling, until it has.
 */
export interface RunEffort {
  iterations: number;
  converge: boolean;
}

/**
 * The minimum sample a converging run takes before the median is first tested.
 *
 * Small enough that an already-stable plan stops early, large enough that the
 * first median it compares against is not noise.
 */
const CONVERGE_FLOOR = 500;

/**
 * The stops, coarse to fine and then open-ended.
 *
 * Iterations are a cost/precision dial rather than a plan parameter — the same
 * scenario at 100 and at 5,000 is the same scenario — so they are offered as a
 * handful of steps you slide between while looking at the results, not as a
 * number to type on the way in. The gaps widen because the precision does not:
 * halving the noise costs four times the iterations.
 */
export const EFFORT_STOPS: ReadonlyArray<RunEffort> = [
  { iterations: 100, converge: false },
  { iterations: 250, converge: false },
  { iterations: 1_000, converge: false },
  { iterations: 2_000, converge: false },
  { iterations: 5_000, converge: false },
  { iterations: CONVERGE_FLOOR, converge: true },
];

export function effortLabel(effort: RunEffort): string {
  return effort.converge ? "Converge" : fmtInt(effort.iterations);
}

/** The stop closest to a plain iteration count, for seeding from a preference. */
export function nearestStop(iterations: number): RunEffort {
  const fixed = EFFORT_STOPS.filter((stop) => !stop.converge);
  return fixed.reduce((best, stop) =>
    Math.abs(stop.iterations - iterations) < Math.abs(best.iterations - iterations)
      ? stop
      : best,
  );
}

function indexOf(effort: RunEffort): number {
  const found = EFFORT_STOPS.findIndex(
    (stop) => stop.converge === effort.converge && stop.iterations === effort.iterations,
  );
  return found === -1 ? 0 : found;
}

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
}: {
  value: RunEffort;
  onChange: (effort: RunEffort) => void;
  disabled?: boolean;
}) {
  const listId = useId();
  const index = indexOf(value);

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
        max={EFFORT_STOPS.length - 1}
        step={1}
        value={index}
        disabled={disabled}
        aria-label="Monte Carlo iterations"
        aria-valuetext={effortLabel(value)}
        onChange={(e) => onChange(EFFORT_STOPS[Number(e.target.value)])}
        style={{ width: "100%", display: "block" }}
      />
      {/* The stops are unevenly spaced in iterations but evenly spaced on the
          track, so the two ends are labelled and the rest read off the value
          above rather than as six numbers crowding a 296px rail. */}
      <datalist id={listId}>
        {EFFORT_STOPS.map((stop, i) => (
          <option key={effortLabel(stop)} value={i} label={effortLabel(stop)} />
        ))}
      </datalist>
      <div
        className="text-muted"
        style={{ display: "flex", justifyContent: "space-between", fontSize: 10.5 }}
      >
        <span>{effortLabel(EFFORT_STOPS[0])}</span>
        <span>{effortLabel(EFFORT_STOPS[EFFORT_STOPS.length - 1])}</span>
      </div>
    </div>
  );
}
