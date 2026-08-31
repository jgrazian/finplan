"use client";

import { useState } from "react";
import {
  DateInput,
  Dialog,
  DialogRow,
  Field,
  Input,
  NumberInput,
  Select,
} from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Profile, Scenario, TaxConfig, UserResponse } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";

/**
 * Creates a scenario.
 *
 * The birth date is offered up front rather than left for later because the
 * server rejects an `Age` trigger — the natural way to say "retire at 62" — in
 * a scenario that has none.
 */
export function NewScenarioDialog({
  defaults,
  inflationProfiles,
  taxConfigs,
  onClose,
  onCreated,
}: {
  /** Account settings: the birth date and horizon a new scenario inherits. */
  defaults: UserResponse;
  inflationProfiles: Profile[];
  taxConfigs: TaxConfig[];
  onClose: () => void;
  onCreated: (scenario: Scenario) => void;
}) {
  const [name, setName] = useState("");
  const [start, setStart] = useState(new Date().toISOString().slice(0, 10));
  const [birth, setBirth] = useState(defaults.birth_date ?? "");
  const [years, setYears] = useState(defaults.default_duration_years);
  const [inflationId, setInflationId] = useState(inflationProfiles[0]?.id ?? 0);
  const [taxId, setTaxId] = useState(taxConfigs[0]?.id ?? 0);
  const submit = useSubmit();

  return (
    <Dialog
      title="New scenario"
      onClose={onClose}
      onSubmit={() =>
        submit.run(
          async () =>
            onCreated(
              await api.scenarios.create({
                name,
                start_date: start,
                birth_date: birth.trim() === "" ? null : birth,
                duration_years: years || defaults.default_duration_years,
                inflation_profile_id: inflationId || null,
                tax_config_id: taxId || null,
              }),
            ),
          onClose,
        )
      }
      submitLabel="Create scenario"
      busy={submit.busy}
      error={submit.error}
    >
      <Field label="Name">
        <Input value={name} onChange={(e) => setName(e.target.value)} required />
      </Field>
      <DialogRow>
        <Field label="Start date">
          <DateInput value={start} onChange={(e) => setStart(e.target.value)} required />
        </Field>
        <Field label="Horizon">
          <NumberInput
            value={years}
            suffix="years"
            decimals={0}
            min={1}
            max={120}
            onValueChange={setYears}
            aria-label="Horizon in years"
          />
        </Field>
      </DialogRow>
      <Field label="Birth date — required for age-based triggers">
        <DateInput value={birth} onChange={(e) => setBirth(e.target.value)} />
      </Field>
      <DialogRow>
        <Field label="Inflation profile">
          <Select value={inflationId} onChange={(e) => setInflationId(Number(e.target.value))}>
            <option value={0}>none</option>
            {inflationProfiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Tax config">
          <Select value={taxId} onChange={(e) => setTaxId(Number(e.target.value))}>
            <option value={0}>none</option>
            {taxConfigs.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </Select>
        </Field>
      </DialogRow>
    </Dialog>
  );
}
