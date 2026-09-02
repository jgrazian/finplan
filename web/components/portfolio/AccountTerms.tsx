"use client";

import { CompactInput, CurrencyInput, Dropdown, Field, NumberInput } from "@/components/ui";
import type { Asset, Profile } from "@/lib/api/types";
import { fmtCurrency } from "@/lib/format";
import type { Account, TaxStatus } from "@/lib/types";
import type { AccountDraft, ChangedFields, SetDraft } from "./accountDraft";
import { DirtyField } from "./DirtyField";

const GRID = { display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 } as const;

const TREATMENTS: ReadonlyArray<{ value: TaxStatus; label: string; detail: string }> = [
  { value: "TaxDeferred", label: "Tax-deferred", detail: "traditional" },
  { value: "TaxFree", label: "Tax-free", detail: "Roth" },
];

/**
 * Block 2 — the kind-specific fields.
 *
 * With Linked, this is the only block whose *content* varies: everything else
 * in the drawer is the same component with different data. A field the engine
 * does not read for this kind is absent, never disabled — there is no greyed
 * contribution limit on a brokerage.
 */
export function AccountTerms({
  account,
  draft,
  pristine,
  changed,
  set,
  profiles,
  assets,
  offline,
}: {
  account: Account;
  draft: AccountDraft;
  pristine: AccountDraft;
  changed: ChangedFields;
  set: SetDraft;
  profiles: Profile[];
  assets: Asset[];
  offline?: boolean;
}) {
  const profileField = (label: string) => (
    <DirtyField label={label} changed={changed.profile}>
      <Dropdown
        className="dd-field"
        options={profiles.map((p) => ({ value: p.id, label: p.name }))}
        value={draft.returnProfileServerId ?? null}
        disabled={offline}
        onChange={(id) => set("returnProfileServerId", id)}
        ariaLabel={label}
      />
    </DirtyField>
  );

  switch (draft.kind) {
    case "investment":
    case "retirement":
      // Both terms are the cash's. The holdings carry their own profile and
      // their own value, so neither is a field on the account — what they add
      // up to is block 3's business.
      return (
        <div style={GRID}>
          {profileField("Cash profile")}
          <DirtyField label="Cash balance" changed={changed.amount}>
            <CurrencyInput
              style={{ minHeight: 32 }}
              value={draft.amount}
              readOnly={offline}
              onValueChange={(amount) => set("amount", amount)}
              aria-label="Cash balance"
            />
            {changed.amount && (
              <div className="field-was">was {fmtCurrency(pristine.amount)}</div>
            )}
          </DirtyField>
          {draft.kind === "retirement" && (
            <>
              <DirtyField label="Tax treatment" changed={changed.taxStatus}>
                <Dropdown
                  className="dd-field"
                  options={TREATMENTS}
                  value={draft.taxStatus ?? "TaxDeferred"}
                  disabled={offline}
                  onChange={(status) => set("taxStatus", status)}
                  ariaLabel="Tax treatment"
                />
              </DirtyField>
              <DirtyField label="Contribution limit" changed={changed.limit}>
                <CurrencyInput
                  style={{ minHeight: 32 }}
                  nullable
                  value={draft.contributionLimit}
                  placeholder="none"
                  readOnly={offline}
                  onValueChange={(limit) => set("contributionLimit", limit)}
                  aria-label="Contribution limit"
                  suffix={
                    <button
                      type="button"
                      className="numfield-unit"
                      disabled={offline}
                      onClick={() =>
                        set(
                          "contributionPeriod",
                          draft.contributionPeriod === "Yearly" ? "Monthly" : "Yearly",
                        )
                      }
                      title="Switch between a yearly and a monthly limit"
                    >
                      / {draft.contributionPeriod === "Yearly" ? "yr" : "mo"}
                    </button>
                  }
                />
              </DirtyField>
            </>
          )}
        </div>
      );

    case "cash":
      return profileField("Rate profile");

    case "property":
      return (
        <div style={GRID}>
          <Field label="Appreciation">
            <CompactInput
              value={account.returnProfileId}
              readOnly
              title="A property grows by the profile of the asset backing it."
            />
          </Field>
          <DirtyField label="Backing asset" changed={changed.asset}>
            <Dropdown
              className="dd-field"
              options={assets.map((a) => ({ value: a.id, label: a.name }))}
              value={draft.assetServerId ?? null}
              disabled={offline}
              onChange={(id) => set("assetServerId", id)}
              ariaLabel="Backing asset"
            />
          </DirtyField>
        </div>
      );

    case "debt":
      return (
        <div style={GRID}>
          <DirtyField label="Rate" changed={changed.rate}>
            <NumberInput
              style={{ minHeight: 32 }}
              value={draft.interestRate * 100}
              decimals={2}
              suffix="%"
              min={0}
              max={100}
              readOnly={offline}
              onValueChange={(pct) => set("interestRate", pct / 100)}
              aria-label="Annual interest rate"
            />
          </DirtyField>
          <Field label="Annual interest">
            <CompactInput
              value={fmtCurrency(draft.amount * draft.interestRate)}
              readOnly
              title="The principal beside it, at that rate, for a full year."
            />
          </Field>
        </div>
      );
  }
}
