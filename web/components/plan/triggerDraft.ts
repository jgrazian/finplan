/**
 * The trigger shapes the event form offers, and their translation to
 * `TriggerSpec`.
 *
 * `TriggerSpec` is a recursive tree — an `And` of an `Or` of a `Repeating`
 * whose end condition is an `AccountBalance` is all legal, and a form that
 * exposed that would be a tree editor. So this covers every leaf condition the
 * engine has, a repeating schedule bounded by any two of them, one level of
 * all-of / any-of over them, and `Manual`. Deeper nesting the API still
 * accepts, and `draftOf` reads such a trigger into a draft that holds it
 * verbatim: the editor shows it as prose and sends it back unchanged, so
 * renaming an event built elsewhere cannot flatten its trigger.
 */
import type { TagTone } from "@/components/ui/Tag";
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
  "Only when another event fires it",
] as const;
export type TriggerForm = (typeof TRIGGER_FORMS)[number];

/**
 * What each shape is, as a tag beside its name.
 *
 * Ten kinds, four shapes: a single value in time, an account and a threshold,
 * a list of nested conditions, or nothing at all. The tag says which before the
 * fields do.
 */
export const FAMILY: Record<TriggerForm, { label: string; tone: TagTone }> = {
  "Once on a date": { label: "time", tone: "outline" },
  "Once at an age": { label: "time", tone: "outline" },
  "Relative to another event": { label: "time", tone: "outline" },
  "When an account balance crosses": { label: "balance", tone: "accent" },
  "When a holding crosses": { label: "balance", tone: "accent" },
  "When net worth crosses": { label: "balance", tone: "accent" },
  "All of": { label: "compound", tone: "neutral" },
  "Any of": { label: "compound", tone: "neutral" },
  Repeating: { label: "scheduled", tone: "neutral" },
  "Only when another event fires it": { label: "no terms", tone: "neutral" },
};

export const INTERVALS: Interval[] = [
  "Weekly",
  "BiWeekly",
  "Monthly",
  "Quarterly",
  "Yearly",
];

export const OFFSET_UNITS: OffsetUnit[] = ["Days", "Months", "Years"];

/** Which side of the threshold fires it. One pair, reused by all three. */
export const COMPARISONS: { value: Comparison; label: string }[] = [
  { value: "GreaterThanOrEqual", label: "Above" },
  { value: "LessThanOrEqual", label: "Below" },
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
  /** Null is the birthday itself; the engine reads whole months past it. */
  ageMonths: number | null;
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
    ageMonths: null,
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

export function isManual(form: TriggerForm): boolean {
  return form === "Only when another event fires it";
}

/* ── the draft ──────────────────────────────────────────────────────────── */

/** A repeating trigger's start or end: a condition, or nothing at all. */
export interface BoundDraft {
  on: boolean;
  condition: ConditionDraft;
}

export interface TriggerDraft {
  form: TriggerForm;
  /**
   * A trigger this form cannot express, held exactly as the server sent it.
   * Set, it is what gets written back and `form` means nothing; the editor
   * shows the trigger as prose until something clears it.
   */
  raw?: TriggerSpec;
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
    // Picking a shape is the one way to give up a trigger the form could not
    // express; until then it is carried through untouched.
    raw: undefined,
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
      return { kind: "Age", years: condition.age, months: condition.ageMonths };
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
  // A trigger the form never modelled goes back exactly as it arrived.
  if (draft.raw) return draft.raw;

  const leaf = LEAF[draft.form];
  if (leaf) return toConditionSpec({ ...draft.condition, form: leaf });

  switch (draft.form) {
    case "All of":
      return { kind: "And", children: draft.children.map(toConditionSpec) };
    case "Any of":
      return { kind: "Or", children: draft.children.map(toConditionSpec) };
    case "Only when another event fires it":
      return { kind: "Manual" };
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
  // Untouched since the server sent it, so it is as valid as it ever was.
  if (draft.raw) return null;

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

/* ── from the wire ──────────────────────────────────────────────────────── */

/** The shape each single-condition trigger is edited as: `LEAF`, reversed. */
const FORM_OF: Record<ConditionForm, TriggerForm> = {
  "on a date": "Once on a date",
  "at an age": "Once at an age",
  "relative to an event": "Relative to another event",
  "on an account balance": "When an account balance crosses",
  "on a holding": "When a holding crosses",
  "on net worth": "When net worth crosses",
};

/**
 * One saved condition as a draft, or null where the form cannot hold it.
 *
 * Null is not a failure — it is how a trigger the editor does not model gets
 * routed to `raw` instead of being quietly rewritten into something simpler:
 * a group nested inside a group, a schedule bounded by another schedule.
 */
function conditionOf(spec: TriggerSpec, base: ConditionDraft): ConditionDraft | null {
  switch (spec.kind) {
    case "Date":
      return { ...base, form: "on a date", date: spec.on_date };
    case "Age":
      return { ...base, form: "at an age", age: spec.years, ageMonths: spec.months };
    case "RelativeToEvent":
      return {
        ...base,
        form: "relative to an event",
        eventId: spec.event_id,
        unit: spec.unit,
        offset: spec.value,
      };
    case "AccountBalance":
      return {
        ...base,
        form: "on an account balance",
        accountId: spec.account_id,
        comparison: spec.comparison,
        threshold: spec.threshold,
      };
    case "AssetBalance":
      return {
        ...base,
        form: "on a holding",
        accountId: spec.account_id,
        assetId: spec.asset_id,
        comparison: spec.comparison,
        threshold: spec.threshold,
      };
    case "NetWorth":
      return {
        ...base,
        form: "on net worth",
        comparison: spec.comparison,
        threshold: spec.threshold,
      };
    default:
      return null;
  }
}

/**
 * A saved trigger as a draft the editor can open.
 *
 * Anything the form models comes back as its own shape, so reopening an event
 * shows the fields it was built from. Anything it does not — a group of groups,
 * a repeating schedule bounded by another repeating one — comes back in `raw`,
 * which the editor renders as prose and writes back byte for byte.
 */
export function draftOfTrigger(
  trigger: TriggerSpec,
  accountId: number,
  assetId: number,
): TriggerDraft {
  const base = emptyTrigger(accountId, assetId);
  const blank = base.condition;
  const raw = { ...base, raw: trigger };

  const leaf = conditionOf(trigger, blank);
  if (leaf) return { ...base, form: FORM_OF[leaf.form], condition: leaf };

  switch (trigger.kind) {
    case "Manual":
      return { ...base, form: "Only when another event fires it" };

    case "And":
    case "Or": {
      const children = trigger.children.map((child) => conditionOf(child, blank));
      // One child the form cannot hold makes the whole group unrepresentable:
      // a half-read group would silently drop it on the next save.
      if (children.length === 0 || children.some((c) => c == null)) return raw;
      return {
        ...base,
        form: trigger.kind === "And" ? "All of" : "Any of",
        children: children as ConditionDraft[],
      };
    }

    case "Repeating": {
      const bound = (spec: TriggerSpec | null): BoundDraft | null => {
        if (!spec) return { on: false, condition: blank };
        const condition = conditionOf(spec, blank);
        return condition ? { on: true, condition } : null;
      };
      const start = bound(trigger.start_condition);
      const end = bound(trigger.end_condition);
      if (!start || !end) return raw;
      return {
        ...base,
        form: "Repeating",
        interval: trigger.interval,
        maxOccurrences: trigger.max_occurrences,
        start,
        end,
      };
    }

    default:
      return raw;
  }
}

/* ── changing shape ─────────────────────────────────────────────────────── */

/**
 * What converting this trigger abandons, said before it commits — the same
 * contract the account drawer's kind field keeps.
 *
 * Only the terms that actually hold something are named. Switching between two
 * threshold conditions carries the account and the figure across, so there is
 * nothing to warn about and the drawer stays quiet.
 */
export function triggerConversion(
  pristine: TriggerDraft,
  form: TriggerForm,
): string | null {
  if (pristine.raw) {
    return form === pristine.form
      ? null
      : `Replaces the trigger saved for this event, which is nested deeper than this form draws.`;
  }
  if (pristine.form === form) return null;

  if (pristine.form === "Repeating" && form !== "Repeating") {
    const held = [
      pristine.start.on && "the start condition",
      pristine.end.on && "the end condition",
      pristine.maxOccurrences != null && "the repeat limit",
    ].filter((x): x is string => typeof x === "string");
    const parts = [...held, "the interval"];
    const list =
      parts.length === 1
        ? parts[0]
        : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
    return `Repeating → ${form} discards ${list}. The event fires once.`;
  }

  if (isGroup(pristine.form) && !isGroup(form) && pristine.children.length > 1) {
    return `${pristine.form} → ${form} keeps the first condition and discards the other ${pristine.children.length - 1}.`;
  }

  if (isManual(form)) {
    return "Manual fires only when another event's TriggerEvent effect reaches it. The condition is discarded.";
  }

  return null;
}
