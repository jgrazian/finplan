import type { PlanEvent } from "@/lib/types";

/** Horizontal extent of the timeline track, in the strip's viewBox units. */
export interface TimelineScale {
  x0: number;
  x1: number;
  a0: number;
  a1: number;
  /** Age → x. */
  x: (age: number) => number;
}

export function makeTimelineScale(
  ageRange: [number, number],
  x0 = 148,
  x1 = 648,
): TimelineScale {
  const [a0, a1] = ageRange;
  return {
    x0,
    x1,
    a0,
    a1,
    x: (age) => x0 + ((age - a0) / (a1 - a0)) * (x1 - x0),
  };
}

/** A one-shot trigger draws as a point; anything with duration draws as a bar. */
export function isPointEvent(event: PlanEvent): boolean {
  return event.span[0] === event.span[1];
}

export interface TimelineMark {
  id: string;
  isPoint: boolean;
  /** Bar geometry. */
  x: number;
  w: number;
  /** Lane centre-line. */
  y: number;
}

/**
 * Lays events into `lanes` rotating rows so overlapping spans stay legible in
 * a strip too short to give each event its own line.
 */
export function layoutMarks(
  events: PlanEvent[],
  scale: TimelineScale,
  lanes = 3,
  laneTop = 14,
  laneHeight = 11,
): TimelineMark[] {
  return events.map((e, i) => {
    const x1 = scale.x(e.span[0]);
    const x2 = scale.x(e.span[1]);
    return {
      id: e.id,
      isPoint: isPointEvent(e),
      x: x1,
      w: Math.max(x2 - x1, 2),
      y: laneTop + (i % lanes) * laneHeight,
    };
  });
}

export interface TimelineTick {
  age: number;
  x: number;
  label: string;
}

/** Milestone ages, labelled by what happens there rather than by number. */
export function milestoneTicks(scale: TimelineScale): TimelineTick[] {
  const MILESTONES: Array<[number, string]> = [
    [45, "age 45"],
    [62, "retire"],
    [67, "SS"],
    [75, "RMD"],
    [80, "80"],
  ];
  return MILESTONES.map(([age, label]) => ({ age, x: scale.x(age), label }));
}
