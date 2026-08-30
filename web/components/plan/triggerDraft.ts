/**
 * The trigger shapes the event form offers.
 *
 * `TriggerSpec` is a recursive tree — `And` of an `Or` of a `Repeating` whose
 * end condition is an `AccountBalance` is all legal. A form that exposed the
 * full grammar would be a tree editor, so this covers the shapes a plan is
 * actually built from and leaves the rest to the API. Events created elsewhere
 * still display correctly; they just cannot be round-tripped through this form.
 */
import type { Interval, TriggerSpec } from "@/lib/api/types";

export const TRIGGER_FORMS = ["Once on a date", "Once at an age", "Repeating"] as const;
export type TriggerForm = (typeof TRIGGER_FORMS)[number];

export const INTERVALS: Interval[] = [
  "Weekly",
  "BiWeekly",
  "Monthly",
  "Quarterly",
  "Yearly",
];

/** A bound of a repeating trigger: nothing, a date, or an age. */
export interface BoundDraft {
  kind: "none" | "date" | "age";
  date: string;
  age: string;
}

export const emptyBound: BoundDraft = { kind: "none", date: "", age: "" };

export interface TriggerDraft {
  form: TriggerForm;
  date: string;
  age: string;
  interval: Interval;
  start: BoundDraft;
  end: BoundDraft;
}

export const emptyTrigger: TriggerDraft = {
  form: "Repeating",
  date: "",
  age: "65",
  interval: "Monthly",
  start: emptyBound,
  end: emptyBound,
};

function bound(draft: BoundDraft): TriggerSpec | null {
  if (draft.kind === "date") return { kind: "Date", on_date: draft.date };
  if (draft.kind === "age") return { kind: "Age", years: Number(draft.age) || 0, months: null };
  return null;
}

export function toTriggerSpec(draft: TriggerDraft): TriggerSpec {
  switch (draft.form) {
    case "Once on a date":
      return { kind: "Date", on_date: draft.date };
    case "Once at an age":
      return { kind: "Age", years: Number(draft.age) || 0, months: null };
    case "Repeating":
      return {
        kind: "Repeating",
        interval: draft.interval,
        start_condition: bound(draft.start),
        end_condition: bound(draft.end),
        max_occurrences: null,
      };
  }
}
