"use client";

import { type ReactNode, useState } from "react";
import { Button, CurrencyInput, DateInput, Dropdown, type DropdownOption, NumberInput } from "@/components/ui";
import type { Interval, OffsetUnit } from "@/lib/api/types";
import { addCalendarMonths } from "@/lib/view/format";
import { detailTrigger, namesOf } from "@/lib/view/events";
import {
  ManualNote,
  Note,
  type TriggerContext,
  accountOptions,
  assetOptions,
  eventOptions,
  parameterOptions,
} from "./TriggerFields";
import {
  COMPARISONS,
  CONDITION_FORMS,
  type ConditionDraft,
  type ConditionForm,
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

/** How often, in the words the sentence reads with. */
const EVERY: Record<Interval, string> = {
  Never: "never (as saved)",
  Weekly: "every week",
  BiWeekly: "every 2 weeks",
  Monthly: "every month",
  Quarterly: "every quarter",
  Yearly: "every year",
};

/** The non-repeating shapes, lower-cased to follow "Fires". */
const SHAPE: Record<Exclude<TriggerForm, "Repeating">, string> = {
  "Once on a date": "once on a date",
  "Once at an age": "once at an age",
  "Relative to another event": "once, when",
  "When an account balance crosses": "when an account balance crosses",
  "When a holding crosses": "when a holding crosses",
  "When net worth crosses": "when net worth crosses",
  "All of": "when all of these hold",
  "Any of": "when any of these hold",
  "Only when another event fires it": "only when another event fires it",
};

/** The value standing for a trigger the form did not build and cannot redraw. */
const AS_SAVED = "as-saved";
const SCHEDULE = "every:";

/**
 * The first slot folds the trigger's shape and a schedule's interval into one
 * menu: "every month" is what a person means by a repeating trigger, and a
 * separate Repeating choice followed by an interval would read "Fires
 * repeatedly every month".
 */
function firstSlotOptions(trigger: TriggerDraft): DropdownOption<string>[] {
  const intervals = INTERVALS.includes(trigger.interval) ? INTERVALS : [trigger.interval, ...INTERVALS];
  return [
    ...(trigger.raw ? [{ value: AS_SAVED, label: "as saved — advanced" }] : []),
    ...intervals.map((i) => ({ value: SCHEDULE + i, label: EVERY[i], group: "on a schedule" })),
    ...TRIGGER_FORMS.filter((f): f is Exclude<TriggerForm, "Repeating"> => f !== "Repeating").map((f) => ({
      value: f,
      label: SHAPE[f],
      group: isGroup(f) ? "compound" : isManual(f) ? "no terms" : leafOf(f) === "on a date" || leafOf(f) === "at an age" || leafOf(f) === "relative to an event" ? "once" : "on a balance",
    })),
  ];
}

function firstSlotValue(trigger: TriggerDraft): string {
  if (trigger.raw) return AS_SAVED;
  return trigger.form === "Repeating" ? SCHEDULE + trigger.interval : trigger.form;
}

/**
 * Artboard 17a · When, as one sentence.
 *
 * "Fires every 2 weeks up to ∞ times starting at plan start ending 0
 * years after Retirement." The nine controls of the old column are the same
 * nine, laid into the words that say what they mean, so the trigger can be read
 * before it is edited. Anything that needs explaining goes in one muted line
 * under it rather than a note per field.
 */
export function TriggerSentence({
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
  const pick = (value: string) => {
    if (value === AS_SAVED) return;
    if (value.startsWith(SCHEDULE)) {
      const interval = value.slice(SCHEDULE.length) as Interval;
      onChange({ ...withForm(trigger, "Repeating"), interval });
    } else {
      onChange(withForm(trigger, value as TriggerForm));
    }
  };
  const first = (
    <Dropdown
      inline
      options={firstSlotOptions(trigger)}
      value={firstSlotValue(trigger)}
      ariaLabel="Fires"
      disabled={disabled}
      maxMenuHeight={340}
      onChange={pick}
    />
  );

  if (trigger.raw) {
    return (
      <>
        <div className="sentence"><span>Fires</span>{first}</div>
        <Note>
          {detailTrigger(trigger.raw, namesOf(context))}. Nested deeper than this form draws,
          so it is saved back exactly as it stands — pick another shape to replace it.
        </Note>
      </>
    );
  }

  if (isManual(trigger.form)) {
    return (
      <>
        <div className="sentence"><span>Fires</span>{first}<span className="punct">.</span></div>
        <ManualNote context={context} />
      </>
    );
  }

  const leaf = leafOf(trigger.form);
  if (leaf) {
    const condition = { ...trigger.condition, form: leaf };
    return (
      <>
        <div className="sentence">
          <span>Fires</span>
          {first}
          <ConditionSlots
            condition={condition}
            context={context}
            disabled={disabled}
            onChange={(next) => onChange({ ...trigger, condition: next })}
          />
          <span className="punct">.</span>
        </div>
        <ConditionNote condition={condition} context={context} />
      </>
    );
  }

  if (isGroup(trigger.form)) {
    const patch = (index: number, next: ConditionDraft) =>
      onChange({ ...trigger, children: trigger.children.map((c, i) => (i === index ? next : c)) });
    return (
      <>
        <div className="sentence"><span>Fires</span>{first}<span className="punct">:</span></div>
        <div style={{ display: "flex", flexDirection: "column", gap: 8, paddingLeft: 18 }}>
          {trigger.children.map((condition, index) => (
            <div key={index} className="sentence">
              <span className="stat-l">{index + 1}</span>
              <ConditionFormSlot
                value={condition.form}
                ariaLabel={`Condition ${index + 1}`}
                disabled={disabled}
                onChange={(form) => form !== "none" && patch(index, { ...condition, form })}
              />
              <ConditionSlots
                condition={condition}
                context={context}
                disabled={disabled}
                onChange={(next) => patch(index, next)}
              />
              {trigger.children.length > 1 && (
                <Button
                  variant="ghost"
                  disabled={disabled}
                  style={{ marginLeft: "auto", fontSize: 12 }}
                  onClick={() => onChange({ ...trigger, children: trigger.children.filter((_, i) => i !== index) })}
                >
                  Remove
                </Button>
              )}
            </div>
          ))}
          <Button
            variant="add"
            disabled={disabled}
            style={{ alignSelf: "flex-start" }}
            onClick={() =>
              onChange({
                ...trigger,
                children: [...trigger.children, emptyCondition(context.accounts[0]?.id ?? 0, context.assets[0]?.id ?? 0)],
              })
            }
          >
            + Add condition
          </Button>
        </div>
        <Note>Checked on every step; the event fires on any step where they hold.</Note>
      </>
    );
  }

  // Repeating: the schedule, bounded by a start and an end.
  const bound = (key: "start" | "end", word: string, fallback: string) => {
    const b = trigger[key];
    return (
      <>
        <span>{word}</span>
        <ConditionFormSlot
          value={b.on ? b.condition.form : "none"}
          fallback={fallback}
          ariaLabel={word}
          disabled={disabled}
          onChange={(form) =>
            onChange({
              ...trigger,
              [key]: form === "none" ? { ...b, on: false } : { on: true, condition: { ...b.condition, form } },
            })
          }
        />
        {b.on && (
          <ConditionSlots
            condition={b.condition}
            context={context}
            disabled={disabled}
            onChange={(condition) => onChange({ ...trigger, [key]: { ...b, condition } })}
          />
        )}
      </>
    );
  };

  const notes = [
    trigger.start.on ? conditionNote(trigger.start.condition, context) : null,
    trigger.end.on ? conditionNote(trigger.end.condition, context) : "No end condition means it runs to the horizon.",
  ].filter((n): n is NonNullable<typeof n> => n != null);

  return (
    <>
      <div className="sentence">
        <span>Fires</span>
        {first}
        <span>up to</span>
        <NumberInput
          nullable
          value={trigger.maxOccurrences}
          readOnly={disabled}
          onValueChange={(maxOccurrences) => onChange({ ...trigger, maxOccurrences })}
          decimals={0}
          min={1}
          placeholder="∞"
          aria-label="At most, occurrences — empty for no limit"
        />
        <span>{trigger.maxOccurrences === 1 ? "time" : "times"}</span>
        {bound("start", "starting", "at plan start")}
        {bound("end", "ending", "never")}
        <span className="punct">.</span>
      </div>
      {notes.length > 0 && <Note>{notes.map((n, i) => <span key={i}>{i > 0 && " · "}{n}</span>)}</Note>}
    </>
  );
}

/**
 * The condition kinds as they read in the sentence. "when" stands alone in the
 * slot so the event after it finishes the clause — "ending when Retirement
 * fires" — and the menu's detail column says what it means.
 */
const CONDITION_LABEL: Record<ConditionForm, { label: string; detail?: string }> = {
  "on a date": { label: "on a date" },
  "at an age": { label: "at an age" },
  "relative to an event": { label: "when", detail: "an event fires" },
  "on an account balance": { label: "when a balance", detail: "crosses a threshold" },
  "on a holding": { label: "when a holding", detail: "crosses a threshold" },
  "on net worth": { label: "when net worth", detail: "crosses a threshold" },
};

/** The condition kind as a slot: "on a date", "at an age"… */
function ConditionFormSlot({
  value,
  onChange,
  fallback,
  ariaLabel,
  disabled,
}: {
  value: ConditionForm | "none";
  onChange: (form: ConditionForm | "none") => void;
  fallback?: string;
  ariaLabel?: string;
  disabled?: boolean;
}) {
  const options: DropdownOption<ConditionForm | "none">[] = [
    ...(fallback ? [{ value: "none" as const, label: fallback }] : []),
    ...CONDITION_FORMS.map((f) => ({ value: f, ...CONDITION_LABEL[f] })),
  ];
  return <Dropdown inline options={options} value={value} ariaLabel={ariaLabel} disabled={disabled} onChange={onChange} />;
}

const FIXED = "fixed";
const PARAM = "param:";

/**
 * A date or an age: the literal, or a named parameter standing in for it. One
 * slot rather than a source picker beside a value picker — the parameter's name
 * is itself the value, so it takes the literal's place in the sentence.
 */
function SourceSlot({
  kind,
  fixedLabel,
  source,
  parameterId,
  context,
  disabled,
  onChange,
}: {
  kind: "Date" | "Age";
  fixedLabel: string;
  source: "fixed" | "parameter";
  parameterId: number;
  context: TriggerContext;
  disabled?: boolean;
  onChange: (source: "fixed" | "parameter", parameterId?: number) => void;
}) {
  const params = parameterOptions(context.parameters, kind);
  if (params.length === 0 && source === "fixed") return null;
  return (
    <Dropdown<string>
      inline
      options={[
        { value: FIXED, label: fixedLabel },
        ...params.map((p) => ({ value: PARAM + p.value, label: p.label, group: "parameters" })),
      ]}
      value={source === "parameter" ? PARAM + parameterId : FIXED}
      placeholder="— pick a parameter —"
      ariaLabel={`${kind} source`}
      disabled={disabled}
      onChange={(picked) =>
        picked === FIXED ? onChange("fixed") : onChange("parameter", Number(picked.slice(PARAM.length)))
      }
    />
  );
}

/** The slots one condition needs, in the order they read. */
function ConditionSlots({
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
  const { accounts, assets } = context;
  const patch = (change: Partial<ConditionDraft>) => onChange({ ...condition, ...change });

  const threshold = (
    <>
      <span>is</span>
      <Dropdown
        inline
        options={COMPARISONS.map((c) => ({ value: c.value, label: c.label.toLowerCase() }))}
        value={condition.comparison}
        ariaLabel="Threshold direction"
        disabled={disabled}
        onChange={(comparison) => patch({ comparison })}
      />
      <CurrencyInput
        value={condition.threshold}
        readOnly={disabled}
        onValueChange={(t) => patch({ threshold: t })}
        aria-label="Threshold amount"
      />
    </>
  );

  const account = (
    <Dropdown
      inline
      options={accountOptions(accounts)}
      value={condition.accountId}
      placeholder={accounts.length === 0 ? "no accounts" : "pick an account"}
      ariaLabel="Account"
      disabled={disabled}
      maxMenuHeight={300}
      onChange={(accountId) => patch({ accountId })}
    />
  );

  switch (condition.form) {
    case "on a date":
      return (
        <>
          <SourceSlot
            kind="Date"
            fixedLabel="a fixed date"
            source={condition.dateSource}
            parameterId={condition.dateParameterId}
            context={context}
            disabled={disabled}
            onChange={(dateSource, dateParameterId) =>
              patch({ dateSource, ...(dateParameterId != null ? { dateParameterId } : {}) })
            }
          />
          {condition.dateSource === "fixed" && (
            <DateInput value={condition.date} onValueChange={(date) => patch({ date })} ariaLabel="Trigger date" disabled={disabled} />
          )}
        </>
      );

    case "at an age":
      return (
        <>
          <SourceSlot
            kind="Age"
            fixedLabel="a fixed age"
            source={condition.ageSource}
            parameterId={condition.ageParameterId}
            context={context}
            disabled={disabled}
            onChange={(ageSource, ageParameterId) =>
              patch({ ageSource, ...(ageParameterId != null ? { ageParameterId } : {}) })
            }
          />
          {condition.ageSource === "fixed" && (
            <>
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
            </>
          )}
        </>
      );

    case "relative to an event":
      return <RelativeSlots condition={condition} context={context} disabled={disabled} onChange={patch} />;

    case "on an account balance":
      return (
        <>
          {account}
          {threshold}
        </>
      );

    case "on a holding":
      return (
        <>
          <Dropdown
            inline
            options={assetOptions(assets)}
            value={condition.assetId}
            placeholder={assets.length === 0 ? "no assets" : "pick an asset"}
            ariaLabel="Asset"
            disabled={disabled}
            maxMenuHeight={300}
            onChange={(assetId) => patch({ assetId })}
          />
          <span>in</span>
          {account}
          {threshold}
        </>
      );

    case "on net worth":
      return threshold;
  }
}

/**
 * "Retirement fires", and only when asked, ", 1 year later". An offset of zero
 * is the common case — something starts or stops at the moment another event
 * happens — and "0 years after" was three slots of noise saying so.
 */
function RelativeSlots({
  condition,
  context,
  onChange,
  disabled,
}: {
  condition: ConditionDraft;
  context: TriggerContext;
  onChange: (change: Partial<ConditionDraft>) => void;
  disabled?: boolean;
}) {
  const [adding, setAdding] = useState(false);
  const showOffset = adding || condition.offset !== 0;
  const magnitude = Math.abs(condition.offset);
  const sign = condition.offset < 0 ? -1 : 1;
  return (
    <>
      <Dropdown
        inline
        options={eventOptions(context.events, context.selfId)}
        value={condition.eventId}
        placeholder="pick an event"
        ariaLabel="Anchor event"
        disabled={disabled}
        maxMenuHeight={300}
        onChange={(eventId) => onChange({ eventId })}
      />
      <span>fires</span>
      {showOffset ? (
        <>
          <span className="punct">,</span>
          <NumberInput
            value={magnitude}
            readOnly={disabled}
            onValueChange={(n) => onChange({ offset: sign * n })}
            decimals={0}
            min={0}
            aria-label="Offset"
          />
          <Dropdown
            inline
            options={OFFSET_UNITS.map((u) => ({
              value: u,
              label: (magnitude === 1 ? u.slice(0, -1) : u).toLowerCase(),
            }))}
            value={condition.unit}
            ariaLabel="Offset unit"
            disabled={disabled}
            onChange={(unit) => onChange({ unit })}
          />
          <Dropdown
            inline
            options={[
              { value: "later", label: "later" },
              { value: "earlier", label: "earlier" },
            ]}
            value={sign < 0 ? "earlier" : "later"}
            ariaLabel="Earlier or later"
            disabled={disabled}
            onChange={(side) => onChange({ offset: (side === "earlier" ? -1 : 1) * magnitude })}
          />
          <button
            type="button"
            className="fx"
            title="Remove the offset"
            aria-label="Remove the offset"
            disabled={disabled}
            onClick={() => {
              setAdding(false);
              onChange({ offset: 0 });
            }}
          >
            ×
          </button>
        </>
      ) : (
        <Button variant="add" disabled={disabled} onClick={() => setAdding(true)}>
          + offset
        </Button>
      )}
    </>
  );
}

/**
 * Where a relative condition lands, worked through its anchor: "Retirement
 * fires at age $retire_age (40) → 2037-03-16; this lands 1 year later,
 * 2038-03-16." Only a fixed anchor — a date or an age, literal or named — has
 * a date to show; anything else says what it is measured from.
 */
function anchorNote(condition: ConditionDraft, context: TriggerContext): ReactNode {
  const anchor = context.events.find((e) => e.id === condition.eventId);
  if (!anchor) return "Pick the event this is measured from.";
  const name = <strong style={{ fontWeight: 600 }}>{anchor.name}</strong>;
  const param = (id: number) => context.parameters?.find((p) => p.id === id);
  const t = anchor.trigger;
  let when: ReactNode = null;
  let date: string | null = null;
  if (t.kind === "Date") {
    when = <>on {t.on_date}</>;
    date = t.on_date;
  } else if (t.kind === "DateParameter") {
    const p = param(t.parameter_id);
    if (p?.value.kind === "Date") {
      when = <>on <span className="cd-name">${p.name}</span> ({p.value.value})</>;
      date = p.value.value;
    }
  } else if (t.kind === "Age" || t.kind === "AgeParameter") {
    const p = t.kind === "AgeParameter" ? param(t.parameter_id) : undefined;
    const age = t.kind === "Age" ? { years: t.years, months: t.months ?? 0 } : p?.value.kind === "Age" ? p.value : null;
    if (age) {
      const label = `${age.years}${age.months ? ` yr ${age.months} mo` : ""}`;
      when = p ? <>at age <span className="cd-name">${p.name}</span> ({label})</> : <>at age {label}</>;
      date = context.birthDate ? addCalendarMonths(context.birthDate, age.years * 12 + age.months) : null;
    }
  }
  if (!when) return <>Measured from the first time {name} fires.</>;
  const landed = date && condition.offset !== 0 ? shift(date, condition.offset, condition.unit) : null;
  return (
    <>
      {name} fires {when}
      {date && <> → <strong style={{ fontWeight: 600 }}>{date}</strong></>}
      {landed && <>; this lands on <strong style={{ fontWeight: 600 }}>{landed}</strong></>}.
    </>
  );
}

function shift(iso: string, offset: number, unit: OffsetUnit): string {
  if (unit === "Years") return addCalendarMonths(iso, offset * 12);
  if (unit === "Months") return addCalendarMonths(iso, offset);
  const d = new Date(`${iso}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + offset);
  return d.toISOString().slice(0, 10);
}

/** The one thing worth saying about a condition, if there is one. */
function conditionNote(condition: ConditionDraft, context: TriggerContext): ReactNode {
  switch (condition.form) {
    case "at an age": {
      const parameter = context.parameters?.find((p) => p.id === condition.ageParameterId);
      const age =
        condition.ageSource === "fixed"
          ? { years: condition.age, months: condition.ageMonths ?? 0 }
          : parameter?.value.kind === "Age"
            ? parameter.value
            : null;
      if (!age) return "Pick an age parameter to resolve a date.";
      if (!context.birthDate) return "Ages need the scenario’s birth date; without one this never fires.";
      return (
        <>
          Age resolves to{" "}
          <strong style={{ fontWeight: 600 }}>{addCalendarMonths(context.birthDate, age.years * 12 + age.months)}</strong>.
        </>
      );
    }
    case "relative to an event":
      return anchorNote(condition, context);
    case "on an account balance":
    case "on a holding":
    case "on net worth":
      return "Holds on every step it stays past the threshold — tick Fires once for a one-time crossing.";
    default:
      return null;
  }
}

function ConditionNote({ condition, context }: { condition: ConditionDraft; context: TriggerContext }) {
  const note = conditionNote(condition, context);
  return note ? <Note>{note}</Note> : null;
}
