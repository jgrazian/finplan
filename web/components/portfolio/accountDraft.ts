import type { Account, ContributionLimitPeriod, TaxStatus } from "@/lib/types";
import { type AccountKind, kindOf } from "./accountKind";

/** The fields a PATCH on the account can carry, across all five kinds. */
export interface AccountDraft {
  /** What the account is called; unique across the scenario, as the server has it. */
  name: string;
  kind: AccountKind;
  /** Investment flavor only; the kind sets it coarsely, Terms refines it. */
  taxStatus: TaxStatus | undefined;
  /**
   * The one figure the account stores itself, always positive: idle cash on an
   * investment account, the balance on cash, the market value on property, the
   * principal outstanding on debt. What it means is the kind's business, which
   * is exactly why the drawer only has to carry one of them.
   */
  amount: number;
  /** The profile the account owns — cash on an investment account, the rate on a bank one. */
  returnProfileServerId: number | undefined;
  /** The asset a property is marked against. */
  assetServerId: number | undefined;
  /** Annual rate as a fraction; debt only. */
  interestRate: number;
  contributionLimit: number | null;
  contributionPeriod: ContributionLimitPeriod;
}

export type SetDraft = <K extends keyof AccountDraft>(
  key: K,
  value: AccountDraft[K],
) => void;

function amountOf(account: Account): number {
  switch (account.flavor) {
    case "Bank":
    case "Investment":
      return account.cashValue;
    case "Property":
      return account.balance;
    case "Liability":
      // Stored as a positive amount owed; only the list signs it.
      return -account.balance;
  }
}

export function draftOf(account: Account): AccountDraft {
  return {
    name: account.name,
    kind: kindOf(account),
    taxStatus: account.taxStatus,
    amount: amountOf(account),
    returnProfileServerId: account.returnProfileServerId,
    assetServerId: account.assetServerId,
    interestRate: account.interestRate ?? 0,
    contributionLimit: account.contributionLimit?.amount ?? null,
    contributionPeriod: account.contributionLimit?.period ?? "Yearly",
  };
}

/** Which fields hold an edit — one flag per thing the footer can count. */
export interface ChangedFields {
  name: boolean;
  kind: boolean;
  taxStatus: boolean;
  amount: boolean;
  profile: boolean;
  asset: boolean;
  rate: boolean;
  limit: boolean;
}

export function changedFields(
  draft: AccountDraft,
  pristine: AccountDraft,
): ChangedFields {
  return {
    name: draft.name !== pristine.name,
    kind: draft.kind !== pristine.kind,
    // A kind change moves the tax status with it; counting both would report
    // two edits for one decision.
    taxStatus: draft.kind === pristine.kind && draft.taxStatus !== pristine.taxStatus,
    amount: draft.amount !== pristine.amount,
    profile: draft.returnProfileServerId !== pristine.returnProfileServerId,
    asset: draft.assetServerId !== pristine.assetServerId,
    rate: draft.interestRate !== pristine.interestRate,
    limit:
      draft.contributionLimit !== pristine.contributionLimit ||
      // A period on its own says nothing until there is a figure to divide.
      (draft.contributionLimit != null &&
        draft.contributionPeriod !== pristine.contributionPeriod),
  };
}

export function pendingCount(changed: ChangedFields): number {
  return Object.values(changed).filter(Boolean).length;
}
