/**
 * The trigger shapes the event form offers, and their translation to
 * `TriggerSpec`.
 *
 * `TriggerSpec` is a recursive tree — an `And` of an `Or` of a `Repeating`
 * whose end condition is an `AccountBalance` is all legal, and a form that
 * exposed that would be a tree editor. So this covers every leaf condition the
 * engine has, a repeating schedule bounded by any two of them, and one level of
 * all-of / any-of over them. Deeper nesting — and `Manual`, which fires only
 * when another event's effect reaches it — the API still accepts; events built
 * that way still display, they just cannot be round-tripped through this form.
 */
import type { Comparison, Interval, OffsetUnit, TriggerSpec } from "@/lib/api/types";

export const TRIGGER_FORMS = [
  "Once on a date",
  "Once at an age",
  "Relative to another event",
  "When an account balance crosses",
  "When a holding crosses",
  "When net worth crosses",
  "Repeating",
  "All of",
  "Any of",
] as const;
export type TriggerForm = (typeof TRIGGER_FORMS)[number];

export const INTERVALS: Interval[] = [
  "Weekly",
  "BiWeekly",
  "Monthly",
  "Quarterly",
  "Yearly",
];

export const OFFSET_UNITS: OffsetUnit[] = ["Days", "Months", "Years"];

/** The comparisons a threshold reads with, as the condition says them aloud. */
export const COMPARISONS: { value: Comparison; label: string }[] = [
  { value: "GreaterThanOrEqual", label: "reaches or passes" },
  { value: "LessThanOrEqual", label: "falls to or below" },
];

/* ── conditions ─────────────────────────────────────────────────────────── */

/**
 * One condition: a leaf of the trigger tree. The same draft serves the whole
 * form — the single-condition triggers, the members of a group, and a
 * repeating schedule's two bounds — so switching a trigger's shape keeps
 * whatever was already typed into it.
 */
export const CONDITION_FORMS = [
  "on a date",
  "at an age",
  "relative to an event",
  "on an account balance",
  "on a holding",
  "on net worth",
] as const;
export type ConditionForm = (typeof CONDITION_FORMS)[number];

export interface ConditionDraft {
  form: ConditionForm;
  date: string;
  age: number;
  eventId: number;
  unit: OffsetUnit;
  offset: number;
  accountId: number;
  assetId: number;
  comparison: Comparison;
  threshold: number;
}

export function emptyCondition(accountId: number, assetId: number): ConditionDraft {
  return {
    form: "on a date",
    date: "",
    age: 65,
    eventId: 0,
    unit: "Years",
    offset: 1,
    accountId,
    assetId,
    comparison: "GreaterThanOrEqual",
    threshold: 0,
  };
}

/** The condition each single-condition trigger is: everything but the groups. */
const LEAF: Partial<Record<TriggerForm, ConditionForm>> = {
  "Once on a date": "on a date",
  "Once at an age": "at an age",
  "Relative to another event": "relative to an event",
  "When an account balance crosses": "on an account balance",
  "When a holding crosses": "on a holding",
  "When net worth crosses": "on net worth",
};

export function leafOf(form: TriggerForm): ConditionForm | null {
  return LEAF[form] ?? null;
}

export function isGroup(form: TriggerForm): boolean {
  return form === "All of" || form === "Any of";
}

/* ── the draft ──────────────────────────────────────────────────────────── */

/** A repeating trigger's start or end: a condition, or nothing at all. */
export interface BoundDraft {
  on: boolean;
  condition: ConditionDraft;
}

export interface TriggerDraft {
  form: TriggerForm;
  /** What the single-condition forms edit. */
  condition: ConditionDraft;
  interval: Interval;
  maxOccurrences: number | null;
  start: BoundDraft;
  end: BoundDraft;
  /** What an all-of / any-of group holds. */
  children: ConditionDraft[];
}

export function emptyTrigger(accountId: number, assetId: number): TriggerDraft {
  const condition = emptyCondition(accountId, assetId);
  return {
    form: "Repeating",
    condition,
    interval: "Monthly",
    maxOccurrences: null,
    start: { on: false, condition },
    end: { on: false, condition },
    children: [],
  };
}

/**
 * Changes a draft's shape, carrying what was typed across where it still
 * means something: the condition follows the single-condition forms, and a
 * group starts out holding the condition the form was showing a moment ago.
 */
export function withForm(draft: TriggerDraft, form: TriggerForm): TriggerDraft {
  const leaf = LEAF[form];
  return {
    ...draft,
    form,
    condition: leaf ? { ...draft.condition, form: leaf } : draft.condition,
    children:
      isGroup(form) && draft.children.length === 0 ? [draft.condition] : draft.children,
  };
}

/* ── to the wire ────────────────────────────────────────────────────────── */

export function toConditionSpec(condition: ConditionDraft): TriggerSpec {
  const { comparison, threshold } = condition;
  switch (condition.form) {
    case "on a date":
      return { kind: "Date", on_date: condition.date };
    case "at an age":
      return { kind: "Age", years: condition.age, months: null };
    case "relative to an event":
      return {
        kind: "RelativeToEvent",
        event_id: condition.eventId,
        unit: condition.unit,
        value: condition.offset,
      };
    case "on an account balance":
      return { kind: "AccountBalance", account_id: condition.accountId, comparison, threshold };
    case "on a holding":
      return {
        kind: "AssetBalance",
        account_id: condition.accountId,
        asset_id: condition.assetId,
        comparison,
        threshold,
      };
    case "on net worth":
      return { kind: "NetWorth", comparison, threshold };
  }
}

function boundSpec(bound: BoundDraft): TriggerSpec | null {
  return bound.on ? toConditionSpec(bound.condition) : null;
}

export function toTriggerSpec(draft: TriggerDraft): TriggerSpec {
  const leaf = LEAF[draft.form];
  if (leaf) return toConditionSpec({ ...draft.condition, form: leaf });

  switch (draft.form) {
    case "All of":
      return { kind: "And", children: draft.children.map(toConditionSpec) };
    case "Any of":
      return { kind: "Or", children: draft.children.map(toConditionSpec) };
    // Everything else is Repeating.
    default:
      return {
        kind: "Repeating",
        interval: draft.interval,
        start_condition: boundSpec(draft.start),
        end_condition: boundSpec(draft.end),
        max_occurrences: draft.maxOccurrences,
      };
  }
}

/* ── what cannot be sent yet ────────────────────────────────────────────── */

/**
 * A blank date or an unpicked account would come back as a 400 or a foreign
 * key error; saying so here saves the round trip, the way `effectProblem`
 * does for the effects below it.
 */
function conditionProblem(condition: ConditionDraft): string | null {
  switch (condition.form) {
    case "on a date":
      return condition.date === "" ? "needs a date" : null;
    case "at an age":
      return condition.age > 0 ? null : "needs an age";
    case "relative to an event":
      return condition.eventId === 0 ? "needs an event to measure from" : null;
    case "on an account balance":
      return condition.accountId === 0 ? "needs an account" : null;
    case "on a holding":
      if (condition.accountId === 0) return "needs an account";
      return condition.assetId === 0 ? "needs an asset" : null;
    case "on net worth":
      return null;
  }
}

export function triggerProblem(draft: TriggerDraft): string | null {
  const say = (where: string, problem: string) => `Trigger: ${where} ${problem}.`;

  if (leafOf(draft.form)) {
    const problem = conditionProblem(draft.condition);
    return problem && say("the condition", problem);
  }

  if (isGroup(draft.form)) {
    if (draft.children.length === 0) {
      return `Trigger: ${draft.form.toLowerCase()} needs at least one condition.`;
    }
    for (const [i, child] of draft.children.entries()) {
      const problem = conditionProblem(child);
      if (problem) return say(`condition ${i + 1}`, problem);
    }
    return null;
  }

  if (draft.form === "Repeating") {
    for (const [where, bound] of [
      ["the start condition", draft.start],
      ["the end condition", draft.end],
    ] as const) {
      const problem = bound.on ? conditionProblem(bound.condition) : null;
      if (problem) return say(where, problem);
    }
  }

  return null;
}
