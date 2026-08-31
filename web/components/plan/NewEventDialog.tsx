"use client";

import { useState } from "react";
import {
  Blueprint,
  Button,
  CurrencyInput,
  DateInput,
  Dialog,
  DialogRow,
  Field,
  Hr,
  Input,
  SectionHeading,
  Select,
} from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Account, Asset, Event as ApiEvent, Interval } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import {
  type BoundDraft,
  INTERVALS,
  TRIGGER_FORMS,
  type TriggerDraft,
  type TriggerForm,
  emptyTrigger,
  toTriggerSpec,
} from "./triggerDraft";
import {
  EFFECT_FORMS,
  type EffectDraft,
  type EffectForm,
  STRATEGIES,
  effectProblem,
  emptyEffect,
  shape,
  toEffectSpec,
} from "./effectDraft";

/**
 * Creates an event: one trigger, and the ordered effects it fires.
 *
 * The API takes the whole tree in one POST, so this builds and sends it in one
 * step rather than creating a shell and filling it in.
 */
export function NewEventDialog({
  scenarioId,
  accounts,
  assets,
  events,
  onClose,
  onCreated,
}: {
  scenarioId: number;
  accounts: Account[];
  assets: Asset[];
  events: ApiEvent[];
  onClose: () => void;
  onCreated: () => void;
}) {
  const firstAccount = accounts[0]?.id ?? 0;
  const firstAsset = assets[0]?.id ?? 0;

  const [name, setName] = useState("");
  const [firesOnce, setFiresOnce] = useState(false);
  const [trigger, setTrigger] = useState<TriggerDraft>(emptyTrigger);
  const [effects, setEffects] = useState<EffectDraft[]>([
    emptyEffect(firstAccount, firstAsset),
  ]);
  const submit = useSubmit();

  const patch = (index: number, change: Partial<EffectDraft>) =>
    setEffects((list) => list.map((e, i) => (i === index ? { ...e, ...change } : e)));

  const create = () => {
    // A missing target would trip a foreign key. The server answers 400 for
    // that, but naming the offending effect here saves the round trip.
    const problem = effects.map(effectProblem).find((p) => p != null);
    if (problem) return submit.fail(problem);

    submit.run(
      () =>
        api.events.create(scenarioId, {
          name,
          fires_once: firesOnce,
          enabled: true,
          sort_order: 0,
          trigger: toTriggerSpec(trigger),
          effects: effects.map(toEffectSpec),
        }),
      () => {
        onCreated();
        onClose();
      },
    );
  };

  return (
    <Dialog
      title="New event"
      onClose={onClose}
      onSubmit={create}
      submitLabel="Create event"
      busy={submit.busy}
      error={
        accounts.length === 0
          ? "An event moves money between accounts. Create an account first."
          : submit.error
      }
    >
      <DialogRow>
        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} required />
        </Field>
        <Field label="Trigger">
          <Select
            value={trigger.form}
            onChange={(e) => setTrigger({ ...trigger, form: e.target.value as TriggerForm })}
          >
            {TRIGGER_FORMS.map((f) => (
              <option key={f}>{f}</option>
            ))}
          </Select>
        </Field>
      </DialogRow>

      {trigger.form === "Once on a date" && (
        <Field label="Date">
          <DateInput
            value={trigger.date}
            onChange={(e) => setTrigger({ ...trigger, date: e.target.value })}
            required
          />
        </Field>
      )}

      {trigger.form === "Once at an age" && (
        <Field label="Age — the scenario needs a birth date for this">
          <Input
            type="number"
            value={trigger.age}
            onChange={(e) => setTrigger({ ...trigger, age: e.target.value })}
          />
        </Field>
      )}

      {trigger.form === "Repeating" && (
        <>
          <Field label="Every">
            <Select
              value={trigger.interval}
              onChange={(e) => setTrigger({ ...trigger, interval: e.target.value as Interval })}
            >
              {INTERVALS.map((i) => (
                <option key={i}>{i}</option>
              ))}
            </Select>
          </Field>
          <DialogRow>
            <BoundField
              label="Starting"
              fallback="at plan start"
              value={trigger.start}
              onChange={(start) => setTrigger({ ...trigger, start })}
            />
            <BoundField
              label="Ending"
              fallback="never"
              value={trigger.end}
              onChange={(end) => setTrigger({ ...trigger, end })}
            />
          </DialogRow>
        </>
      )}

      <label className="radio">
        <input
          type="checkbox"
          checked={firesOnce}
          onChange={(e) => setFiresOnce(e.target.checked)}
        />
        <span className="dot" />
        Fires once only
      </label>

      <Hr flush />

      <SectionHeading
        action={
          <Button
            variant="ghost"
            onClick={() => setEffects((list) => [...list, emptyEffect(firstAccount, firstAsset)])}
          >
            Add effect
          </Button>
        }
      >
        Effects
      </SectionHeading>

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {effects.map((effect, index) => (
          <EffectFields
            key={index}
            effect={effect}
            accounts={accounts}
            assets={assets}
            events={events}
            onChange={(change) => patch(index, change)}
            onRemove={
              effects.length > 1
                ? () => setEffects((list) => list.filter((_, i) => i !== index))
                : undefined
            }
          />
        ))}
      </div>
    </Dialog>
  );
}

/** A repeating trigger's start or end: nothing, a date, or an age. */
function BoundField({
  label,
  fallback,
  value,
  onChange,
}: {
  label: string;
  fallback: string;
  value: BoundDraft;
  onChange: (next: BoundDraft) => void;
}) {
  return (
    <Field label={label}>
      <div style={{ display: "flex", gap: 6 }}>
        <Select
          value={value.kind}
          onChange={(e) => onChange({ ...value, kind: e.target.value as BoundDraft["kind"] })}
        >
          <option value="none">{fallback}</option>
          <option value="date">on date</option>
          <option value="age">at age</option>
        </Select>
        {value.kind === "date" && (
          <DateInput
            value={value.date}
            onChange={(e) => onChange({ ...value, date: e.target.value })}
          />
        )}
        {value.kind === "age" && (
          <Input
            type="number"
            style={{ width: 72 }}
            value={value.age}
            onChange={(e) => onChange({ ...value, age: e.target.value })}
          />
        )}
      </div>
    </Field>
  );
}

function EffectFields({
  effect,
  accounts,
  assets,
  events,
  onChange,
  onRemove,
}: {
  effect: EffectDraft;
  accounts: Account[];
  assets: Asset[];
  events: ApiEvent[];
  onChange: (change: Partial<EffectDraft>) => void;
  onRemove?: () => void;
}) {
  const fields = shape(effect.form);

  const accountSelect = (value: number, key: "fromAccountId" | "toAccountId", label: string) => (
    <Field label={label}>
      <Select value={value} onChange={(e) => onChange({ [key]: Number(e.target.value) })}>
        {accounts.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </Select>
    </Field>
  );

  return (
    <Blueprint style={{ padding: "9px 10px", display: "flex", flexDirection: "column", gap: 8 }}>
      <DialogRow>
        <Field label="Effect">
          <Select
            value={effect.form}
            onChange={(e) => onChange({ form: e.target.value as EffectForm })}
          >
            {EFFECT_FORMS.map((f) => (
              <option key={f}>{f}</option>
            ))}
          </Select>
        </Field>
        {fields.amount && (
          <Field label="Amount per occurrence">
            <CurrencyInput
              value={effect.amount}
              onValueChange={(amount) => onChange({ amount })}
              aria-label="Amount per occurrence"
            />
          </Field>
        )}
      </DialogRow>

      <DialogRow>
        {fields.from && accountSelect(effect.fromAccountId, "fromAccountId", "From account")}
        {fields.to && accountSelect(effect.toAccountId, "toAccountId", "To account")}
        {fields.asset && (
          <Field label="Asset">
            <Select
              value={effect.assetId}
              onChange={(e) => onChange({ assetId: Number(e.target.value) })}
            >
              {assets.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </Select>
          </Field>
        )}
        {fields.strategy && (
          <Field label="Draw from">
            <Select
              value={effect.strategy}
              onChange={(e) =>
                onChange({ strategy: e.target.value as EffectDraft["strategy"] })
              }
            >
              {STRATEGIES.map((s) => (
                <option key={s}>{s}</option>
              ))}
            </Select>
          </Field>
        )}
        {fields.event && (
          <Field label="Target event">
            <Select
              value={effect.targetEventId}
              onChange={(e) => onChange({ targetEventId: Number(e.target.value) })}
            >
              <option value={0}>— pick an event —</option>
              {events.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.name}
                </option>
              ))}
            </Select>
          </Field>
        )}
      </DialogRow>

      <div style={{ display: "flex", gap: 14, alignItems: "center", flexWrap: "wrap" }}>
        {fields.amount && (
          <label className="radio" style={{ fontSize: 12 }}>
            <input
              type="checkbox"
              checked={effect.inflationAdjusted}
              onChange={(e) => onChange({ inflationAdjusted: e.target.checked })}
            />
            <span className="dot" />
            In today&rsquo;s money
          </label>
        )}
        {fields.taxable && (
          <label className="radio" style={{ fontSize: 12 }}>
            <input
              type="checkbox"
              checked={effect.taxFree}
              onChange={(e) => onChange({ taxFree: e.target.checked })}
            />
            <span className="dot" />
            Tax-free
          </label>
        )}
        {onRemove && (
          <Button variant="ghost" onClick={onRemove} style={{ marginLeft: "auto" }}>
            Remove
          </Button>
        )}
      </div>
    </Blueprint>
  );
}
