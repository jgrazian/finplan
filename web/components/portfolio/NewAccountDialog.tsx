"use client";

import { useState } from "react";
import { CurrencyInput, Dialog, DialogRow, Field, Input, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import type {
  Asset,
  ContributionPeriod,
  CreateAccount,
  FlavorSpec,
  Profile,
  TaxStatus,
} from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";

const FLAVORS = ["Bank", "Investment", "Property", "Liability"] as const;
type Flavor = (typeof FLAVORS)[number];

const TAX_STATUSES: TaxStatus[] = ["Taxable", "TaxDeferred", "TaxFree"];
const PERIODS: ContributionPeriod[] = ["Yearly", "Monthly"];

/**
 * Creates an account of any of the four flavors.
 *
 * The flavor cannot be changed afterwards — the server refuses, because
 * switching a 401k into a mortgage would invalidate every event and position
 * pointing at it — so it is chosen here and only here.
 */
export function NewAccountDialog({
  scenarioId,
  profiles,
  assets,
  onClose,
  onCreated,
}: {
  scenarioId: number;
  profiles: Profile[];
  assets: Asset[];
  onClose: () => void;
  onCreated: () => void;
}) {
  const [flavor, setFlavor] = useState<Flavor>("Bank");
  const [name, setName] = useState("");
  const [cash, setCash] = useState(0);
  const [profileId, setProfileId] = useState(profiles[0]?.id ?? 0);
  const [taxStatus, setTaxStatus] = useState<TaxStatus>("Taxable");
  const [limit, setLimit] = useState<number | null>(null);
  const [period, setPeriod] = useState<ContributionPeriod>("Yearly");
  const [assetId, setAssetId] = useState(assets[0]?.id ?? 0);
  const [value, setValue] = useState(0);
  const [principal, setPrincipal] = useState(0);
  const [rate, setRate] = useState("6.0");

  const submit = useSubmit();

  const spec = (): FlavorSpec => {
    switch (flavor) {
      case "Bank":
        return { flavor, cash_value: cash, return_profile_id: profileId };
      case "Investment":
        return {
          flavor,
          tax_status: taxStatus,
          cash_value: cash,
          cash_return_profile_id: profileId,
          // The server rejects one without the other, so they travel together.
          contribution_limit: limit,
          contribution_period: limit == null ? null : period,
        };
      case "Property":
        return { flavor, asset_id: assetId, value };
      case "Liability":
        // Stored as a positive amount owed, and the rate as a fraction.
        return { flavor, principal, interest_rate: (Number(rate) || 0) / 100 };
    }
  };

  const create = () => {
    const body: CreateAccount = { name, sort_order: 0, ...spec() };
    submit.run(() => api.accounts.create(scenarioId, body), () => {
      onCreated();
      onClose();
    });
  };

  const noAssets = flavor === "Property" && assets.length === 0;

  return (
    <Dialog
      title="New account"
      onClose={onClose}
      onSubmit={create}
      submitLabel="Create account"
      busy={submit.busy}
      error={
        noAssets
          ? "A property account is valued by an asset. Create an asset first."
          : submit.error
      }
    >
      <DialogRow>
        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} required />
        </Field>
        <Field label="Flavor">
          <Select value={flavor} onChange={(e) => setFlavor(e.target.value as Flavor)}>
            {FLAVORS.map((f) => (
              <option key={f}>{f}</option>
            ))}
          </Select>
        </Field>
      </DialogRow>

      {flavor === "Bank" && (
        <DialogRow>
          <Field label="Opening balance">
            <CurrencyInput value={cash} onValueChange={setCash} aria-label="Opening balance" />
          </Field>
          <ProfileField value={profileId} onChange={setProfileId} profiles={profiles} label="Return profile" />
        </DialogRow>
      )}

      {flavor === "Investment" && (
        <>
          <DialogRow>
            <Field label="Tax status">
              <Select value={taxStatus} onChange={(e) => setTaxStatus(e.target.value as TaxStatus)}>
                {TAX_STATUSES.map((t) => (
                  <option key={t}>{t}</option>
                ))}
              </Select>
            </Field>
            <Field label="Opening cash">
              <CurrencyInput value={cash} onValueChange={setCash} aria-label="Opening cash" />
            </Field>
          </DialogRow>
          <ProfileField
            value={profileId}
            onChange={setProfileId}
            profiles={profiles}
            label="Cash return profile — holdings grow by their asset's own profile"
          />
          <DialogRow>
            <Field label="Contribution limit (optional)">
              <CurrencyInput
                nullable
                value={limit}
                placeholder="none"
                onValueChange={setLimit}
                aria-label="Contribution limit"
              />
            </Field>
            <Field label="Limit period">
              <Select
                value={period}
                disabled={limit == null}
                onChange={(e) => setPeriod(e.target.value as ContributionPeriod)}
              >
                {PERIODS.map((p) => (
                  <option key={p}>{p}</option>
                ))}
              </Select>
            </Field>
          </DialogRow>
        </>
      )}

      {flavor === "Property" && (
        <DialogRow>
          <Field label="Underlying asset">
            <Select value={assetId} onChange={(e) => setAssetId(Number(e.target.value))}>
              {assets.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Value">
            <CurrencyInput value={value} onValueChange={setValue} aria-label="Value" />
          </Field>
        </DialogRow>
      )}

      {flavor === "Liability" && (
        <DialogRow>
          <Field label="Principal owed">
            <CurrencyInput
              value={principal}
              onValueChange={setPrincipal}
              aria-label="Principal owed"
            />
          </Field>
          <Field label="Interest rate (%)">
            <Input type="number" step="any" value={rate} onChange={(e) => setRate(e.target.value)} />
          </Field>
        </DialogRow>
      )}
    </Dialog>
  );
}

function ProfileField({
  value,
  onChange,
  profiles,
  label,
}: {
  value: number;
  onChange: (id: number) => void;
  profiles: Profile[];
  label: string;
}) {
  return (
    <Field label={label}>
      <Select value={value} onChange={(e) => onChange(Number(e.target.value))}>
        {profiles.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </Select>
    </Field>
  );
}
