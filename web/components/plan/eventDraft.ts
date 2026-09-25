/**
 * One event as the drawer edits it: the identity fields, the trigger draft and
 * the ordered effect drafts.
 *
 * The API replaces an event wholesale — a trigger is a tree and a partial merge
 * into one has no meaning — so the drawer holds the entire event and PUTs the
 * entire event. Which makes reading it back faithfully the whole game: see the
 * `raw` holds in `triggerDraft` and `effectDraft` for the shapes this form
 * carries through rather than redraws.
 */
import type { Event as ApiEvent, EventBody } from "@/lib/api/types";
import type { AmountNames } from "./amountDraft";
import {
  type EffectDraft,
  draftOfEffect,
  effectProblem,
  toEffectSpec,
} from "./effectDraft";
import {
  type TriggerDraft,
  draftOfTrigger,
  toTriggerSpec,
  triggerProblem,
} from "./triggerDraft";

export interface EventDraft {
  name: string;
  description: string;
  firesOnce: boolean;
  /** A disabled event stays in the plan and never fires. */
  enabled: boolean;
  trigger: TriggerDraft;
  effects: EffectDraft[];
}

/**
 * The saved event as a draft.
 *
 * `accountId` and `assetId` seed the pickers of anything added afterwards, so
 * a new condition or effect starts on a real row rather than on id 0.
 */
export function draftOfEvent(
  event: ApiEvent,
  accountId: number,
  assetId: number,
  names?: AmountNames,
): EventDraft {
  return {
    name: event.name,
    description: event.description ?? "",
    firesOnce: event.fires_once,
    enabled: event.enabled,
    trigger: draftOfTrigger(event.trigger, accountId, assetId),
    effects: event.effects.map((effect) => draftOfEffect(effect, accountId, assetId, names)),
  };
}

/** Which parts hold an edit — one entry per thing the footer can count. */
export interface EventChanges {
  name: boolean;
  description: boolean;
  firesOnce: boolean;
  enabled: boolean;
  trigger: boolean;
  /** How many effects were added, removed or altered. */
  effects: number;
}

/**
 * Triggers and effects are compared as the specs they will be sent as, not as
 * the drafts they are held as. A draft carries fields its current shape does
 * not use — the account a condition would watch if it became a balance one —
 * and typing in one of those is not an edit to the event.
 */
function sameSpec(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function changedFields(draft: EventDraft, pristine: EventDraft): EventChanges {
  const before = pristine.effects.map(toEffectSpec);
  const after = draft.effects.map(toEffectSpec);
  let effects = Math.abs(after.length - before.length);
  for (let i = 0; i < Math.min(after.length, before.length); i += 1) {
    if (!sameSpec(after[i], before[i])) effects += 1;
  }

  return {
    name: draft.name !== pristine.name,
    description: draft.description !== pristine.description,
    firesOnce: draft.firesOnce !== pristine.firesOnce,
    enabled: draft.enabled !== pristine.enabled,
    trigger: !sameSpec(toTriggerSpec(draft.trigger), toTriggerSpec(pristine.trigger)),
    effects,
  };
}

export function pendingCount(changed: EventChanges): number {
  const { effects, ...flags } = changed;
  return Object.values(flags).filter(Boolean).length + effects;
}

/**
 * Why this draft cannot be sent yet, or null when it can.
 *
 * The server validates properly — a duplicate name, an `Age` trigger with no
 * birth date, an effect pointing at a deleted account. This only catches the
 * blanks that would come back as a 400 naming a column rather than a field.
 */
export function eventProblem(draft: EventDraft): string | null {
  if (draft.name.trim() === "") return "An event needs a name.";
  return (
    triggerProblem(draft.trigger) ??
    draft.effects.map(effectProblem).find((p) => p != null) ??
    null
  );
}

export function toEventBody(draft: EventDraft): EventBody {
  return {
    name: draft.name.trim(),
    description: draft.description.trim() || null,
    fires_once: draft.firesOnce,
    enabled: draft.enabled,
    // Omitted, so the server leaves the row where the list already has it.
    sort_order: null,
    trigger: toTriggerSpec(draft.trigger),
    effects: draft.effects.map(toEffectSpec),
  };
}
