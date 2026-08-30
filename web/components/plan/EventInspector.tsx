"use client";

import {
  Button,
  CompactInput,
  Field,
  Hr,
  SectionHeading,
  Select,
  Tag,
} from "@/components/ui";
import type { PlanEvent, TriggerKind } from "@/lib/types";
import { EffectCard } from "./EffectCard";

const TRIGGER_KINDS: TriggerKind[] = [
  "Date",
  "Age",
  "Repeating",
  "NetWorth",
  "AccountBalance",
  "RelativeToEvent",
  "And",
  "Or",
];

/** Inspects the selected event, in the same drawer pattern as Portfolio's. */
export function EventInspector({
  event,
  onApply,
  onAddEffect,
  onOpenTaxConfig,
}: {
  event: PlanEvent;
  onApply?: () => void;
  onAddEffect?: () => void;
  onOpenTaxConfig?: () => void;
}) {
  return (
    <div
      style={{
        padding: "16px 18px 20px",
        display: "flex",
        flexDirection: "column",
        gap: 12,
        height: "100%",
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
        <h5 style={{ margin: 0, fontFamily: "ui-monospace, Menlo, monospace", fontSize: 14 }}>
          {event.id}
        </h5>
        <Tag tone="outline">{event.triggerKind}</Tag>
      </div>

      <div>
        <SectionHeading className="mb-[6px]">Trigger</SectionHeading>
        <Field className="mb-[8px]">
          <Select style={{ minHeight: 32 }} defaultValue={event.triggerKind}>
            {TRIGGER_KINDS.map((k) => (
              <option key={k}>{k}</option>
            ))}
          </Select>
        </Field>
        <div
          style={{
            fontSize: 12,
            color: "color-mix(in srgb, var(--color-text) 60%, transparent)",
          }}
        >
          {event.triggerDetail}
        </div>
        <div style={{ fontSize: 12, marginTop: 4 }}>
          Next fire <strong style={{ fontWeight: 500 }}>{event.next}</strong>
        </div>
      </div>

      <Hr flush />

      <div>
        <SectionHeading
          className="mb-[6px]"
          action={
            <Button variant="ghost" onClick={onAddEffect}>
              Add effect
            </Button>
          }
        >
          Effects
        </SectionHeading>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {event.effects.map((f, i) => (
            <EffectCard key={`${f.kind}-${i}`} effect={f} />
          ))}
        </div>
      </div>

      <Hr flush />

      <Field label="Amount">
        <CompactInput value={event.amount} readOnly />
      </Field>

      <label className="radio">
        <input type="checkbox" checked={event.firesOnce ?? false} readOnly />
        <span className="dot" />
        Fires once only
      </label>

      <div style={{ display: "flex", gap: 8, marginTop: "auto" }}>
        <Button variant="ghost" onClick={onOpenTaxConfig}>
          Tax config…
        </Button>
        <Button variant="primary" style={{ marginLeft: "auto" }} onClick={onApply}>
          Apply
        </Button>
      </div>
    </div>
  );
}
