"use client";

import type { ReactNode } from "react";
import {
  Blueprint,
  Button,
  CurrencyInput,
  DateInput,
  Field,
  NumberInput,
  Select,
} from "@/components/ui";
import type {
  Account,
  Asset,
  Comparison,
  Event as ApiEvent,
  Interval,
  OffsetUnit,
} from "@/lib/api/types";
import {
  COMPARISONS,
  CONDITION_FORMS,
  type ConditionDraft,
  type ConditionForm,
  INTERVALS,
  OFFSET_UNITS,
  type TriggerDraft,
  emptyCondition,
  isGroup,
  leafOf,
} from "./triggerDraft";

export interface TriggerContext {
  accounts: Account[];
  assets: Asset[];
  events: ApiEvent[];
}

/** A row of controls that wraps rather than squeezing — conditions run 1–4 wide. */
function Row({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "repeat(auto-fit, minmax(148px, 1fr))",
        gap: 10,
      }}
    >
      {children}
    </div>
  );
}

function Note({ children }: { children: ReactNode }) {
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

/**
 * Everything the trigger needs beyond the shape itself: the condition it
 * watches, or the schedule it keeps and the conditions that bound it.
 */
export function TriggerFields({
  trigger,
  context,
  onChange,
}: {
  trigger: TriggerDraft;
  context: TriggerContext;
  onChange: (next: TriggerDraft) => void;
}) {
  const leaf = leafOf(trigger.form);

  if (leaf) {
    return (
      <ConditionFields
        condition={{ ...trigger.condition, form: leaf }}
        context={context}
        onChange={(condition) => onChange({ ...trigger, condition })}
      />
    );
  }

  if (isGroup(trigger.form)) {
    const conjunction = trigger.form === "All of" ? "all" : "any";
    const patch = (index: number, next: ConditionDraft) =>
      onChange({
        ...trigger,
        children: trigger.children.map((c, i) => (i === index ? next : c)),
      });

    return (
      <>
        <Note>The event fires on any step where {conjunction} of these hold.</Note>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {trigger.children.map((condition, index) => (
            <Blueprint
              key={index}
              style={{
                padding: "9px 10px",
                display: "flex",
                flexDirection: "column",
                gap: 8,
              }}
            >
              <Row>
                <Field label={`Condition ${index + 1}`}>
                  <FormSelect
                    value={condition.form}
                    onChange={(form) => form !== "none" && patch(index, { ...condition, form })}
                  />
                </Field>
              </Row>
              <ConditionFields
                condition={condition}
                context={context}
                onChange={(next) => patch(index, next)}
              />
              {trigger.children.length > 1 && (
                <Button
                  variant="ghost"
                  style={{ marginLeft: "auto" }}
                  onClick={() =>
                    onChange({
                      ...trigger,
                      children: trigger.children.filter((_, i) => i !== index),
                    })
                  }
                >
                  Remove
                </Button>
              )}
            </Blueprint>
          ))}
        </div>
        <Button
          variant="ghost"
          style={{ alignSelf: "flex-start" }}
          onClick={() =>
            onChange({
              ...trigger,
              children: [
                ...trigger.children,
                emptyCondition(context.accounts[0]?.id ?? 0, context.assets[0]?.id ?? 0),
              ],
            })
          }
        >
          Add condition
        </Button>
      </>
    );
  }

  // Everything else is Repeating.
  return (
    <>
      <Row>
        <Field label="Every">
          <Select
            value={trigger.interval}
            onChange={(e) => onChange({ ...trigger, interval: e.target.value as Interval })}
          >
            {INTERVALS.map((i) => (
              <option key={i}>{i}</option>
            ))}
          </Select>
        </Field>
        <Field label="At most">
          <NumberInput
            nullable
            value={trigger.maxOccurrences}
            onValueChange={(maxOccurrences) => onChange({ ...trigger, maxOccurrences })}
            decimals={0}
            min={1}
            suffix="times"
            placeholder="no limit"
            aria-label="At most, occurrences"
          />
        </Field>
      </Row>
      <BoundFields
        label="Starting"
        fallback="at plan start"
        bound={trigger.start}
        context={context}
        onChange={(start) => onChange({ ...trigger, start })}
      />
      <BoundFields
        label="Ending"
        fallback="never"
        bound={trigger.end}
        context={context}
        onChange={(end) => onChange({ ...trigger, end })}
      />
    </>
  );
}

/** The condition kind, in the words a sub-condition reads with. */
function FormSelect({
  value,
  onChange,
  fallback,
  ariaLabel,
}: {
  value: ConditionForm | "none";
  onChange: (form: ConditionForm | "none") => void;
  /** The label of the extra "no condition" option, where there is one. */
  fallback?: string;
  ariaLabel?: string;
}) {
  return (
    <Select
      value={value}
      aria-label={ariaLabel}
      onChange={(e) => onChange(e.target.value as ConditionForm | "none")}
    >
      {fallback && <option value="none">{fallback}</option>}
      {CONDITION_FORMS.map((f) => (
        <option key={f} value={f}>
          {f}
        </option>
      ))}
    </Select>
  );
}

/** A repeating schedule's start or end: nothing, or any one condition. */
function BoundFields({
  label,
  fallback,
  bound,
  context,
  onChange,
}: {
  label: string;
  fallback: string;
  bound: TriggerDraft["start"];
  context: TriggerContext;
  onChange: (next: TriggerDraft["start"]) => void;
}) {
  return (
    <>
      <Row>
        <Field label={label}>
          <FormSelect
            value={bound.on ? bound.condition.form : "none"}
            fallback={fallback}
            ariaLabel={label}
            onChange={(form) =>
              onChange(
                form === "none"
                  ? { ...bound, on: false }
                  : { on: true, condition: { ...bound.condition, form } },
              )
            }
          />
        </Field>
      </Row>
      {bound.on && (
        <ConditionFields
          condition={bound.condition}
          context={context}
          onChange={(condition) => onChange({ ...bound, condition })}
        />
      )}
    </>
  );
}

/** The fields one condition needs, and only those. */
function ConditionFields({
  condition,
  context,
  onChange,
}: {
  condition: ConditionDraft;
  context: TriggerContext;
  onChange: (next: ConditionDraft) => void;
}) {
  const { accounts, assets, events } = context;
  const patch = (change: Partial<ConditionDraft>) => onChange({ ...condition, ...change });

  const comparison = (
    <Field label="Balance">
      <Select
        value={condition.comparison}
        aria-label="Comparison"
        onChange={(e) => patch({ comparison: e.target.value as Comparison })}
      >
        {COMPARISONS.map((c) => (
          <option key={c.value} value={c.value}>
            {c.label}
          </option>
        ))}
      </Select>
    </Field>
  );

  /**
   * The engine re-tests a threshold every step, so one that stays crossed
   * keeps firing — which is what a floor-topping sweep wants, and not what a
   * one-off wants.
   */
  const standing = (
    <Note>
      Holds on every step the balance stays past the threshold. For a one-time crossing, tick{" "}
      <em>Fires once only</em>.
    </Note>
  );

  const threshold = (
    <Field label="Threshold">
      <CurrencyInput
        value={condition.threshold}
        onValueChange={(t) => patch({ threshold: t })}
        aria-label="Threshold"
      />
    </Field>
  );

  const account = (
    <Field label="Account">
      <Select
        value={condition.accountId}
        aria-label="Account"
        onChange={(e) => patch({ accountId: Number(e.target.value) })}
      >
        {accounts.length === 0 && <option value={0}>— no accounts —</option>}
        {accounts.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </Select>
    </Field>
  );

  switch (condition.form) {
    case "on a date":
      return (
        <Row>
          <Field label="Date">
            <DateInput
              value={condition.date}
              onValueChange={(date) => patch({ date })}
              ariaLabel="Trigger date"
            />
          </Field>
        </Row>
      );

    case "at an age":
      return (
        <>
          <Row>
            <Field label="Age">
              <NumberInput
                value={condition.age}
                onValueChange={(age) => patch({ age })}
                decimals={0}
                min={0}
                max={120}
                suffix="years"
                aria-label="Age"
              />
            </Field>
          </Row>
          <Note>Ages need the scenario&rsquo;s birth date; without one this never fires.</Note>
        </>
      );

    case "relative to an event":
      return (
        <>
          <Row>
            <Field label="Event">
              <Select
                value={condition.eventId}
                aria-label="Anchor event"
                onChange={(e) => patch({ eventId: Number(e.target.value) })}
              >
                <option value={0}>— pick an event —</option>
                {events.map((e) => (
                  <option key={e.id} value={e.id}>
                    {e.name}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Offset">
              <NumberInput
                value={condition.offset}
                onValueChange={(offset) => patch({ offset })}
                decimals={0}
                allowNegative
                aria-label="Offset"
              />
            </Field>
            <Field label="Unit">
              <Select
                value={condition.unit}
                aria-label="Offset unit"
                onChange={(e) => patch({ unit: e.target.value as OffsetUnit })}
              >
                {OFFSET_UNITS.map((u) => (
                  <option key={u}>{u}</option>
                ))}
              </Select>
            </Field>
          </Row>
          <Note>
            Measured from the first time that event fires. A negative offset lands before it.
          </Note>
        </>
      );

    case "on an account balance":
      return (
        <>
          <Row>
            {account}
            {comparison}
            {threshold}
          </Row>
          {standing}
        </>
      );

    case "on a holding":
      return (
        <>
          <Row>
            {account}
            <Field label="Asset">
              <Select
                value={condition.assetId}
                aria-label="Asset"
                onChange={(e) => patch({ assetId: Number(e.target.value) })}
              >
                {assets.length === 0 && <option value={0}>— no assets —</option>}
                {assets.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </Select>
            </Field>
            {comparison}
            {threshold}
          </Row>
          {standing}
        </>
      );

    case "on net worth":
      return (
        <>
          <Row>
            <Field label="Net worth">
              <Select
                value={condition.comparison}
                aria-label="Comparison"
                onChange={(e) => patch({ comparison: e.target.value as Comparison })}
              >
                {COMPARISONS.map((c) => (
                  <option key={c.value} value={c.value}>
                    {c.label}
                  </option>
                ))}
              </Select>
            </Field>
            {threshold}
          </Row>
          {standing}
        </>
      );
  }
}
