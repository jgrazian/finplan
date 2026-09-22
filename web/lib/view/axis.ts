/**
 * The plan's horizontal axis.
 *
 * Every screen that draws a horizon — the mini-timeline, the results chart —
 * plots against age. A scenario without a birth date has no age to plot, so the
 * axis falls back to calendar years and says so in its labels.
 */
import type { Scenario } from "@/lib/api/types";
import { addYears, yearsBetween } from "./format";

export interface PlanAxis {
  /** `age` when the scenario has a birth date, else `year`. */
  unit: "age" | "year";
  /** Inclusive [start, end] in the axis' own unit. */
  range: [number, number];
  /** ISO date → axis position. */
  at: (isoDate: string) => number;
  /** Axis position → label, e.g. `age 62` or `2043`. */
  label: (position: number) => string;
}

export function planAxis(scenario: Scenario): PlanAxis {
  const end = addYears(scenario.start_date, scenario.duration_years);
  const birth = scenario.birth_date;

  if (birth) {
    const at = (isoDate: string) => yearsBetween(birth, isoDate);
    return {
      unit: "age",
      range: [at(scenario.start_date), at(end)],
      at,
      label: (position) => `age ${position}`,
    };
  }

  const at = (isoDate: string) => Number(isoDate.slice(0, 4));
  return {
    unit: "year",
    range: [at(scenario.start_date), at(end)],
    at,
    label: (position) => String(position),
  };
}
