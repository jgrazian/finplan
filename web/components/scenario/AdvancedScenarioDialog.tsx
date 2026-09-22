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
import { addYears, yearsBetween } from "@/lib/view/format";

/**
 * Creates a scenario.
 *
 * The birth date is offered up front rather than left for later because the
 * server rejects an `Age` trigger — the natural way to say "retire at 62" — in
 * a scenario that has none.
 */
export function AdvancedScenarioDialog({
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
  const end = start && Number.isInteger(years) && years > 0 && years <= 120
    ? addYears(start, years)
    : null;

  return (
    <Dialog
      title="New scenario"
      onClose={onClose}
      onSubmit={() =>
        submit.run(
          async () => {
            if (!name.trim()) throw new Error("Enter a scenario name.");
            if (!end) throw new Error("Plan length must be a whole number from 1 to 120 years.");
            if (birth && birth > start) throw new Error("Birth date must be on or before the plan start date.");
            onCreated(
              await api.scenarios.create({
                name: name.trim(),
                start_date: start,
                birth_date: birth.trim() === "" ? null : birth,
                duration_years: years,
                inflation_profile_id: inflationId || null,
                tax_config_id: taxId || null,
              }),
            );
          },
          onClose,
        )
      }
      submitLabel="Create scenario"
      busy={submit.busy}
      error={submit.error}
    >
      <Field label="Name">
        <Input aria-label="Scenario name" value={name} onChange={(e) => setName(e.target.value)} required />
      </Field>
      <DialogRow>
        <Field label="Start date">
          <DateInput value={start} onValueChange={setStart} required />
        </Field>
        <Field label="Plan length">
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
      <Field label="Birth date — to plan by age">
        <DateInput value={birth} onValueChange={setBirth} />
      </Field>
      {end && (
        <p role="status" style={{ margin: 0, fontSize: 12 }}>
          This plan ends on {end}{birth ? `, at age ${Math.floor(yearsBetween(birth, end))}` : ""}.
          {" "}Choose a length that covers the years you want to plan for.
        </p>
      )}
      <DialogRow>
        <Field label="Inflation profile">
          <Select aria-label="Inflation profile" value={inflationId} onChange={(e) => setInflationId(Number(e.target.value))}>
            <option value={0}>none</option>
            {inflationProfiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Tax assumptions">
          <Select aria-label="Tax assumptions" value={taxId} onChange={(e) => setTaxId(Number(e.target.value))}>
            <option value={0}>none</option>
            {taxConfigs.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </Select>
        </Field>
      </DialogRow>
      <p style={{ margin: 0, fontSize: 12 }}>
        Review the tax profile&apos;s year and filing status before creating your plan.
        Next, add your accounts,
        income, spending, and funding rules.
      </p>
    </Dialog>
  );
}
