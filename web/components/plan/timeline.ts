import type { PlanEvent } from "@/lib/types";
import { ON_CONDITION } from "@/lib/view/events";

/** A one-shot trigger draws as a point; anything with duration draws as a bar. */
export function isPointEvent(event: PlanEvent): boolean {
  return event.span[0] === event.span[1];
}

/**
 * Whether the event's dates are knowable at all.
 *
 * A balance-driven trigger has no fire date until the run produces one, so its
 * span is the whole horizon rather than a measured stretch. The band draws
 * those dashed: a solid bar across the plan would claim it fires throughout,
 * which is a different statement from "somewhere in here, if the balance goes".
 */
export function isUndated(event: PlanEvent): boolean {
  return event.next === ON_CONDITION;
}

/** One event's lane geometry, as percentages across the band's track. */
export interface Lane {
  id: string;
  isPoint: boolean;
  undated: boolean;
  left: number;
  /** Zero for a point, which draws as a dot at `left` instead. */
  width: number;
}

export function laneOf(event: PlanEvent, [a0, a1]: [number, number]): Lane {
  // A one-year horizon still has to divide by something.
  const span = Math.max(a1 - a0, 1);
  const at = (position: number) =>
    Math.min(100, Math.max(0, ((position - a0) / span) * 100));
  const left = at(event.span[0]);
  const right = at(event.span[1]);
  return {
    id: event.id,
    isPoint: isPointEvent(event),
    undated: isUndated(event),
    left,
    width: Math.max(right - left, 0),
  };
}

export interface AxisTick {
  position: number;
  label: string;
}

/**
 * Evenly spaced ticks across the plan's horizon.
 *
 * The axis is the scenario's own — ages when it has a birth date, calendar
 * years when it does not — so the labels come from the caller rather than from
 * a fixed list of retirement milestones. The band lays them out with
 * `space-between`, so only the labels are needed, not their positions.
 */
export function axisTicks(
  [a0, a1]: [number, number],
  label: (position: number) => string,
  count = 5,
): AxisTick[] {
  const span = a1 - a0;
  const steps = Math.max(1, Math.min(count - 1, span));
  return Array.from({ length: steps + 1 }, (_, i) => {
    const position = Math.round(a0 + (span * i) / steps);
    return { position, label: label(position) };
  });
}
