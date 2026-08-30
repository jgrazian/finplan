/**
 * Which accounts an event touches.
 *
 * Triggers, transfer amounts and effects are all recursive, and an account can
 * be named at any depth — inside a `Random` branch, inside the right operand of
 * a `Min`, inside a sweep's exclusion list. The walk below is exhaustive over
 * the generated unions, so adding an `EffectSpec` variant that names an account
 * fails to compile here until it is handled.
 */
import type {
  AmountSpec,
  EffectSpec,
  Event as ApiEvent,
  TriggerSpec,
  WithdrawalSourcesSpec,
} from "@/lib/api/types";

export function accountRefs(event: ApiEvent): Set<number> {
  const found = new Set<number>();
  walkTrigger(event.trigger, found);
  for (const effect of event.effects) walkEffect(effect, found);
  return found;
}

function walkTrigger(trigger: TriggerSpec, out: Set<number>): void {
  switch (trigger.kind) {
    case "AccountBalance":
    case "AssetBalance":
      out.add(trigger.account_id);
      return;
    case "And":
    case "Or":
      for (const child of trigger.children) walkTrigger(child, out);
      return;
    case "Repeating":
      if (trigger.start_condition) walkTrigger(trigger.start_condition, out);
      if (trigger.end_condition) walkTrigger(trigger.end_condition, out);
      return;
    case "Date":
    case "Age":
    case "RelativeToEvent":
    case "NetWorth":
    case "Manual":
      return;
  }
}

function walkAmount(amount: AmountSpec, out: Set<number>): void {
  switch (amount.kind) {
    case "AssetBalance":
    case "AccountTotalBalance":
    case "AccountCashBalance":
      out.add(amount.account_id);
      return;
    case "InflationAdjusted":
    case "Scale":
      walkAmount(amount.inner, out);
      return;
    case "Min":
    case "Max":
    case "Sub":
    case "Add":
    case "Mul":
      walkAmount(amount.left, out);
      walkAmount(amount.right, out);
      return;
    case "Fixed":
    case "TargetToBalance":
    case "SourceBalance":
    case "ZeroTargetBalance":
      return;
  }
}

function walkSources(sources: WithdrawalSourcesSpec, out: Set<number>): void {
  switch (sources.mode) {
    case "SingleAsset":
    case "SingleAccount":
      out.add(sources.account_id);
      return;
    case "Strategy":
      // An exclusion is still a reference: deleting the account changes what
      // the sweep does.
      for (const id of sources.exclude_accounts) out.add(id);
      return;
    case "Custom":
      for (const entry of sources.entries) out.add(entry.account_id);
      return;
  }
}

function walkEffect(effect: EffectSpec, out: Set<number>): void {
  switch (effect.kind) {
    case "Income":
    case "ApplyRmd":
      out.add(effect.to_account_id);
      if (effect.kind === "Income") walkAmount(effect.amount, out);
      return;
    case "RsuVesting":
      out.add(effect.to_account_id);
      return;
    case "Expense":
    case "AssetSale":
      out.add(effect.from_account_id);
      walkAmount(effect.amount, out);
      return;
    case "AssetPurchase":
    case "CashTransfer":
      out.add(effect.from_account_id);
      out.add(effect.to_account_id);
      walkAmount(effect.amount, out);
      return;
    case "Sweep":
      out.add(effect.to_account_id);
      walkAmount(effect.amount, out);
      if (effect.sources) walkSources(effect.sources, out);
      return;
    case "AdjustBalance":
      out.add(effect.account_id);
      walkAmount(effect.amount, out);
      return;
    case "DeleteAccount":
      out.add(effect.account_id);
      return;
    case "Random":
      walkEffect(effect.on_true, out);
      if (effect.on_false) walkEffect(effect.on_false, out);
      return;
    case "TriggerEvent":
    case "PauseEvent":
    case "ResumeEvent":
    case "TerminateEvent":
      return;
  }
}
