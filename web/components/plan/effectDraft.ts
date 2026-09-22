/**
 * The effect shapes the event form offers, and their translation to and from
 * `EffectSpec`.
 *
 * Covered: every effect the engine has except `Random`, which branches into two
 * more effects and would make this a tree editor rather than a list one.
 *
 * Fourteen engine kinds in eleven forms: the four event-control effects differ
 * only in their verb, and a block whose one field is an event id should not be
 * drawn four ways, so they share a form and the verb is a select inside it.
 *
 * Amounts have two settings rather than one. An effect's amount is its own
 * recursive language — `Min(AccountCashBalance, Scale(0.04, Fixed))` is a legal
 * withdrawal rule — but almost every amount anyone writes is a figure, so the
 * form asks for a figure and keeps the tree out of the way. `rawAmount` holds
 * the expression whenever there is one, either because it came off the wire
 * that way or because the editor was asked for one, and `AmountExpression`
 * draws it in full: nothing about an amount is beyond this form.
 *
 * What is still held verbatim is a sweep's source list, in `rawSources`, and a
 * whole `Random`, in `raw`. Editing an event's name never rewrites either.
 */
import type { TagTone } from "@/components/ui/Tag";
import type {
  AmountMode,
  AmountSpec,
  EffectSpec,
  LotMethod,
  WithdrawalSourcesSpec,
  WithdrawalStrategy,
} from "@/lib/api/types";
import { amountProblem } from "./amountDraft";

export const EFFECT_FORMS = [
  "Income",
  "Expense",
  "CashTransfer",
  "AssetPurchase",
  "AssetSale",
  "Sweep",
  "AdjustBalance",
  "ApplyRmd",
  "RsuVesting",
  "DeleteAccount",
  "Event control",
] as const;
export type EffectForm = (typeof EFFECT_FORMS)[number];

/** The four effects that differ only in what they do to another event. */
export const VERBS = [
  "TriggerEvent",
  "PauseEvent",
  "ResumeEvent",
  "TerminateEvent",
] as const;
export type Verb = (typeof VERBS)[number];

/**
 * What each form is for, as a tag beside its name.
 *
 * Eleven cards is more than the eye sorts unaided; four families is not. The
 * groups are the design's: what moves cash, what moves holdings, what reaches
 * across accounts, what writes a balance outright, and what drives other events.
 */
export const FAMILY: Record<EffectForm, { label: string; tone: TagTone }> = {
  Income: { label: "cash flow", tone: "accent" },
  Expense: { label: "cash flow", tone: "accent" },
  CashTransfer: { label: "cash flow", tone: "accent" },
  AssetPurchase: { label: "assets", tone: "outline" },
  AssetSale: { label: "assets", tone: "outline" },
  RsuVesting: { label: "assets", tone: "outline" },
  Sweep: { label: "multi-account", tone: "accent" },
  ApplyRmd: { label: "multi-account", tone: "accent" },
  AdjustBalance: { label: "adjustment", tone: "neutral" },
  DeleteAccount: { label: "accounts", tone: "outline" },
  "Event control": { label: "event control", tone: "neutral" },
};

export const STRATEGIES: WithdrawalStrategy[] = [
  "TaxEfficientEarly",
  "TaxDeferredFirst",
  "TaxFreeFirst",
  "ProRata",
  "PenaltyAware",
];

export const LOT_METHODS: LotMethod[] = [
  "Fifo",
  "Lifo",
  "HighestCost",
  "LowestCost",
  "AverageCost",
];

/** How an amount reads: what lands, or what leaves before tax takes its cut. */
export const AMOUNT_MODES: { value: AmountMode; label: string }[] = [
  { value: "Net", label: "after tax" },
  { value: "Gross", label: "before tax" },
];

/** Stable keys, so removing one card does not re-key the ones below it. */
let lastUid = 0;

export interface EffectDraft {
  /** Render key only — never sent, and never read back off the wire. */
  uid: number;
  form: EffectForm;
  /** Dollars per occurrence — every form that shows it moves cash. */
  amount: number;
  /** Grow the amount with inflation, i.e. it is stated in today's money. */
  inflationAdjusted: boolean;
  /**
   * The amount as an expression. Set, it is what gets written and the two
   * fields above mean nothing — see `expandAmount` and `collapseAmount`.
   */
  rawAmount?: AmountSpec;
  /** Whether the amount is what arrives, or what is taken before tax. */
  amountMode: AmountMode;
  fromAccountId: number;
  toAccountId: number;
  assetId: number;
  /** `AssetSale` only: sell across every holding rather than a named one. */
  anyAsset: boolean;
  /** `RsuVesting` only. */
  units: number;
  sellToCover: boolean;
  targetEventId: number;
  /** Which of the four event-control effects the one form is standing in for. */
  verb: Verb;
  strategy: WithdrawalStrategy;
  /** A sweep's sources where they are not a plain strategy — held verbatim. */
  rawSources?: WithdrawalSourcesSpec;
  /** Which lots a sale reaches for. */
  lotMethod: LotMethod;
  taxFree: boolean;
  /** An effect the form cannot express (`Random`) — held verbatim. */
  raw?: EffectSpec;
}

export function emptyEffect(accountId: number, assetId: number): EffectDraft {
  return {
    uid: (lastUid += 1),
    form: "Income",
    amount: 0,
    inflationAdjusted: true,
    amountMode: "Net",
    fromAccountId: accountId,
    toAccountId: accountId,
    assetId,
    anyAsset: false,
    units: 0,
    sellToCover: false,
    targetEventId: 0,
    verb: "TriggerEvent",
    strategy: "TaxEfficientEarly",
    lotMethod: "Fifo",
    taxFree: false,
  };
}

/** Which fields a form needs, so the editor renders only those. */
export interface EffectShape {
  from: boolean;
  to: boolean;
  /** One account the effect acts on, rather than a direction between two. */
  account: boolean;
  asset: boolean;
  /** The asset may be left unnamed, meaning "whatever is in the account". */
  anyAsset: boolean;
  amount: boolean;
  /** The amount can be read gross or net of tax. */
  mode: boolean;
  units: boolean;
  event: boolean;
  /** The event-control form, whose verb picks which effect it really is. */
  verb: boolean;
  strategy: boolean;
  lots: boolean;
  taxable: boolean;
}

const NOTHING: EffectShape = {
  from: false,
  to: false,
  account: false,
  asset: false,
  anyAsset: false,
  amount: false,
  mode: false,
  units: false,
  event: false,
  verb: false,
  strategy: false,
  lots: false,
  taxable: false,
};

export function shape(form: EffectForm): EffectShape {
  switch (form) {
    case "Income":
      return { ...NOTHING, to: true, amount: true, mode: true, taxable: true };
    case "Expense":
      return { ...NOTHING, from: true, amount: true };
    case "CashTransfer":
      return { ...NOTHING, from: true, to: true, amount: true };
    case "AssetPurchase":
      return { ...NOTHING, from: true, to: true, asset: true, amount: true };
    case "AssetSale":
      return {
        ...NOTHING,
        from: true,
        asset: true,
        anyAsset: true,
        amount: true,
        mode: true,
        lots: true,
      };
    case "Sweep":
      return {
        ...NOTHING,
        to: true,
        amount: true,
        mode: true,
        strategy: true,
        lots: true,
        taxable: true,
      };
    case "AdjustBalance":
      return { ...NOTHING, account: true, amount: true };
    case "ApplyRmd":
      return { ...NOTHING, to: true, lots: true };
    case "RsuVesting":
      return { ...NOTHING, to: true, asset: true, units: true, lots: true };
    case "DeleteAccount":
      return { ...NOTHING, account: true };
    default:
      return { ...NOTHING, event: true, verb: true };
  }
}

/* ── changing kind ──────────────────────────────────────────────────────── */

/** What each term is called when a conversion is about to drop it. */
const TERM: Partial<Record<keyof EffectShape, string>> = {
  from: "the source account",
  to: "the destination account",
  account: "the account",
  asset: "the asset",
  units: "the units",
  mode: "whether the amount is gross or net",
  strategy: "the source order",
  lots: "the lot method",
  taxable: "the income type",
  event: "the target event",
};

/** `a, b and c` — the list as it reads in the sentence below. */
function series(parts: string[]): string {
  if (parts.length <= 1) return parts.join("");
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

/**
 * What converting this effect abandons, said before it commits.
 *
 * The drawer changes kind in place, so the field that was holding the lot
 * method simply stops being drawn. Saying which terms go — and that the amount
 * survives, which is the one people worry about — is the difference between a
 * conversion and a quiet loss.
 */
export function effectConversion(from: EffectForm, to: EffectForm): string | null {
  if (from === to) return null;
  const was = shape(from);
  const now = shape(to);

  const dropped = (Object.keys(TERM) as (keyof EffectShape)[])
    .filter((key) => was[key] && !now[key])
    .map((key) => TERM[key] as string);

  const keepsAmount = was.amount && now.amount;
  if (dropped.length === 0) {
    return keepsAmount || !was.amount
      ? null
      : `${from} → ${to} adds an amount; ${from} carried none.`;
  }

  const head = `${from} → ${to} `;
  const drops = `drops ${series(dropped)}.`;
  return keepsAmount ? `${head}keeps the amount and ${drops}` : head + drops;
}

/** Why this draft cannot be sent yet, or null when it can. */
export function effectProblem(draft: EffectDraft, index: number): string | null {
  const say = (problem: string) => `Effect ${index + 1}: ${draft.form} ${problem}.`;
  if (draft.raw) return null;
  const fields = shape(draft.form);
  if (fields.event && draft.targetEventId === 0) return say("needs a target event");
  if (fields.asset && !(fields.anyAsset && draft.anyAsset) && draft.assetId === 0) {
    return say("needs an asset");
  }
  if (fields.amount && draft.rawAmount) {
    const problem = amountProblem(draft.rawAmount);
    if (problem) return say(`has an amount that ${problem}`);
  }
  return null;
}

/* ── to the wire ────────────────────────────────────────────────────────── */

function amountSpec(draft: EffectDraft): AmountSpec {
  if (draft.rawAmount) return draft.rawAmount;
  const fixed: AmountSpec = { kind: "Fixed", value: draft.amount };
  return draft.inflationAdjusted ? { kind: "InflationAdjusted", inner: fixed } : fixed;
}

/**
 * Into the expression editor, and back out of it.
 *
 * Expanding is lossless by construction — the figure and its inflation tick
 * are exactly a one- or two-node tree, so the editor opens on what the field
 * was already saying. Collapsing only keeps a figure where the expression
 * still is one; anything else falls back to the figure the draft last held,
 * which is the one the field will show.
 */
export function expandAmount(draft: EffectDraft): Partial<EffectDraft> {
  return { rawAmount: amountSpec(draft) };
}

export function collapseAmount(draft: EffectDraft): Partial<EffectDraft> {
  const read = draft.rawAmount ? readAmount(draft.rawAmount) : null;
  return {
    rawAmount: undefined,
    ...(read ? { amount: read.value, inflationAdjusted: read.inflationAdjusted } : {}),
  };
}

function sourcesSpec(draft: EffectDraft): WithdrawalSourcesSpec {
  return (
    draft.rawSources ?? {
      mode: "Strategy",
      strategy: draft.strategy,
      exclude_accounts: [],
    }
  );
}

export function toEffectSpec(draft: EffectDraft): EffectSpec {
  if (draft.raw) return draft.raw;

  const amount = amountSpec(draft);
  const income_type = draft.taxFree ? "TaxFree" : "Taxable";
  const { amountMode: amount_mode, lotMethod: lot_method } = draft;

  switch (draft.form) {
    case "Income":
      return {
        kind: "Income",
        to_account_id: draft.toAccountId,
        amount,
        amount_mode,
        income_type,
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
        asset_id: draft.anyAsset ? null : draft.assetId,
        amount,
        amount_mode,
        lot_method,
      };
    case "Sweep":
      return {
        kind: "Sweep",
        to_account_id: draft.toAccountId,
        amount,
        sources: sourcesSpec(draft),
        amount_mode,
        lot_method,
        income_type,
      };
    case "AdjustBalance":
      return { kind: "AdjustBalance", account_id: draft.toAccountId, amount };
    case "ApplyRmd":
      return { kind: "ApplyRmd", to_account_id: draft.toAccountId, lot_method };
    case "RsuVesting":
      return {
        kind: "RsuVesting",
        to_account_id: draft.toAccountId,
        asset_id: draft.assetId,
        units: draft.units,
        sell_to_cover: draft.sellToCover,
        lot_method,
      };
    case "DeleteAccount":
      return { kind: "DeleteAccount", account_id: draft.toAccountId };
    default:
      return { kind: draft.verb, target_event_id: draft.targetEventId };
  }
}

/* ── from the wire ──────────────────────────────────────────────────────── */

/** A `Fixed`, alone or wrapped in one `InflationAdjusted` — what the form asks. */
function readAmount(
  amount: AmountSpec,
): { value: number; inflationAdjusted: boolean } | null {
  if (amount.kind === "Fixed") return { value: amount.value, inflationAdjusted: false };
  if (amount.kind === "InflationAdjusted" && amount.inner.kind === "Fixed") {
    return { value: amount.inner.value, inflationAdjusted: true };
  }
  return null;
}

/** A plain strategy, or nothing — a named account or a custom list is neither. */
function readStrategy(sources: WithdrawalSourcesSpec | null): WithdrawalStrategy | null {
  return sources?.mode === "Strategy" && sources.exclude_accounts.length === 0
    ? sources.strategy
    : null;
}

/**
 * A saved effect as a draft the editor can open.
 *
 * Every field it recognises lands in its own control; everything else lands in
 * one of the three `raw` holds and is written back exactly as it came.
 */
export function draftOfEffect(
  effect: EffectSpec,
  accountId: number,
  assetId: number,
): EffectDraft {
  const base = emptyEffect(accountId, assetId);

  const withAmount = (amount: AmountSpec): Partial<EffectDraft> => {
    const read = readAmount(amount);
    return read
      ? { amount: read.value, inflationAdjusted: read.inflationAdjusted }
      : { rawAmount: amount };
  };

  switch (effect.kind) {
    case "Income":
      return {
        ...base,
        form: "Income",
        toAccountId: effect.to_account_id,
        amountMode: effect.amount_mode,
        taxFree: effect.income_type === "TaxFree",
        ...withAmount(effect.amount),
      };
    case "Expense":
      return {
        ...base,
        form: "Expense",
        fromAccountId: effect.from_account_id,
        ...withAmount(effect.amount),
      };
    case "CashTransfer":
      return {
        ...base,
        form: "CashTransfer",
        fromAccountId: effect.from_account_id,
        toAccountId: effect.to_account_id,
        ...withAmount(effect.amount),
      };
    case "AssetPurchase":
      return {
        ...base,
        form: "AssetPurchase",
        fromAccountId: effect.from_account_id,
        toAccountId: effect.to_account_id,
        assetId: effect.asset_id,
        ...withAmount(effect.amount),
      };
    case "AssetSale":
      return {
        ...base,
        form: "AssetSale",
        fromAccountId: effect.from_account_id,
        anyAsset: effect.asset_id == null,
        assetId: effect.asset_id ?? base.assetId,
        amountMode: effect.amount_mode,
        lotMethod: effect.lot_method,
        ...withAmount(effect.amount),
      };
    case "Sweep": {
      const strategy = readStrategy(effect.sources);
      return {
        ...base,
        form: "Sweep",
        toAccountId: effect.to_account_id,
        amountMode: effect.amount_mode,
        lotMethod: effect.lot_method,
        taxFree: effect.income_type === "TaxFree",
        // A null source list compiles to the engine's own default, which is
        // this draft's default strategy — so it shows as TaxEfficientEarly and
        // saves as the same thing. A list the picker cannot draw — a named
        // account, a custom order, a strategy with exclusions — is held as it
        // stands rather than flattened to the strategy alone.
        ...(strategy
          ? { strategy }
          : effect.sources
            ? { rawSources: effect.sources }
            : {}),
        ...withAmount(effect.amount),
      };
    }
    case "AdjustBalance":
      return {
        ...base,
        form: "AdjustBalance",
        toAccountId: effect.account_id,
        ...withAmount(effect.amount),
      };
    case "ApplyRmd":
      return {
        ...base,
        form: "ApplyRmd",
        toAccountId: effect.to_account_id,
        lotMethod: effect.lot_method,
      };
    case "RsuVesting":
      return {
        ...base,
        form: "RsuVesting",
        toAccountId: effect.to_account_id,
        assetId: effect.asset_id,
        units: effect.units,
        sellToCover: effect.sell_to_cover,
        lotMethod: effect.lot_method,
      };
    case "DeleteAccount":
      return { ...base, form: "DeleteAccount", toAccountId: effect.account_id };
    case "TriggerEvent":
    case "PauseEvent":
    case "ResumeEvent":
    case "TerminateEvent":
      return {
        ...base,
        form: "Event control",
        verb: effect.kind,
        targetEventId: effect.target_event_id,
      };
    case "Random":
      return { ...base, raw: effect };
  }
}
