/**
 * The transfer-amount language, as the form draws it.
 *
 * An amount is a small expression tree — `Min(AccountCashBalance, Scale(0.04,
 * Fixed))` is a legal withdrawal rule — and every node of it is one of the
 * fourteen kinds below. This module is the metadata that lets one recursive
 * component draw any of them: what each kind is called, what its operands are
 * called, and what changing a node's kind keeps.
 *
 * There is no draft type here. The tree the editor holds *is* an `AmountSpec`,
 * because there is nothing in it the form cannot express — unlike triggers and
 * effects, an amount needs no `raw` hold and loses nothing on a round trip.
 */
import type { DropdownOption } from "@/components/ui";
import type { AmountSpec } from "@/lib/api/types";

export type AmountKind = AmountSpec["kind"];

/**
 * The kinds, in the three families the picker bands them into.
 *
 * Fourteen options in one flat list is a wall; the split is by what the node
 * *is* — a value you type, a balance the plan already holds, or arithmetic
 * over two more amounts.
 */
const AMOUNT_GROUPS = [
  { label: "Values", kinds: ["Fixed", "InflationAdjusted", "TargetToBalance"] },
  {
    label: "Balances",
    kinds: [
      "AccountTotalBalance",
      "AccountCashBalance",
      "AssetBalance",
      "SourceBalance",
      "ZeroTargetBalance",
    ],
  },
  { label: "Arithmetic", kinds: ["Min", "Max", "Add", "Sub", "Mul", "Scale"] },
] satisfies { label: string; kinds: AmountKind[] }[];

/**
 * What each kind is called in the picker.
 *
 * The engine's own term for the node, not a paraphrase of it: someone reading
 * `Min(AccountCashBalance, Scale(0.04, Fixed))` in the spec should be able to
 * find each of those words in this menu. The prose that explains a term lives
 * in `AMOUNT_HINT`, under the node it belongs to.
 *
 * Exhaustive over the generated union, so a kind added to the server has to be
 * named here before this compiles.
 */
const AMOUNT_LABEL: Record<AmountKind, string> = {
  Fixed: "Static Value",
  InflationAdjusted: "Static Value (Inflation Adjusted)",
  TargetToBalance: "Target To Balance",
  SourceBalance: "Source Balance",
  ZeroTargetBalance: "Zero Target Balance",
  AssetBalance: "Asset Balance",
  AccountTotalBalance: "Account Balance",
  AccountCashBalance: "Account Cash Balance",
  Min: "Min",
  Max: "Max",
  Add: "Add",
  Sub: "Subtract",
  Mul: "Multiply",
  Scale: "Scale (Percentage)",
};

/** The kind picker's menu: every kind, under its family's band. */
export const AMOUNT_OPTIONS: DropdownOption<AmountKind>[] = AMOUNT_GROUPS.flatMap(
  (family) =>
    family.kinds.map((value) => ({ value, label: AMOUNT_LABEL[value], group: family.label })),
);

/**
 * The line under a node, for the kinds whose meaning is not in their name.
 *
 * The names above are the engine's, which makes them precise and not always
 * self-explanatory — this is where each one is spelled out. Source and target
 * are the effect's own accounts, not something the amount names, which is the
 * one thing about this language that surprises people.
 */
export const AMOUNT_HINT: Partial<Record<AmountKind, string>> = {
  InflationAdjusted:
    "The value below is in plan-start dollars, grown by inflation to the day this fires.",
  TargetToBalance:
    "Moves only the shortfall to bring the effect's own destination up to this balance — nothing if it is already there.",
  SourceBalance: "Everything the effect's own source account holds.",
  ZeroTargetBalance:
    "Enough to bring the effect's own destination to zero — for paying a debt off.",
  Scale: "Multiplies the operand below by a factor, typed as a percentage: 4% of a balance.",
};

/**
 * What this kind's operands are called, in the order they are drawn.
 *
 * The field names off the wire: a unary node stores its operand as `inner` and
 * a binary one as `left` and `right`, so `Subtract` is left − right and the
 * labels say which is which without a sentence about it.
 */
export function operandLabels(kind: AmountKind): string[] {
  return kind === "InflationAdjusted" || kind === "Scale"
    ? ["Inner"]
    : ["Left", "Right"];
}

/* ── walking the tree ───────────────────────────────────────────────────── */

export function operandsOf(amount: AmountSpec): AmountSpec[] {
  switch (amount.kind) {
    case "InflationAdjusted":
    case "Scale":
      return [amount.inner];
    case "Min":
    case "Max":
    case "Sub":
    case "Add":
    case "Mul":
      return [amount.left, amount.right];
    default:
      return [];
  }
}

/** The same node with one operand replaced. A leaf is returned untouched. */
export function withOperand(
  amount: AmountSpec,
  index: number,
  child: AmountSpec,
): AmountSpec {
  switch (amount.kind) {
    case "InflationAdjusted":
    case "Scale":
      return { ...amount, inner: child };
    case "Min":
    case "Max":
    case "Sub":
    case "Add":
    case "Mul":
      return index === 0 ? { ...amount, left: child } : { ...amount, right: child };
    default:
      return amount;
  }
}

/* ── changing a node's kind ─────────────────────────────────────────────── */

const ZERO: AmountSpec = { kind: "Fixed", value: 0 };

/** The dollar figure a node carries, where it carries one. */
function valueOf(amount: AmountSpec): number | null {
  return amount.kind === "Fixed" || amount.kind === "TargetToBalance" ? amount.value : null;
}

function accountOf(amount: AmountSpec): number | null {
  switch (amount.kind) {
    case "AssetBalance":
    case "AccountTotalBalance":
    case "AccountCashBalance":
      return amount.account_id;
    default:
      return null;
  }
}

/**
 * The same amount as a different kind, keeping everything the new kind can
 * still hold.
 *
 * The operand rule is what makes this a builder rather than a reset: a leaf
 * becoming an operator *becomes its own left operand*, so turning `$2,000`
 * into `Min` gives `min($2,000, $0)` and you fill in the other side. Nothing
 * else in the editor wraps a subtree, and nothing else needs to.
 */
export function withAmountKind(
  amount: AmountSpec,
  kind: AmountKind,
  fallback: { accountId: number; assetId: number },
): AmountSpec {
  if (amount.kind === kind) return amount;

  const value = valueOf(amount) ?? 0;
  const account_id = accountOf(amount) ?? fallback.accountId;
  const asset_id = amount.kind === "AssetBalance" ? amount.asset_id : fallback.assetId;
  const factor = amount.kind === "Scale" ? amount.factor : 1;

  // A leaf standing in for its own first operand is how wrapping happens.
  const kept = operandsOf(amount);
  const carried = kept.length > 0 ? kept : [amount];
  const operand = (index: number) => carried[index] ?? ZERO;

  switch (kind) {
    case "Fixed":
      return { kind, value };
    case "TargetToBalance":
      return { kind, value };
    case "SourceBalance":
    case "ZeroTargetBalance":
      return { kind };
    case "AccountTotalBalance":
    case "AccountCashBalance":
      return { kind, account_id };
    case "AssetBalance":
      return { kind, account_id, asset_id };
    case "InflationAdjusted":
      return { kind, inner: operand(0) };
    case "Scale":
      return { kind, factor, inner: operand(0) };
    default:
      return { kind, left: operand(0), right: operand(1) };
  }
}

/* ── validation ─────────────────────────────────────────────────────────── */

/**
 * Why this expression cannot be sent yet, or null when it can.
 *
 * Only unnamed references: an account or holding select left at `0` — which is
 * what a plan with no accounts, or a reference to a deleted one, leaves behind.
 */
export function amountProblem(amount: AmountSpec): string | null {
  switch (amount.kind) {
    case "AssetBalance":
      if (amount.asset_id === 0) return "names no holding";
      return amount.account_id === 0 ? "names no account" : null;
    case "AccountTotalBalance":
    case "AccountCashBalance":
      return amount.account_id === 0 ? "names no account" : null;
    default:
      for (const child of operandsOf(amount)) {
        const problem = amountProblem(child);
        if (problem) return problem;
      }
      return null;
  }
}
