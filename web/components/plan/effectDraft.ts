/**
 * The effect shapes the event form offers, and their translation to
 * `EffectSpec`.
 *
 * Covered: the money movements a plan is built from, plus the four effects that
 * drive other events. Not covered by the form — `Random`, `RsuVesting`,
 * `AdjustBalance`, `DeleteAccount` and a sweep's `Custom` source list — which
 * the API still accepts and the Plan screen still displays.
 */
import type { AmountSpec, EffectSpec, WithdrawalStrategy } from "@/lib/api/types";

export const EFFECT_FORMS = [
  "Income",
  "Expense",
  "CashTransfer",
  "AssetPurchase",
  "AssetSale",
  "Sweep",
  "ApplyRmd",
  "TriggerEvent",
  "PauseEvent",
  "ResumeEvent",
  "TerminateEvent",
] as const;
export type EffectForm = (typeof EFFECT_FORMS)[number];

export const STRATEGIES: WithdrawalStrategy[] = [
  "TaxEfficientEarly",
  "TaxDeferredFirst",
  "TaxFreeFirst",
  "ProRata",
  "PenaltyAware",
];

export interface EffectDraft {
  form: EffectForm;
  /** Dollars per occurrence — every form that shows it moves cash. */
  amount: number;
  /** Grow the amount with inflation, i.e. it is stated in today's money. */
  inflationAdjusted: boolean;
  fromAccountId: number;
  toAccountId: number;
  assetId: number;
  targetEventId: number;
  strategy: WithdrawalStrategy;
  taxFree: boolean;
}

export function emptyEffect(accountId: number, assetId: number): EffectDraft {
  return {
    form: "Income",
    amount: 0,
    inflationAdjusted: true,
    fromAccountId: accountId,
    toAccountId: accountId,
    assetId,
    targetEventId: 0,
    strategy: "TaxEfficientEarly",
    taxFree: false,
  };
}

/** Which fields a form needs, so the editor renders only those. */
export function shape(form: EffectForm): {
  from: boolean;
  to: boolean;
  asset: boolean;
  amount: boolean;
  event: boolean;
  strategy: boolean;
  taxable: boolean;
} {
  switch (form) {
    case "Income":
      return { from: false, to: true, asset: false, amount: true, event: false, strategy: false, taxable: true };
    case "Expense":
      return { from: true, to: false, asset: false, amount: true, event: false, strategy: false, taxable: false };
    case "CashTransfer":
      return { from: true, to: true, asset: false, amount: true, event: false, strategy: false, taxable: false };
    case "AssetPurchase":
      return { from: true, to: true, asset: true, amount: true, event: false, strategy: false, taxable: false };
    case "AssetSale":
      return { from: true, to: false, asset: true, amount: true, event: false, strategy: false, taxable: false };
    case "Sweep":
      return { from: false, to: true, asset: false, amount: true, event: false, strategy: true, taxable: true };
    case "ApplyRmd":
      return { from: false, to: true, asset: false, amount: false, event: false, strategy: false, taxable: false };
    default:
      return { from: false, to: false, asset: false, amount: false, event: true, strategy: false, taxable: false };
  }
}

/** Why this draft cannot be sent yet, or null when it can. */
export function effectProblem(draft: EffectDraft, index: number): string | null {
  if (shape(draft.form).event && draft.targetEventId === 0) {
    return `Effect ${index + 1}: ${draft.form} needs a target event.`;
  }
  return null;
}

function amountSpec(draft: EffectDraft): AmountSpec {
  const fixed: AmountSpec = { kind: "Fixed", value: draft.amount };
  return draft.inflationAdjusted ? { kind: "InflationAdjusted", inner: fixed } : fixed;
}

export function toEffectSpec(draft: EffectDraft): EffectSpec {
  const amount = amountSpec(draft);
  const incomeType = draft.taxFree ? "TaxFree" : "Taxable";

  switch (draft.form) {
    case "Income":
      return {
        kind: "Income",
        to_account_id: draft.toAccountId,
        amount,
        amount_mode: "Net",
        income_type: incomeType,
      };
    case "Expense":
      return { kind: "Expense", from_account_id: draft.fromAccountId, amount };
    case "CashTransfer":
      return {
        kind: "CashTransfer",
        from_account_id: draft.fromAccountId,
        to_account_id: draft.toAccountId,
        amount,
      };
    case "AssetPurchase":
      return {
        kind: "AssetPurchase",
        from_account_id: draft.fromAccountId,
        to_account_id: draft.toAccountId,
        asset_id: draft.assetId,
        amount,
      };
    case "AssetSale":
      return {
        kind: "AssetSale",
        from_account_id: draft.fromAccountId,
        asset_id: draft.assetId,
        amount,
        amount_mode: "Net",
        lot_method: "Fifo",
      };
    case "Sweep":
      return {
        kind: "Sweep",
        to_account_id: draft.toAccountId,
        amount,
        sources: { mode: "Strategy", strategy: draft.strategy, exclude_accounts: [] },
        amount_mode: "Net",
        lot_method: "Fifo",
        income_type: incomeType,
      };
    case "ApplyRmd":
      return { kind: "ApplyRmd", to_account_id: draft.toAccountId, lot_method: "Fifo" };
    default:
      return { kind: draft.form, target_event_id: draft.targetEventId };
  }
}
