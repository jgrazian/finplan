"use client";

import { useState } from "react";
import { Dialog, DialogRow, Field, Input, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Profile, Scenario, TaxConfig } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";

/**
 * Creates a scenario.
 *
 * The birth date is offered up front rather than left for later because the
 * server rejects an `Age` trigger — the natural way to say "retire at 62" — in
 * a scenario that has none.
 */
export function NewScenarioDialog({
  inflationProfiles,
  taxConfigs,
  onClose,
  onCreated,
}: {
  inflationProfiles: Profile[];
  taxConfigs: TaxConfig[];
  onClose: () => void;
  onCreated: (scenario: Scenario) => void;
}) {
  const [name, setName] = useState("");
  const [start, setStart] = useState(new Date().toISOString().slice(0, 10));
  const [birth, setBirth] = useState("");
  const [years, setYears] = useState("30");
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
                duration_years: Number(years) || 30,
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
          <Input type="date" value={start} onChange={(e) => setStart(e.target.value)} required />
        </Field>
        <Field label="Horizon (years)">
          <Input type="number" value={years} onChange={(e) => setYears(e.target.value)} />
        </Field>
      </DialogRow>
      <Field label="Birth date — required for age-based triggers">
        <Input type="date" value={birth} onChange={(e) => setBirth(e.target.value)} />
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
