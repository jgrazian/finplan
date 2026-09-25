"use client";

import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import {
  Blueprint,
  Button,
  CompactInput,
  DirtyField,
  Field,
  Hr,
  InlineStat,
  SectionHeading,
  Tag,
  cx,
} from "@/components/ui";
import type { EffectSpec, Event as ApiEvent } from "@/lib/api/types";
import { fmtClock } from "@/lib/format";
import type { PlanEvent } from "@/lib/types";
import { eventRefs } from "@/lib/view/refs";
import { namesOf } from "@/lib/view/events";
import { EffectsBlock } from "./EffectFields";
import { renameAmountReferences, renameParameterReferences } from "./amountDraft";
import {
  Note,
  ScheduleFields,
  type TriggerContext,
  TriggerFamily,
  TriggerFields,
  TriggerFormSelect,
  hasSchedule,
} from "./TriggerFields";
import { isManual, triggerConversion } from "./triggerDraft";
import {
  type EventDraft,
  changedFields,
  draftOfEvent,
  pendingCount,
} from "./eventDraft";

export type { EventDraft } from "./eventDraft";

const MUTED = "color-mix(in srgb, var(--color-text) 55%, transparent)";

function renameSavedEffect(effect: EffectSpec, oldName: string, newName: string): EffectSpec {
  if (effect.kind === "Random") return {
    ...effect,
    on_true: renameSavedEffect(effect.on_true, oldName, newName),
    on_false: effect.on_false ? renameSavedEffect(effect.on_false, oldName, newName) : null,
  };
  if ("amount" in effect) return {
    ...effect, amount: renameAmountReferences(effect.amount, oldName, newName),
  };
  return effect;
}

/**
 * Artboard 10a — the event editor, two columns wide: when it fires on the
 * left, what it does on the right, identity across the top of both.
 *
 * The blocks themselves are unchanged from the drawer of artboard 8, and so is
 * their contract: order never changes, a block is present or absent, never
 * moved, and only the trigger's terms and the selected effect's terms vary in
 * content. A term the engine does not read for the chosen kind is absent, not
 * disabled — no greyed-out lot method on an Expense. What the wider column
 * buys is that the polymorphic halves sit beside each other instead of one
 * scrolling the other off a 400px rail.
 *
 * Mount with a `key` of the event id so switching rows starts a fresh draft
 * rather than carrying edits across.
 */
export function EventEditor({
  event,
  raw,
  context,
  onApply,
  onDuplicate,
  onDelete,
  onSelectEvent,
  busy,
  error,
  savedAt,
  offline,
}: {
  /** The view model, for what the draft cannot say: when it next fires. */
  event: PlanEvent;
  /** The row the draft is read from and written back to. */
  raw: ApiEvent;
  /** Everything the pickers choose between. */
  context: TriggerContext;
  onApply: (draft: EventDraft) => void;
  /** Copies the saved event, not the draft — see the note on the button. */
  onDuplicate?: () => void;
  onDelete?: () => void;
  /** Opening one of the events that drives this one. */
  onSelectEvent?: (name: string) => void;
  busy?: boolean;
  error?: string;
  /** When this event was last saved from this session, for the resting line. */
  savedAt?: number;
  /** Writes are being refused, so nothing here can be saved. */
  offline?: boolean;
}) {
  const seed = () =>
    draftOfEvent(raw, context.accounts[0]?.id ?? 0, context.assets[0]?.id ?? 0, namesOf(context));
  const [draft, setDraft] = useState<EventDraft>(seed);
  const previousParameters = useRef(new Map((context.parameters ?? []).map((p) => [p.id, p.name])));
  useEffect(() => {
    const next = new Map((context.parameters ?? []).map((p) => [p.id, p.name]));
    const renames = [...next].flatMap(([id, name]) => {
      const previous = previousParameters.current.get(id);
      return previous && previous !== name ? [[previous, name] as const] : [];
    });
    previousParameters.current = next;
    if (renames.length === 0) return;
    setDraft((current) => ({
      ...current,
      effects: current.effects.map((effect) => {
        const rename = (source: string) => renames.reduce(
          (text, [oldName, newName]) => renameParameterReferences(text, oldName, newName), source,
        );
        return {
          ...effect,
          ...(effect.rawAmount?.kind === "Expression"
            ? { rawAmount: { kind: "Expression" as const, source: rename(effect.rawAmount.source) } }
            : {}),
          ...(effect.expressionDraft ? { expressionDraft: rename(effect.expressionDraft) } : {}),
          ...(effect.raw ? { raw: renames.reduce(
            (saved, [oldName, newName]) => renameSavedEffect(saved, oldName, newName), effect.raw,
          ) } : {}),
        };
      }),
    }));
  }, [context.parameters]);
  const set = <K extends keyof EventDraft>(key: K, value: EventDraft[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const pristine = seed();
  const changed = changedFields(draft, pristine);
  const pending = pendingCount(changed);
  const dirty = pending > 0;
  const canApply = dirty && !busy && !offline;

  const revert = () => setDraft(seed());

  const conversion = triggerConversion(pristine.trigger, draft.trigger.form);
  const manual = !draft.trigger.raw && isManual(draft.trigger.form);
  const drivers = eventRefs(raw.id, context.events).all;

  return (
    <div
      style={{ display: "flex", flexDirection: "column", minWidth: 0, height: "100%" }}
      onKeyDown={(e) => {
        if (!dirty) return;
        if (e.key === "Escape") {
          e.preventDefault();
          revert();
        } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
          e.preventDefault();
          if (canApply) onApply(draft);
        }
      }}
    >
      {/* 1 · Identity — across the top, so both columns below are about the
          same event without either having to say which. */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          flexWrap: "wrap",
          gap: "8px 12px",
          padding: "12px 20px",
          borderBottom: "1px solid var(--color-divider)",
        }}
      >
        {/* Unlabelled, and marked dirty by the accent rule alone: the name is
            the heading of the screen below it, and a label appearing only when
            the field is edited would shift the whole row down a line. */}
        <Field className={cx(changed.name && "field-dirty")} style={{ width: 220 }}>
          <CompactInput
            value={draft.name}
            readOnly={offline}
            aria-label="Event name"
            style={{
              minHeight: 34,
              fontFamily: "var(--font-heading)",
              fontWeight: 600,
              fontSize: 17,
            }}
            onChange={(e) => set("name", e.target.value)}
          />
        </Field>
        <Field
          className={cx(changed.description && "field-dirty")}
          style={{ flex: 1, minWidth: 160 }}
        >
          <CompactInput
            value={draft.description}
            readOnly={offline}
            placeholder="note, optional"
            aria-label="Note"
            style={{ minHeight: 34 }}
            onChange={(e) => set("description", e.target.value)}
          />
        </Field>

        <label className="radio" style={{ fontSize: 12.5 }}>
          <input
            type="checkbox"
            checked={draft.firesOnce}
            disabled={offline}
            onChange={(e) => set("firesOnce", e.target.checked)}
          />
          <span className="dot" />
          Fires once
        </label>
        <label className="radio" style={{ fontSize: 12.5 }}>
          <input
            type="checkbox"
            checked={draft.enabled}
            disabled={offline}
            onChange={(e) => set("enabled", e.target.checked)}
          />
          <span className="dot" />
          Enabled
        </label>

        {/* Only where something follows it: offline and unedited, the row ends
            at Enabled, and a rule against nothing reads as a missing control. */}
        {(dirty || savedAt != null || onDuplicate || onDelete) && (
          <span style={{ opacity: 0.35 }}>|</span>
        )}

        {dirty ? (
          <>
            <span style={{ fontSize: 11.5, color: MUTED }}>{pending} unsaved</span>
            <Button shortcut="esc" onClick={revert}>
              Revert
            </Button>
            <Button
              variant="primary"
              shortcut="⌘⏎"
              disabled={!canApply}
              title={offline ? "No connection to the server." : undefined}
              onClick={() => onApply(draft)}
            >
              Apply
            </Button>
          </>
        ) : (
          <>
            {savedAt != null && (
              <span style={{ fontSize: 11.5, color: MUTED }}>
                Saved {fmtClock(savedAt)} · results marked stale
              </span>
            )}
            {onDuplicate && (
              <Button variant="ghost" onClick={onDuplicate} disabled={offline}>
                Duplicate
              </Button>
            )}
            {onDelete && (
              <Button variant="ghost" onClick={onDelete} disabled={offline}>
                Delete
              </Button>
            )}
          </>
        )}
      </div>

      {(error || !draft.enabled) && (
        <div style={{ padding: "10px 20px 0" }}>
          {error && (
            <p style={{ margin: 0, fontSize: 12, color: "var(--color-accent-900)" }}>
              {error}
            </p>
          )}
          {!draft.enabled && (
            <Note>
              Kept in the plan and never fired — a way to take an event out of a run
              without losing how it was set up.
            </Note>
          )}
        </div>
      )}

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          flex: 1,
          minHeight: 0,
          alignItems: "stretch",
        }}
      >
        {/* When — blocks 2 and 3, straight out of artboard 8 */}
        <Column border>
          <SectionHeading action={<TriggerFamily trigger={draft.trigger} />}>
            When
          </SectionHeading>
          <DirtyField label="Fires" changed={changed.trigger}>
            <TriggerFormSelect
              trigger={draft.trigger}
              disabled={offline}
              onChange={(trigger) => set("trigger", trigger)}
            />
          </DirtyField>
          {conversion && <ConversionNote>{conversion}</ConversionNote>}
          <TriggerFields
            trigger={draft.trigger}
            context={context}
            disabled={offline}
            onChange={(trigger) => set("trigger", trigger)}
          />

          {/* 3 · Schedule — the one trigger that keeps one */}
          {hasSchedule(draft.trigger) && (
            <>
              <Hr flush />
              <SectionHeading>Schedule</SectionHeading>
              <ScheduleFields
                trigger={draft.trigger}
                context={context}
                disabled={offline}
                onChange={(trigger) => set("trigger", trigger)}
              />
            </>
          )}

          {/* 6 · Next fires — absent for Manual, which has nothing to resolve */}
          {!manual && (
            <div
              style={{
                marginTop: "auto",
                paddingTop: 12,
                borderTop: "1px solid var(--color-divider)",
              }}
            >
              {/* Off the saved event, not the draft: these are what the server
                  last worked out, and an unsaved edit has not been resolved. */}
              <div style={{ display: "flex", gap: 26, flexWrap: "wrap" }}>
                <InlineStat label="Next fires" value={event.next} />
                <InlineStat label="Amount" value={event.amount} />
              </div>
              {dirty && <Note>As last saved — apply to resolve the edits above.</Note>}
            </div>
          )}
        </Column>

        {/* What — blocks 4, 5 and 7, straight out of artboard 8 */}
        <Column>
          <EffectsBlock
            label="What"
            effects={draft.effects}
            context={context}
            disabled={offline}
            onChange={(effects) => set("effects", effects)}
          />

          {/* 7 · Referenced by */}
          <div style={{ marginTop: "auto", paddingTop: 12 }}>
            <SectionHeading className="mb-[6px]">Referenced by</SectionHeading>
            <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
              {drivers.length === 0 ? (
                <span className="text-muted" style={{ fontSize: 12 }}>
                  No event fires, pauses or ends this one.
                </span>
              ) : (
                drivers.map((e) => (
                  <Tag key={e.id} tone="neutral">
                    {onSelectEvent ? (
                      <button
                        type="button"
                        onClick={() => onSelectEvent(e.name)}
                        style={{
                          all: "unset",
                          cursor: "pointer",
                          textDecoration: "underline",
                          textUnderlineOffset: 3,
                        }}
                      >
                        {e.name}
                      </button>
                    ) : (
                      e.name
                    )}
                  </Tag>
                ))
              )}
            </div>
          </div>
        </Column>
      </div>
    </div>
  );
}

/** One half of the editor: same padding, same rhythm, optional hairline. */
function Column({ children, border }: { children: ReactNode; border?: boolean }) {
  return (
    <div
      style={{
        padding: "16px 20px 18px",
        display: "flex",
        flexDirection: "column",
        gap: 11,
        minWidth: 0,
        borderRight: border ? "1px solid var(--color-divider)" : undefined,
      }}
    >
      {children}
    </div>
  );
}

/** What a conversion abandons, said before it commits. */
function ConversionNote({ children }: { children: ReactNode }) {
  return (
    <Blueprint
      corners={false}
      style={{
        padding: "9px 11px",
        fontSize: 11.5,
        lineHeight: 1.5,
        background: "color-mix(in srgb, var(--color-accent) 6%, transparent)",
      }}
    >
      <div style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
        <svg
          width="14"
          height="14"
          viewBox="0 0 16 16"
          fill="none"
          stroke="var(--color-accent-800)"
          strokeWidth="1.5"
          strokeLinecap="round"
          style={{ flex: "none", marginTop: 2 }}
          aria-hidden
        >
          <path d="M8 2.4 14.4 13.4H1.6z" />
          <line x1="8" y1="6.4" x2="8" y2="9.4" />
          <circle cx="8" cy="11.4" r="0.6" fill="var(--color-accent-800)" />
        </svg>
        <span>{children}</span>
      </div>
    </Blueprint>
  );
}
