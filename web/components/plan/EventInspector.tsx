"use client";

import type { ReactNode } from "react";
import { useState } from "react";
import {
  Blueprint,
  Button,
  CompactInput,
  DirtyField,
  Hr,
  InlineStat,
  SectionHeading,
  Tag,
} from "@/components/ui";
import type { Event as ApiEvent } from "@/lib/api/types";
import { fmtClock } from "@/lib/format";
import type { PlanEvent } from "@/lib/types";
import { eventRefs } from "@/lib/view/refs";
import { EffectsBlock } from "./EffectFields";
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

/**
 * The event drawer — one skeleton, ten triggers, eleven effect forms.
 *
 * Seven blocks in a fixed order: identity, trigger, schedule, effects,
 * selected effect, next fires, referenced by. Same contract as the account
 * drawer: order never changes, a block is present or absent, never moved. Only
 * blocks 2 and 5 vary in content — everything else is the same component with
 * different data, so the eye learns one shape rather than ten.
 *
 * A term the engine does not read for the chosen kind is absent, not disabled:
 * no greyed-out lot method on an Expense.
 *
 * Mount with a `key` of the event id so switching rows starts a fresh draft
 * rather than carrying edits across.
 */
export function EventInspector({
  event,
  raw,
  context,
  onApply,
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
    draftOfEvent(raw, context.accounts[0]?.id ?? 0, context.assets[0]?.id ?? 0);
  const [draft, setDraft] = useState<EventDraft>(seed);
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
      style={{
        padding: "18px 18px 22px",
        display: "flex",
        flexDirection: "column",
        gap: 13,
        height: "100%",
      }}
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
      {/* 1 · Identity */}
      <div>
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            justifyContent: "space-between",
            gap: 8,
          }}
        >
          <h5 style={{ margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
            {draft.name.trim() || event.id}
          </h5>
          {!draft.enabled && <Tag tone="neutral">off</Tag>}
        </div>
        <DirtyField label="Name" changed={changed.name} style={{ marginTop: 8 }}>
          <CompactInput
            value={draft.name}
            readOnly={offline}
            aria-label="Event name"
            onChange={(e) => set("name", e.target.value)}
          />
        </DirtyField>
        <DirtyField label="Note" changed={changed.description} style={{ marginTop: 8 }}>
          <CompactInput
            value={draft.description}
            readOnly={offline}
            placeholder="optional"
            aria-label="Note"
            onChange={(e) => set("description", e.target.value)}
          />
        </DirtyField>
        <div style={{ display: "flex", gap: 16, marginTop: 10, flexWrap: "wrap" }}>
          <label className="radio" style={{ fontSize: 13 }}>
            <input
              type="checkbox"
              checked={draft.firesOnce}
              disabled={offline}
              onChange={(e) => set("firesOnce", e.target.checked)}
            />
            <span className="dot" />
            Fires once
          </label>
          <label className="radio" style={{ fontSize: 13 }}>
            <input
              type="checkbox"
              checked={draft.enabled}
              disabled={offline}
              onChange={(e) => set("enabled", e.target.checked)}
            />
            <span className="dot" />
            Enabled
          </label>
        </div>
        {!draft.enabled && (
          <Note>
            Kept in the plan and never fired — a way to take an event out of a run without
            losing how it was set up.
          </Note>
        )}
      </div>

      <Hr flush />

      {/* 2 · Trigger */}
      <div style={{ display: "flex", flexDirection: "column", gap: 9 }}>
        <SectionHeading action={<TriggerFamily trigger={draft.trigger} />}>
          Trigger
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
      </div>

      {/* 3 · Schedule — the one trigger that keeps one */}
      {hasSchedule(draft.trigger) && (
        <>
          <Hr flush />
          <div style={{ display: "flex", flexDirection: "column", gap: 9 }}>
            <SectionHeading>Schedule</SectionHeading>
            <ScheduleFields
              trigger={draft.trigger}
              context={context}
              disabled={offline}
              onChange={(trigger) => set("trigger", trigger)}
            />
          </div>
        </>
      )}

      <Hr flush />

      {/* 4 · Effects, and 5 · the selected one's terms */}
      <EffectsBlock
        effects={draft.effects}
        context={context}
        disabled={offline}
        onChange={(effects) => set("effects", effects)}
      />

      {/* 6 · Next fires — absent for Manual, which has nothing to resolve */}
      {!manual && (
        <>
          <Hr flush />
          <div>
            <SectionHeading className="mb-[8px]">Next fires</SectionHeading>
            {/* Off the saved event, not the draft: these are what the server
                last worked out, and an unsaved edit has not been resolved. */}
            <div style={{ display: "flex", gap: 22, flexWrap: "wrap" }}>
              <InlineStat label="Next" value={event.next} />
              <InlineStat label="Amount" value={event.amount} />
            </div>
            {dirty && <Note>As last saved — apply to resolve the edits above.</Note>}
          </div>
        </>
      )}

      <Hr flush />

      {/* 7 · Referenced by */}
      <div>
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

      {error && (
        <p style={{ margin: 0, fontSize: 12, color: "var(--color-accent-900)" }}>{error}</p>
      )}

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          marginTop: "auto",
          paddingTop: 4,
          fontSize: 11.5,
          color: MUTED,
        }}
      >
        {dirty ? (
          <>
            <span>{pending} unsaved</span>
            <Button style={{ marginLeft: "auto" }} shortcut="esc" onClick={revert}>
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
              <span>Saved {fmtClock(savedAt)} · results marked stale</span>
            )}
            {onDelete && (
              <Button
                variant="ghost"
                style={{ marginLeft: "auto" }}
                onClick={onDelete}
                disabled={offline}
              >
                Delete
              </Button>
            )}
          </>
        )}
      </div>
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
