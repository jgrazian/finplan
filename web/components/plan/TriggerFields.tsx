"use client";

import type { ReactNode } from "react";
import { Tag, type DropdownOption } from "@/components/ui";
import type { Account, Asset, Event as ApiEvent, NamedParameter } from "@/lib/api/types";
import { eventRefs } from "@/lib/view/refs";
import { FAMILY, type TriggerDraft } from "./triggerDraft";

export interface TriggerContext {
  accounts: Account[];
  assets: Asset[];
  events: ApiEvent[];
  parameters?: NamedParameter[];
  scenarioId?: number;
  /** The scenario's, for resolving an age to the date it lands on. */
  birthDate?: string;
  /** The event being edited, so it cannot anchor or be listed against itself. */
  selfId?: number;
}

/**
 * The three menus that name a row of the plan.
 *
 * The secondary column carries what tells two similarly named rows apart — an
 * account's flavour, an asset's description — which is the thing the native
 * popup had no room for. Nothing stands in for "none": an id matching no row
 * leaves the trigger on its placeholder, so an account deleted out from under a
 * condition reads as unset rather than as a stale name.
 */
export function accountOptions(accounts: Account[]): DropdownOption<number>[] {
  return accounts.map((a) => ({ value: a.id, label: a.name, detail: a.flavor }));
}

export function assetOptions(assets: Asset[]): DropdownOption<number>[] {
  return assets.map((a) => ({
    value: a.id,
    label: a.name,
    detail: a.description ?? undefined,
  }));
}

/** Every event but the one being edited — nothing anchors to itself. */
export function eventOptions(events: ApiEvent[], selfId?: number): DropdownOption<number>[] {
  return events
    .filter((e) => e.id !== selfId)
    .map((e) => ({ value: e.id, label: e.name }));
}

export function parameterOptions(parameters: NamedParameter[] | undefined, kind: "Date" | "Age"): DropdownOption<number>[] {
  return (parameters ?? [])
    .filter((parameter) => parameter.value.kind === kind)
    .map((parameter) => ({ value: parameter.id, label: parameter.name }));
}

export function Note({ children }: { children: ReactNode }) {
  return (
    <p
      style={{
        margin: 0,
        fontSize: 12,
        lineHeight: 1.45,
        color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
      }}
    >
      {children}
    </p>
  );
}

/** What shape the trigger is, in one word, beside its name. */
export function TriggerFamily({ trigger }: { trigger: TriggerDraft }) {
  if (trigger.raw) return <Tag tone="neutral">advanced</Tag>;
  const family = FAMILY[trigger.form];
  return <Tag tone={family.tone}>{family.label}</Tag>;
}

/** A `Manual` trigger has no terms — only the events that reach it. */
export function ManualNote({ context }: { context: TriggerContext }) {
  const { triggers } =
    context.selfId != null
      ? eventRefs(context.selfId, context.events)
      : { triggers: [] as ApiEvent[] };

  return (
    <Note>
      {triggers.length === 0 ? (
        <>
          Never fires. Nothing in this plan reaches it — give some event a{" "}
          <em>TriggerEvent</em> effect pointing here.
        </>
      ) : (
        <>
          Fired only by a <em>TriggerEvent</em> effect — from{" "}
          {triggers.map((e, i) => (
            <span key={e.id}>
              {i > 0 && ", "}
              <strong style={{ fontWeight: 600 }}>{e.name}</strong>
            </span>
          ))}
          .
        </>
      )}
    </Note>
  );
}
