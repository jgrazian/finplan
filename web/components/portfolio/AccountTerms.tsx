"use client";

import { CompactInput, CurrencyInput, DirtyField, Dropdown, Field, NumberInput } from "@/components/ui";
import type { Account as ApiAccount, Asset, Profile } from "@/lib/api/types";
import { termLabel } from "@/lib/view/events";
import { fmtCurrency } from "@/lib/format";
import type { Account, TaxStatus } from "@/lib/types";
import type { AccountDraft, ChangedFields, SetDraft } from "./accountDraft";

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
  payers,
  offline,
}: {
  account: Account;
  draft: AccountDraft;
  pristine: AccountDraft;
  changed: ChangedFields;
  set: SetDraft;
  profiles: Profile[];
  assets: Asset[];
  /** Accounts a loan's monthly payment can be drawn from. */
  payers: ApiAccount[];
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
          {/* Repayment: without a payer the loan only accrues, and nothing
              pays it down unless an event does. */}
          <DirtyField label="Paid from" changed={changed.repayment}>
            <Dropdown
              className="dd-field"
              options={[
                { value: 0, label: "not repaid" },
                ...payers.map((a) => ({ value: a.id, label: a.name, detail: a.flavor })),
              ]}
              value={draft.repayFrom ?? 0}
              disabled={offline}
              onChange={(id) => set("repayFrom", id === 0 ? null : id)}
              ariaLabel="Paid from"
            />
          </DirtyField>
          {draft.repayFrom != null ? (
            <DirtyField label={`Term · ${termLabel(draft.termMonths)}`} changed={changed.repayment}>
              <NumberInput
                style={{ minHeight: 32 }}
                value={draft.termMonths}
                decimals={0}
                suffix="months"
                min={1}
                max={600}
                readOnly={offline}
                onValueChange={(months) => set("termMonths", Math.max(1, Math.round(months)))}
                aria-label="Months left to pay"
              />
            </DirtyField>
          ) : (
            <span />
          )}
          {draft.repayFrom != null && (
            <Field label="Monthly payment">
              <CompactInput
                value={fmtCurrency(monthlyPayment(draft.amount, draft.interestRate, draft.termMonths))}
                readOnly
                title="Level payment that clears the principal over the term, at this rate."
              />
            </Field>
          )}
        </div>
      );
  }
}

/**
 * The level monthly payment the engine will make: the same amortization, at
 * the monthly rate equivalent to the annual one it accrues at.
 */
export function monthlyPayment(principal: number, rate: number, months: number): number {
  const n = Math.max(1, months);
  const monthly = Math.pow(1 + rate, 1 / 12) - 1;
  if (Math.abs(monthly) < 1e-12) return principal / n;
  return (principal * monthly) / (1 - Math.pow(1 + monthly, -n));
}
