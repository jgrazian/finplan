/**
 * The effect shapes the event form offers, and their translation to and from
 * `EffectSpec`.
 *
 * Covered: every effect the engine has except `Random`, which branches into two
 * more effects and would make this a tree editor rather than a list one.
 *
 * Seventeen engine kinds in fourteen forms: the four event-control effects differ
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
import { amountSource, readStaticAmount, rootAmountMode, staticAmount, type AmountNames } from "./amountDraft.ts";

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
  "BuyProperty",
  "SellProperty",
  "MarketShock",
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
  BuyProperty: { label: "real estate", tone: "outline" },
  SellProperty: { label: "real estate", tone: "outline" },
  MarketShock: { label: "market", tone: "neutral" },
  "Event control": { label: "event control", tone: "neutral" },
};

export const STRATEGIES: WithdrawalStrategy[] = [
  "BracketFilling",
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
  /** Kept while the static control is shown, so switching back restores the formula. */
  expressionDraft?: string;
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
  /** Highest marginal bracket rate, expressed as a percentage in the form. */
  bracketCeiling: number;
  /** A sweep's sources where they are not a plain strategy — held verbatim. */
  rawSources?: WithdrawalSourcesSpec;
  /** Which lots a sale reaches for. */
  lotMethod: LotMethod;
  taxFree: boolean;
  /** `BuyProperty` / `SellProperty`: the Property account. */
  propertyAccountId: number;
  /** `BuyProperty`: whether a loan covers part of the price. */
  financed: boolean;
  /** `BuyProperty`: the Liability drawn; `SellProperty`: the one paid off. */
  loanAccountId: number;
  /** `BuyProperty`: cash put down — a figure unless it came as an expression. */
  downPayment: number;
  rawDownPayment?: AmountSpec;
  termMonths: number;
  /** `SellProperty`: whether the sale pays a loan off. */
  payoff: boolean;
  /** `SellProperty`: fees and closing costs, as a share of the price (0–1). */
  sellingCostRate: number;
  /** `SellProperty`: gain excluded from tax. */
  gainExclusion: number;
  /** `MarketShock`: the one-time fall in market asset prices, as a fraction (0–1). */
  drop: number;
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
    strategy: "BracketFilling",
    bracketCeiling: 12,
    lotMethod: "Fifo",
    taxFree: false,
    // No account is a sensible default for these three: a property or a loan
    // picked for you is one you did not notice being picked.
    propertyAccountId: 0,
    financed: true,
    loanAccountId: 0,
    downPayment: 0,
    termMonths: 360,
    payoff: true,
    sellingCostRate: 0.06,
    gainExclusion: 250_000,
    drop: 0.3,
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
  /** Names a Property account. */
  property: boolean;
  /** A purchase's loan: down payment, loan account, term. */
  financing: boolean;
  /** A sale's costs, exclusion and payoff. */
  sale: boolean;
  /** A market shock's drop. */
  shock: boolean;
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
  property: false,
  financing: false,
  sale: false,
  shock: false,
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
    case "BuyProperty":
      return { ...NOTHING, from: true, property: true, amount: true, financing: true };
    case "SellProperty":
      return { ...NOTHING, to: true, property: true, sale: true };
    case "MarketShock":
      return { ...NOTHING, shock: true };
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
  property: "the property",
  financing: "the financing",
  sale: "the sale terms",
  shock: "the market drop",
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
  if (fields.amount && draft.rawAmount?.kind === "Expression" && !draft.rawAmount.source.trim())
    return say("needs a value expression");
  if (fields.property && draft.propertyAccountId === 0) return say("needs a property account");
  if (fields.financing && draft.financed && draft.loanAccountId === 0) return say("needs a loan account");
  if (fields.financing && draft.financed && draft.termMonths < 1) return say("needs a term");
  if (
    fields.financing && draft.financed &&
    draft.rawDownPayment?.kind === "Expression" && !draft.rawDownPayment.source.trim()
  ) return say("needs a down payment expression");
  if (fields.sale && draft.payoff && draft.loanAccountId === 0) return say("needs the loan it pays off");
  if (fields.shock && !(draft.drop > 0 && draft.drop < 1)) return say("needs a drop between 0% and 100%");
  return null;
}

/* ── to the wire ────────────────────────────────────────────────────────── */

function amountSpec(draft: EffectDraft): AmountSpec {
  if (draft.rawAmount) return draft.rawAmount;
  return staticAmount(draft.amount, draft.inflationAdjusted);
}

/**
 * Into the expression editor, and back out of it.
 *
 * Expanding is lossless by construction — the figure and its inflation tick
 * are exactly a one- or two-node tree, so the editor opens on what the field
 * was already saying. Collapsing only keeps a figure where the expression
 * still is one. A computed expression remains in expression mode so switching
 * cannot silently replace it with the draft's earlier static figure.
 */
export function expandAmount(draft: EffectDraft, names?: AmountNames): Partial<EffectDraft> {
  const source = draft.expressionDraft ?? amountSource(amountSpec(draft), names ?? {
    account: (id) => `account ${id}`,
    asset: (id) => `asset ${id}`,
  });
  return { rawAmount: { kind: "Expression", source } };
}

export function collapseAmount(draft: EffectDraft): Partial<EffectDraft> {
  const read = draft.rawAmount ? readStaticAmount(draft.rawAmount) : null;
  if (!read) return {};
  return {
    rawAmount: undefined,
    expressionDraft: draft.rawAmount?.kind === "Expression" ? draft.rawAmount.source : undefined,
    amount: read.value,
    inflationAdjusted: read.inflationAdjusted,
  };
}

export function updateExpression(draft: EffectDraft, source: string): Partial<EffectDraft> {
  const mode = shape(draft.form).mode ? rootAmountMode(source) : null;
  return {
    rawAmount: { kind: "Expression", source },
    expressionDraft: source,
    ...(mode ? { amountMode: mode } : {}),
  };
}

function sourcesSpec(draft: EffectDraft): WithdrawalSourcesSpec {
  return (
    draft.rawSources ?? {
      mode: "Strategy",
      strategy: draft.strategy,
      exclude_accounts: [],
      ...(draft.strategy === "BracketFilling" ? { bracket_ceiling: draft.bracketCeiling / 100 } : {}),
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
    case "BuyProperty":
      return {
        kind: "BuyProperty",
        property_account_id: draft.propertyAccountId,
        from_account_id: draft.fromAccountId,
        price: amount,
        financing: draft.financed
          ? {
            loan_account_id: draft.loanAccountId,
            down_payment: draft.rawDownPayment ?? staticAmount(draft.downPayment, false),
            term_months: draft.termMonths,
          }
          : null,
      };
    case "SellProperty":
      return {
        kind: "SellProperty",
        property_account_id: draft.propertyAccountId,
        to_account_id: draft.toAccountId,
        selling_cost_rate: draft.sellingCostRate,
        gain_exclusion: draft.gainExclusion,
        payoff_account_id: draft.payoff ? draft.loanAccountId : null,
      };
    case "MarketShock":
      return { kind: "MarketShock", drop: draft.drop };
    default:
      return { kind: draft.verb, target_event_id: draft.targetEventId };
  }
}

/* ── from the wire ──────────────────────────────────────────────────────── */

/** A `Fixed`, alone or wrapped in one `InflationAdjusted` — what the form asks. */
/** A plain strategy, or nothing — a named account or a custom list is neither. */
function readStrategy(sources: WithdrawalSourcesSpec | null): WithdrawalStrategy | null {
  return sources?.mode === "Strategy" && sources.exclude_accounts.length === 0 &&
    (sources.strategy === "BracketFilling" || sources.bracket_ceiling == null)
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
  names?: AmountNames,
): EffectDraft {
  const base = emptyEffect(accountId, assetId);

  const withAmount = (amount: AmountSpec): Partial<EffectDraft> => {
    const read = readStaticAmount(amount);
    const annotatedMode = amount.kind === "Expression" ? rootAmountMode(amount.source) : null;
    const fields: Partial<EffectDraft> = read
      ? { amount: read.value, inflationAdjusted: read.inflationAdjusted }
      : {
        rawAmount: {
          kind: "Expression", source: amountSource(amount, names ?? {
            account: (id) => `account ${id}`,
            asset: (id) => `asset ${id}`,
          })
        }
      };
    return { ...fields, ...(annotatedMode ? { amountMode: annotatedMode } : {}) };
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
        // A null source list compiles to TaxEfficientEarly, even though new
        // drafts default to BracketFilling. Preserve that saved behavior.
        // Sources the picker cannot draw (such as account exclusions) are
        // held verbatim rather than flattened to the strategy alone.
        ...(strategy
          ? {
            strategy,
            bracketCeiling: effect.sources?.mode === "Strategy"
              ? (effect.sources.bracket_ceiling ?? 0.12) * 100
              : base.bracketCeiling,
          }
          : effect.sources
            ? { rawSources: effect.sources }
            : { strategy: "TaxEfficientEarly" }),
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
    case "BuyProperty": {
      const down = effect.financing ? readStaticAmount(effect.financing.down_payment) : null;
      return {
        ...base,
        form: "BuyProperty",
        propertyAccountId: effect.property_account_id,
        fromAccountId: effect.from_account_id,
        financed: effect.financing != null,
        ...(effect.financing
          ? {
            loanAccountId: effect.financing.loan_account_id,
            termMonths: effect.financing.term_months,
            // A plain figure is edited as one; anything else is kept as it
            // came, so saving the event cannot flatten a formula.
            ...(down && !down.inflationAdjusted
              ? { downPayment: down.value }
              : {
                rawDownPayment: {
                  kind: "Expression" as const,
                  source: amountSource(effect.financing.down_payment, names ?? {
                    account: (id) => `account ${id}`,
                    asset: (id) => `asset ${id}`,
                  }),
                },
              }),
          }
          : {}),
        ...withAmount(effect.price),
      };
    }
    case "SellProperty":
      return {
        ...base,
        form: "SellProperty",
        propertyAccountId: effect.property_account_id,
        toAccountId: effect.to_account_id,
        sellingCostRate: effect.selling_cost_rate,
        gainExclusion: effect.gain_exclusion,
        payoff: effect.payoff_account_id != null,
        loanAccountId: effect.payoff_account_id ?? 0,
      };
    case "MarketShock":
      return { ...base, form: "MarketShock", drop: effect.drop };
    case "Random":
      return { ...base, raw: effect };
  }
}

/**
 * The down payment into an expression and back — the price's ƒ, for the
 * purchase's second amount. A figure opens as itself; an expression comes back
 * to a figure only while it still is one.
 */
export function expandDownPayment(draft: EffectDraft): Partial<EffectDraft> {
  return { rawDownPayment: { kind: "Expression", source: String(draft.downPayment) } };
}

export function collapseDownPayment(draft: EffectDraft): Partial<EffectDraft> | null {
  const read = draft.rawDownPayment ? readStaticAmount(draft.rawDownPayment) : null;
  if (!read || read.inflationAdjusted) return null;
  return { rawDownPayment: undefined, downPayment: read.value };
}

/**
 * Something the expression checker can read the down payment as. It only ever
 * looks at an effect's main amount, so this stands the down payment in as an
 * Expense from the paying account — the same account `source` means in it.
 */
export function downPaymentProbe(draft: EffectDraft): EffectSpec {
  return {
    kind: "Expense",
    from_account_id: draft.fromAccountId,
    amount: draft.rawDownPayment ?? staticAmount(draft.downPayment, false),
  };
}
