"use client";

import { useState } from "react";
import {
  Blueprint,
  Button,
  CurrencyInput,
  Dialog,
  DialogRow,
  Field,
  Hr,
  Input,
  SectionHeading,
  Select,
} from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Account, Asset, Event as ApiEvent } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { TriggerFields } from "./TriggerFields";
import {
  TRIGGER_FORMS,
  type TriggerDraft,
  type TriggerForm,
  emptyTrigger,
  toTriggerSpec,
  triggerProblem,
  withForm,
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
  const [trigger, setTrigger] = useState<TriggerDraft>(() =>
    emptyTrigger(firstAccount, firstAsset),
  );
  const [effects, setEffects] = useState<EffectDraft[]>([
    emptyEffect(firstAccount, firstAsset),
  ]);
  const submit = useSubmit();

  const patch = (index: number, change: Partial<EffectDraft>) =>
    setEffects((list) => list.map((e, i) => (i === index ? { ...e, ...change } : e)));

  const create = () => {
    // A missing target would trip a foreign key. The server answers 400 for
    // that, but naming the offending field here saves the round trip.
    const problem =
      triggerProblem(trigger) ?? effects.map(effectProblem).find((p) => p != null);
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
            onChange={(e) => setTrigger(withForm(trigger, e.target.value as TriggerForm))}
          >
            {TRIGGER_FORMS.map((f) => (
              <option key={f}>{f}</option>
            ))}
          </Select>
        </Field>
      </DialogRow>

      <TriggerFields
        trigger={trigger}
        context={{ accounts, assets, events }}
        onChange={setTrigger}
      />

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
