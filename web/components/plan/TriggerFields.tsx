"use client";

import type { ReactNode } from "react";
import {
  Blueprint,
  Button,
  CurrencyInput,
  DateInput,
  Dropdown,
  type DropdownOption,
  Field,
  NumberInput,
  Tag,
} from "@/components/ui";
import type { Account, Asset, Event as ApiEvent } from "@/lib/api/types";
import { addYears } from "@/lib/view/format";
import { detailTrigger, namesOf } from "@/lib/view/events";
import { eventRefs } from "@/lib/view/refs";
import {
  COMPARISONS,
  CONDITION_FORMS,
  type ConditionDraft,
  type ConditionForm,
  FAMILY,
  INTERVALS,
  OFFSET_UNITS,
  TRIGGER_FORMS,
  type TriggerDraft,
  type TriggerForm,
  emptyCondition,
  isGroup,
  isManual,
  leafOf,
  withForm,
} from "./triggerDraft";

export interface TriggerContext {
  accounts: Account[];
  assets: Asset[];
  events: ApiEvent[];
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

/** The value standing for a trigger the form did not build and cannot redraw. */
const AS_SAVED = "\u0000as-saved";

/**
 * The trigger's shape, and the family it belongs to.
 *
 * A trigger too deep for this form gets an option of its own, selected — so
 * the field says what the event actually does rather than naming some shape it
 * is not, and picking anything else is the deliberate act that replaces it.
 */
export function TriggerFormSelect({
  trigger,
  onChange,
  disabled,
}: {
  trigger: TriggerDraft;
  onChange: (next: TriggerDraft) => void;
  disabled?: boolean;
}) {
  const options: DropdownOption<string>[] = trigger.raw
    ? [{ value: AS_SAVED, label: "As saved — advanced" }, ...TRIGGER_OPTIONS]
    : TRIGGER_OPTIONS;

  return (
    <Dropdown
      className="dd-field"
      options={options}
      value={trigger.raw ? AS_SAVED : trigger.form}
      ariaLabel="Trigger"
      disabled={disabled}
      maxMenuHeight={320}
      onChange={(picked) => {
        if (picked !== AS_SAVED) onChange(withForm(trigger, picked as TriggerForm));
      }}
    />
  );
}

/**
 * The ten shapes under their family's band — the grouping the tag beside the
 * field names, which the flat native list could only imply by its order.
 */
const TRIGGER_OPTIONS: DropdownOption<string>[] = TRIGGER_FORMS.map((f) => ({
  value: f,
  label: f,
  group: FAMILY[f].label,
}));

/** What shape the trigger is, in one word, beside its name. */
export function TriggerFamily({ trigger }: { trigger: TriggerDraft }) {
  if (trigger.raw) return <Tag tone="neutral">advanced</Tag>;
  const family = FAMILY[trigger.form];
  return <Tag tone={family.tone}>{family.label}</Tag>;
}

/**
 * Block 2 · the condition the trigger watches.
 *
 * A repeating trigger's own terms are its schedule, which is block 3 — so this
 * renders nothing for it, and the drawer mounts `ScheduleFields` instead.
 */
export function TriggerFields({
  trigger,
  context,
  onChange,
  disabled,
}: {
  trigger: TriggerDraft;
  context: TriggerContext;
  onChange: (next: TriggerDraft) => void;
  disabled?: boolean;
}) {
  if (trigger.raw) {
    return (
      <Note>
        {detailTrigger(trigger.raw, namesOf(context))}. Nested deeper than this form
        draws, so it is saved back exactly as it stands — pick another shape above to
        replace it.
      </Note>
    );
  }

  if (isManual(trigger.form)) return <ManualNote context={context} />;

  const leaf = leafOf(trigger.form);
  if (leaf) {
    return (
      <ConditionFields
        condition={{ ...trigger.condition, form: leaf }}
        context={context}
        disabled={disabled}
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
                    disabled={disabled}
                    onChange={(form) => form !== "none" && patch(index, { ...condition, form })}
                  />
                </Field>
              </Row>
              <ConditionFields
                condition={condition}
                context={context}
                disabled={disabled}
                onChange={(next) => patch(index, next)}
              />
              {trigger.children.length > 1 && (
                <Button
                  variant="ghost"
                  disabled={disabled}
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
          disabled={disabled}
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

  // Repeating: its terms are the schedule, which is a block of its own.
  return null;
}

/** Whether this trigger mounts block 3 at all. Only a schedule has one. */
export function hasSchedule(trigger: TriggerDraft): boolean {
  return !trigger.raw && trigger.form === "Repeating";
}

/**
 * Block 3 · the schedule: how often, and the two conditions that bound it.
 *
 * Its own block rather than more of block 2, because start and end are whole
 * triggers themselves — nesting them under the trigger's own terms reads as if
 * they were terms of it.
 */
export function ScheduleFields({
  trigger,
  context,
  onChange,
  disabled,
}: {
  trigger: TriggerDraft;
  context: TriggerContext;
  onChange: (next: TriggerDraft) => void;
  disabled?: boolean;
}) {
  return (
    <>
      <Row>
        <Field label="Every">
          <Dropdown
            className="dd-field"
            /* `Never` is a legal saved interval the picker does not offer;
               without it the field would sit blank on an event that has one. */
            options={(INTERVALS.includes(trigger.interval)
              ? INTERVALS
              : [trigger.interval, ...INTERVALS]
            ).map((i) => ({ value: i, label: i }))}
            value={trigger.interval}
            ariaLabel="Every"
            disabled={disabled}
            onChange={(interval) => onChange({ ...trigger, interval })}
          />
        </Field>
        <Field label="At most">
          <NumberInput
            nullable
            value={trigger.maxOccurrences}
            readOnly={disabled}
            onValueChange={(maxOccurrences) => onChange({ ...trigger, maxOccurrences })}
            decimals={0}
            min={1}
            suffix={trigger.maxOccurrences === 1 ? "time" : "times"}
            placeholder="unlimited"
            affixesWhenEmpty
            aria-label="At most, occurrences"
          />
        </Field>
      </Row>
      <BoundFields
        label="Starting"
        fallback="at plan start"
        bound={trigger.start}
        context={context}
        disabled={disabled}
        onChange={(start) => onChange({ ...trigger, start })}
      />
      <BoundFields
        label="Ending"
        fallback="never"
        bound={trigger.end}
        context={context}
        disabled={disabled}
        onChange={(end) => onChange({ ...trigger, end })}
      />
      {!trigger.end.on && <Note>No end condition means it runs to the horizon.</Note>}
    </>
  );
}

/** A `Manual` trigger has no terms — only the events that reach it. */
function ManualNote({ context }: { context: TriggerContext }) {
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

/** The condition kind, in the words a sub-condition reads with. */
function FormSelect({
  value,
  onChange,
  fallback,
  ariaLabel,
  disabled,
}: {
  value: ConditionForm | "none";
  onChange: (form: ConditionForm | "none") => void;
  /** The label of the extra "no condition" option, where there is one. */
  fallback?: string;
  ariaLabel?: string;
  disabled?: boolean;
}) {
  const options: DropdownOption<ConditionForm | "none">[] = [
    ...(fallback ? [{ value: "none" as const, label: fallback }] : []),
    ...CONDITION_FORMS.map((f) => ({ value: f, label: f })),
  ];

  return (
    <Dropdown
      className="dd-field"
      options={options}
      value={value}
      ariaLabel={ariaLabel}
      disabled={disabled}
      onChange={onChange}
    />
  );
}

/** A repeating schedule's start or end: nothing, or any one condition. */
function BoundFields({
  label,
  fallback,
  bound,
  context,
  onChange,
  disabled,
}: {
  label: string;
  fallback: string;
  bound: TriggerDraft["start"];
  context: TriggerContext;
  onChange: (next: TriggerDraft["start"]) => void;
  disabled?: boolean;
}) {
  return (
    <>
      <Row>
        <Field label={label}>
          <FormSelect
            value={bound.on ? bound.condition.form : "none"}
            fallback={fallback}
            ariaLabel={label}
            disabled={disabled}
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
          disabled={disabled}
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
  disabled,
}: {
  condition: ConditionDraft;
  context: TriggerContext;
  onChange: (next: ConditionDraft) => void;
  disabled?: boolean;
}) {
  const { accounts, assets, events } = context;
  const patch = (change: Partial<ConditionDraft>) => onChange({ ...condition, ...change });

  /**
   * The threshold pair, identical in all three balance conditions — so Below /
   * Above and the figure sit in the same place whichever one you picked, and
   * switching between them moves no field sideways.
   */
  const threshold = (
    <>
      <Field label="Threshold">
        <Dropdown
          className="dd-field"
          options={COMPARISONS}
          value={condition.comparison}
          ariaLabel="Threshold direction"
          disabled={disabled}
          onChange={(comparison) => patch({ comparison })}
        />
      </Field>
      <Field label="Amount">
        <CurrencyInput
          value={condition.threshold}
          readOnly={disabled}
          onValueChange={(t) => patch({ threshold: t })}
          aria-label="Threshold amount"
        />
      </Field>
    </>
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

  const account = (
    <Field label="Account">
      <Dropdown
        className="dd-field"
        options={accountOptions(accounts)}
        value={condition.accountId}
        placeholder={accounts.length === 0 ? "— no accounts —" : "— pick an account —"}
        ariaLabel="Account"
        disabled={disabled}
        maxMenuHeight={300}
        onChange={(accountId) => patch({ accountId })}
      />
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

    case "at an age": {
      const lands =
        context.birthDate && condition.age > 0
          ? shiftMonths(addYears(context.birthDate, condition.age), condition.ageMonths ?? 0)
          : null;
      return (
        <>
          <Row>
            <Field label="Years">
              <NumberInput
                value={condition.age}
                readOnly={disabled}
                onValueChange={(age) => patch({ age })}
                decimals={0}
                min={0}
                max={120}
                suffix={condition.age === 1 ? "year" : "years"}
                aria-label="Years"
              />
            </Field>
            <Field label="Months">
              <NumberInput
                nullable
                value={condition.ageMonths}
                readOnly={disabled}
                onValueChange={(ageMonths) => patch({ ageMonths })}
                decimals={0}
                min={0}
                max={11}
                suffix={condition.ageMonths === 1 ? "month" : "months"}
                placeholder="0"
                affixesWhenEmpty
                aria-label="Months"
              />
            </Field>
          </Row>
          <Note>
            Months is optional — left empty it means the birthday itself.{" "}
            {lands ? (
              <>
                Resolves to <strong style={{ fontWeight: 600 }}>{lands}</strong> for this
                plan.
              </>
            ) : (
              <>Ages need the scenario&rsquo;s birth date; without one this never fires.</>
            )}
          </Note>
        </>
      );
    }

    case "relative to an event":
      return (
        <>
          <Row>
            <Field label="Anchor event">
              <Dropdown
                className="dd-field"
                options={eventOptions(events, context.selfId)}
                value={condition.eventId}
                placeholder="— pick an event —"
                ariaLabel="Anchor event"
                disabled={disabled}
                maxMenuHeight={300}
                onChange={(eventId) => patch({ eventId })}
              />
            </Field>
            <Field label="Offset">
              <NumberInput
                value={condition.offset}
                readOnly={disabled}
                onValueChange={(offset) => patch({ offset })}
                decimals={0}
                allowNegative
                aria-label="Offset"
              />
            </Field>
            <Field label="Unit">
              <Dropdown
                className="dd-field"
                options={OFFSET_UNITS.map((u) => ({ value: u, label: u }))}
                value={condition.unit}
                ariaLabel="Offset unit"
                disabled={disabled}
                onChange={(unit) => patch({ unit })}
              />
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
              <Dropdown
                className="dd-field"
                options={assetOptions(assets)}
                value={condition.assetId}
                placeholder={assets.length === 0 ? "— no assets —" : "— pick an asset —"}
                ariaLabel="Asset"
                disabled={disabled}
                maxMenuHeight={300}
                onChange={(assetId) => patch({ assetId })}
              />
            </Field>
            {threshold}
          </Row>
          {standing}
        </>
      );

    case "on net worth":
      return (
        <>
          <Row>{threshold}</Row>
          <Note>Assets less liabilities, before tax on unrealized gains.</Note>
          {standing}
        </>
      );
  }
}

/** An ISO date moved on by whole months, for the age note above. */
function shiftMonths(isoDate: string, months: number): string {
  if (months === 0) return isoDate;
  const [y, m, d] = isoDate.split("-").map(Number);
  // UTC so a timezone west of Greenwich cannot roll the date back a day.
  return new Date(Date.UTC(y, m - 1 + months, d)).toISOString().slice(0, 10);
}
