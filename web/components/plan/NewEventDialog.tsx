"use client";

import { useState } from "react";
import { Dialog, DialogRow, Field, Hr, Input } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Account, Asset, Event as ApiEvent } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { EffectsBlock } from "./EffectFields";
import { ScheduleFields, TriggerFields, TriggerFormSelect, hasSchedule } from "./TriggerFields";
import {
  type TriggerDraft,
  emptyTrigger,
  toTriggerSpec,
  triggerProblem,
} from "./triggerDraft";
import {
  type EffectDraft,
  effectProblem,
  emptyEffect,
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
  birthDate,
  onClose,
  onCreated,
}: {
  scenarioId: number;
  accounts: Account[];
  assets: Asset[];
  events: ApiEvent[];
  /** The scenario's, so an age condition can say the date it lands on. */
  birthDate?: string;
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
  const context = { accounts, assets, events, birthDate };

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
          <TriggerFormSelect trigger={trigger} onChange={setTrigger} />
        </Field>
      </DialogRow>

      <TriggerFields trigger={trigger} context={context} onChange={setTrigger} />
      {hasSchedule(trigger) && (
        <ScheduleFields trigger={trigger} context={context} onChange={setTrigger} />
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

      <EffectsBlock effects={effects} context={context} onChange={setEffects} />
    </Dialog>
  );
}
