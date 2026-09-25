/**
 * `api::events::Event` → the Plan screen's `PlanEvent`.
 *
 * The API hands back the trigger and effect trees verbatim; the list and the
 * inspector want prose. Everything here is that translation, plus the timing
 * arithmetic the table needs: when an event first fires, how far it runs, and
 * where those land on the plan's axis.
 */
import type {
  AmountSpec,
  EffectSpec,
  Event as ApiEvent,
  Interval,
  Scenario,
  TriggerSpec,
  WithdrawalSourcesSpec,
} from "@/lib/api/types";
import type { EffectKind, EventEffect, PlanEvent } from "@/lib/types";
import type { PlanAxis } from "./axis";
import { addCalendarMonths, addYears, money, ratePercent } from "./format";

export interface EventNames {
  account: (id: number) => string;
  asset: (id: number) => string;
  event: (id: number) => string;
  parameter: (id: number) => string;
  parameterValue: (id: number) => { kind: string; value?: string; years?: number; months?: number } | undefined;
}

/**
 * The naming closure every description here takes.
 *
 * An id that names nothing still reads as something — a deleted account inside
 * an effect prints `account 12` rather than blank, so the row says what it is
 * pointing at even when the target is gone.
 */
export function namesOf(rows: {
  accounts: { id: number; name: string }[];
  assets: { id: number; name: string }[];
  events: { id: number; name: string }[];
  parameters?: { id: number; name: string; value?: { kind: string; value?: string | number; years?: number; months?: number } }[];
}): EventNames {
  const account = new Map(rows.accounts.map((a) => [a.id, a.name]));
  const asset = new Map(rows.assets.map((a) => [a.id, a.name]));
  const event = new Map(rows.events.map((e) => [e.id, e.name]));
  const parameter = new Map((rows.parameters ?? []).map((p) => [p.id, p.name]));
  const parameterValue = new Map((rows.parameters ?? []).map((p) => [p.id, p.value]));
  return {
    account: (id) => account.get(id) ?? `account ${id}`,
    asset: (id) => asset.get(id) ?? `asset ${id}`,
    event: (id) => event.get(id) ?? `event ${id}`,
    parameter: (id) => parameter.get(id) ?? `parameter ${id}`,
    parameterValue: (id) => {
      const value = parameterValue.get(id);
      if (!value) return undefined;
      if (value.kind === "Date" && typeof value.value === "string")
        return { kind: "Date", value: value.value };
      if (value.kind === "Age")
        return { kind: "Age", years: value.years, months: value.months };
      return { kind: value.kind };
    },
  };
}

export function toViewEvents(
  events: ApiEvent[],
  scenario: Scenario,
  axis: PlanAxis,
  names: EventNames,
  today = new Date().toISOString().slice(0, 10),
): PlanEvent[] {
  const clock = new PlanClock(events, scenario, names);

  return events.map((event) => {
    const first = clock.firstFire(event.id);
    const last = clock.lastFire(event.id);
    const [rangeStart, rangeEnd] = axis.range;

    return {
      id: event.name,
      serverId: event.id,
      trigger: summarizeTrigger(event.trigger, names),
      triggerKind: event.trigger.kind,
      triggerDetail: detailTrigger(event.trigger, names),
      next: nextFireLabel(event.trigger, first, today),
      // A condition-driven trigger has no knowable date, so it draws across the
      // whole horizon rather than pretending to a point.
      span: [
        first ? clamp(axis.at(first), rangeStart, rangeEnd) : rangeStart,
        last ? clamp(axis.at(last), rangeStart, rangeEnd) : rangeEnd,
      ],
      amount: summarizeAmount(event, names),
      effects: event.effects.map((effect) => describeEffect(effect, names)),
      firesOnce: event.fires_once,
      enabled: event.enabled,
    };
  });
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high);
}

// ── timing ──────────────────────────────────────────────────────────────────

/**
 * Resolves when each event starts and stops.
 *
 * `RelativeToEvent` makes this a graph walk rather than a per-event
 * calculation, and the server does not forbid a cycle between two events that
 * each anchor to the other, so visited ids are tracked.
 */
class PlanClock {
  private readonly byId: Map<number, ApiEvent>;
  private readonly firstCache = new Map<number, string | null>();

  constructor(
    events: ApiEvent[],
    private readonly scenario: Scenario,
    private readonly names: EventNames,
  ) {
    this.byId = new Map(events.map((e) => [e.id, e]));
  }

  firstFire(eventId: number): string | null {
    const cached = this.firstCache.get(eventId);
    if (cached !== undefined) return cached;
    // Seed the cache before recursing so a cycle resolves to "unknown" rather
    // than overflowing the stack.
    this.firstCache.set(eventId, null);
    const event = this.byId.get(eventId);
    const date = event ? this.start(event.trigger, 0) : null;
    this.firstCache.set(eventId, date);
    return date;
  }

  lastFire(eventId: number): string | null {
    const event = this.byId.get(eventId);
    if (!event) return null;
    const trigger = event.trigger;
    if (trigger.kind === "Repeating") {
      const end = trigger.end_condition ? this.start(trigger.end_condition, 0) : null;
      return end ?? this.horizonEnd();
    }
    // Everything else is a point in time, whether or not it repeats on its own.
    return this.firstFire(eventId);
  }

  private horizonEnd(): string {
    return addYears(this.scenario.start_date, this.scenario.duration_years);
  }

  private start(trigger: TriggerSpec, depth: number): string | null {
    if (depth > 16) return null;

    switch (trigger.kind) {
      case "Date":
        return trigger.on_date;
      case "DateParameter":
        {
          const value = this.names.parameterValue(trigger.parameter_id);
          return value?.kind === "Date" ? value.value ?? null : null;
        }
      case "AgeParameter": {
        const value = this.names.parameterValue(trigger.parameter_id);
        return value?.kind === "Age" && this.scenario.birth_date
          ? addCalendarMonths(this.scenario.birth_date, (value.years ?? 0) * 12 + (value.months ?? 0))
          : null;
      }
      case "Age":
        return this.scenario.birth_date
          ? addCalendarMonths(this.scenario.birth_date, trigger.years * 12 + (trigger.months ?? 0))
          : null;
      case "RelativeToEvent": {
        const anchor = this.firstFire(trigger.event_id);
        return anchor ? shift(anchor, trigger.unit, trigger.value) : null;
      }
      case "Repeating":
        return trigger.start_condition
          ? (this.start(trigger.start_condition, depth + 1) ?? this.scenario.start_date)
          : this.scenario.start_date;
      case "And": {
        // Every condition must hold, so the earliest the group can fire is the
        // latest of its parts.
        const dates = trigger.children
          .map((child) => this.start(child, depth + 1))
          .filter((d): d is string => d != null);
        return dates.length ? dates.reduce((a, b) => (a > b ? a : b)) : null;
      }
      case "Or": {
        const dates = trigger.children
          .map((child) => this.start(child, depth + 1))
          .filter((d): d is string => d != null);
        return dates.length ? dates.reduce((a, b) => (a < b ? a : b)) : null;
      }
      case "AccountBalance":
      case "AssetBalance":
      case "NetWorth":
      case "Manual":
        return null;
    }
  }
}

/** ISO date shifted by a `RelativeToEvent` offset. */
function shift(isoDate: string, unit: "Days" | "Months" | "Years", value: number): string {
  if (unit === "Years") return addYears(isoDate, value);
  const [y, m, d] = isoDate.split("-").map(Number);
  const days = unit === "Days" ? value : 0;
  const months = unit === "Months" ? value : 0;
  // UTC so a local timezone west of Greenwich cannot roll the date back a day.
  const at = new Date(Date.UTC(y, m - 1 + months, d + days));
  return at.toISOString().slice(0, 10);
}

/** Calendar step of each interval: `[months, days]`, one of which is zero. */
const INTERVAL_STEP: Record<Interval, [number, number]> = {
  Never: [0, 0],
  Weekly: [0, 7],
  BiWeekly: [0, 14],
  Monthly: [1, 0],
  Quarterly: [3, 0],
  Yearly: [12, 0],
};

/**
 * The date the inspector labels "next fire". A repeating event that started in
 * the past is stepped forward to the first occurrence still ahead.
 */
/**
 * The `next` of an event whose trigger has no knowable date — a balance
 * crossing, or a Manual one. The timeline reads it back, so it is a constant
 * rather than a string written twice.
 */
export const ON_CONDITION = "on condition";

function nextFireLabel(
  trigger: TriggerSpec,
  first: string | null,
  today: string,
): string {
  if (!first) return ON_CONDITION;
  if (trigger.kind !== "Repeating" || first >= today) return first;

  const [months, days] = INTERVAL_STEP[trigger.interval];
  if (months === 0 && days === 0) return first;

  const [y, m, d] = first.split("-").map(Number);
  let at = Date.UTC(y, m - 1, d);
  const now = Date.parse(`${today}T00:00:00Z`);

  if (days > 0) {
    const step = days * 86_400_000;
    at += Math.ceil((now - at) / step) * step;
  } else {
    // Months have no constant length, so advance a period at a time. The
    // horizon is decades and the period at worst monthly, so this is a few
    // hundred iterations even for a plan that started long ago.
    let periods = 0;
    while (at < now && periods < 2000) {
      periods += 1;
      at = Date.UTC(y, m - 1 + months * periods, d);
    }
  }
  return new Date(at).toISOString().slice(0, 10);
}

// ── triggers ────────────────────────────────────────────────────────────────

const COMPARISON: Record<"GreaterThanOrEqual" | "LessThanOrEqual", string> = {
  GreaterThanOrEqual: "≥",
  LessThanOrEqual: "≤",
};

function summarizeTrigger(trigger: TriggerSpec, names: EventNames): string {
  switch (trigger.kind) {
    case "Date":
      return `Date · ${trigger.on_date}`;
    case "DateParameter":
      return `Date · ${names.parameter(trigger.parameter_id)}`;
    case "Age":
      return `Age ${trigger.years}`;
    case "AgeParameter":
      return `Age · ${names.parameter(trigger.parameter_id)}`;
    case "Repeating":
      return `Repeating · ${trigger.interval.toLowerCase()}`;
    case "RelativeToEvent":
      return `Relative to ${names.event(trigger.event_id)}`;
    case "AccountBalance":
      return `${names.account(trigger.account_id)} ${COMPARISON[trigger.comparison]} ${money(trigger.threshold)}`;
    case "AssetBalance":
      return `${names.asset(trigger.asset_id)} ${COMPARISON[trigger.comparison]} ${money(trigger.threshold)}`;
    case "NetWorth":
      return `Net worth ${COMPARISON[trigger.comparison]} ${money(trigger.threshold)}`;
    case "And":
      return `All of ${trigger.children.length}`;
    case "Or":
      return `Any of ${trigger.children.length}`;
    case "Manual":
      return "Manual";
  }
}

/** A trigger as it reads inside a sentence — "age 62", not "Age 62". */
function conditionLabel(trigger: TriggerSpec, names: EventNames): string {
  switch (trigger.kind) {
    case "Date":
      return trigger.on_date;
    case "DateParameter":
      return names.parameter(trigger.parameter_id);
    case "Age":
      return `age ${trigger.years}`;
    case "AgeParameter":
      return `age ${names.parameter(trigger.parameter_id)}`;
    default:
      return summarizeTrigger(trigger, names);
  }
}

export function detailTrigger(trigger: TriggerSpec, names: EventNames): string {
  switch (trigger.kind) {
    case "DateParameter":
    case "AgeParameter":
      return names.parameter(trigger.parameter_id);
    case "Age":
      return trigger.months == null
        ? `years ${trigger.years}`
        : `years ${trigger.years} · months ${trigger.months}`;
    case "RelativeToEvent":
      return `${names.event(trigger.event_id)} ${trigger.value >= 0 ? "+" : ""}${trigger.value} ${trigger.unit.toLowerCase()}`;
    case "Repeating": {
      const parts = [trigger.interval.toLowerCase()];
      parts.push(
        trigger.start_condition
          ? `from ${conditionLabel(trigger.start_condition, names)}`
          : "from plan start",
      );
      parts.push(
        trigger.end_condition
          ? `until ${conditionLabel(trigger.end_condition, names)}`
          : "no end condition",
      );
      if (trigger.max_occurrences != null) parts.push(`max ${trigger.max_occurrences}×`);
      return parts.join(" · ");
    }
    case "AssetBalance":
      return `${names.account(trigger.account_id)} / ${names.asset(trigger.asset_id)} ${COMPARISON[trigger.comparison]} ${money(trigger.threshold)}`;
    case "And":
    case "Or":
      return trigger.children.map((c) => summarizeTrigger(c, names)).join(
        trigger.kind === "And" ? " and " : " or ",
      );
    default:
      return summarizeTrigger(trigger, names);
  }
}

// ── amounts ─────────────────────────────────────────────────────────────────

const PER_INTERVAL: Record<Interval, string> = {
  Never: "",
  Weekly: " / wk",
  BiWeekly: " / 2wk",
  Monthly: " / mo",
  Quarterly: " / qtr",
  Yearly: " / yr",
};

/** The list's Amount column: the first effect that moves money, per period. */
/**
 * The `amount` of an event whose effects name no figure — a marker, a pause,
 * an RMD the engine sizes itself. The rail reads it back to know it has
 * nothing to print, so it is a constant rather than a dash written twice.
 */
export const NO_AMOUNT = "—";

function summarizeAmount(event: ApiEvent, names: EventNames): string {
  const amount = event.effects.map((e) => amountOf(e)).find((a) => a != null);
  if (!amount) return NO_AMOUNT;
  const per = event.trigger.kind === "Repeating" ? PER_INTERVAL[event.trigger.interval] : "";
  return describeAmount(amount, names) + per;
}

function amountOf(effect: EffectSpec): AmountSpec | null {
  switch (effect.kind) {
    case "Income":
    case "Expense":
    case "AssetPurchase":
    case "AssetSale":
    case "Sweep":
    case "AdjustBalance":
    case "CashTransfer":
      return effect.amount;
    case "BuyProperty":
      return effect.price;
    case "Random":
      return amountOf(effect.on_true);
    default:
      return null;
  }
}

export function describeAmount(amount: AmountSpec, names: EventNames): string {
  const rec = (a: AmountSpec) => describeAmount(a, names);
  switch (amount.kind) {
    case "Expression":
      if (/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)$/.test(amount.source.trim())) {
        return money(Number(amount.source.trim()));
      }
      {
        const inflated = /^inflation\(\s*([+-]?(?:\d+(?:\.\d*)?|\.\d+))\s*\)$/.exec(amount.source.trim());
        if (inflated) return `${money(Number(inflated[1]))} infl-adj`;
      }
      return amount.source;
    case "Fixed":
      return money(amount.value);
    case "InflationAdjusted":
      return `${rec(amount.inner)} infl-adj`;
    case "Scale":
      // Held as a multiplier and said as a percentage, the way the editor asks
      // for it — `0.04` is the 4% withdrawal rate anyone would say out loud.
      return `${Number(ratePercent(amount.factor).toFixed(4))}% of ${rec(amount.inner)}`;
    case "SourceBalance":
      return "source balance";
    case "ZeroTargetBalance":
      return "to zero";
    case "TargetToBalance":
      return `top up to ${money(amount.value)}`;
    case "AssetBalance":
      return `${names.asset(amount.asset_id)} in ${names.account(amount.account_id)}`;
    case "AccountTotalBalance":
      return `${names.account(amount.account_id)} balance`;
    case "AccountCashBalance":
      return `${names.account(amount.account_id)} cash`;
    case "Min":
      return `min(${rec(amount.left)}, ${rec(amount.right)})`;
    case "Max":
      return `max(${rec(amount.left)}, ${rec(amount.right)})`;
    case "Sub":
      return `${rec(amount.left)} − ${rec(amount.right)}`;
    case "Add":
      return `${rec(amount.left)} + ${rec(amount.right)}`;
    case "Mul":
      return `${rec(amount.left)} × ${rec(amount.right)}`;
  }
}

// ── effects ─────────────────────────────────────────────────────────────────

function describeSources(sources: WithdrawalSourcesSpec, names: EventNames): string {
  switch (sources.mode) {
    case "SingleAsset":
      return `${names.asset(sources.asset_id)} in ${names.account(sources.account_id)}`;
    case "SingleAccount":
      return names.account(sources.account_id);
    case "Strategy":
      return sources.exclude_accounts.length === 0
        ? sources.strategy
        : `${sources.strategy} (excl. ${sources.exclude_accounts.map(names.account).join(", ")})`;
    case "Custom":
      return sources.entries
        .map((e) => `${names.asset(e.asset_id)} in ${names.account(e.account_id)}`)
        .join(" → ");
  }
}

export function describeEffect(effect: EffectSpec, names: EventNames): EventEffect {
  const kind: EffectKind = effect.kind;
  const amount = (a: AmountSpec) => describeAmount(a, names);

  switch (effect.kind) {
    case "Income":
      return {
        kind,
        detail: `→ ${names.account(effect.to_account_id)} · ${effect.income_type} · ${amount(effect.amount)} ${effect.amount_mode.toLowerCase()}`,
      };
    case "Expense":
      return {
        kind,
        detail: `from ${names.account(effect.from_account_id)} · ${amount(effect.amount)}`,
      };
    case "CashTransfer":
      return {
        kind,
        detail: `${names.account(effect.from_account_id)} → ${names.account(effect.to_account_id)} · ${amount(effect.amount)}`,
      };
    case "AssetPurchase":
      return {
        kind,
        detail: `${names.account(effect.from_account_id)} → ${names.account(effect.to_account_id)} / ${names.asset(effect.asset_id)} · ${amount(effect.amount)}`,
      };
    case "AssetSale":
      return {
        kind,
        detail: `${names.account(effect.from_account_id)} / ${effect.asset_id == null ? "any asset" : names.asset(effect.asset_id)} · ${amount(effect.amount)} · ${effect.lot_method}`,
      };
    case "Sweep":
      return {
        kind,
        detail: `${effect.sources ? describeSources(effect.sources, names) : "default sources"} → ${names.account(effect.to_account_id)} · ${amount(effect.amount)} · ${effect.lot_method}`,
      };
    case "AdjustBalance":
      return {
        kind,
        detail: `${names.account(effect.account_id)} · ${amount(effect.amount)}`,
      };
    case "ApplyRmd":
      return {
        kind,
        detail: `destination ${names.account(effect.to_account_id)} · ${effect.lot_method}`,
      };
    case "RsuVesting":
      return {
        kind,
        detail: `${effect.units} × ${names.asset(effect.asset_id)} → ${names.account(effect.to_account_id)}${effect.sell_to_cover ? " · sell to cover" : ""}`,
      };
    case "DeleteAccount":
      return { kind, detail: names.account(effect.account_id) };
    case "TriggerEvent":
    case "PauseEvent":
    case "ResumeEvent":
    case "TerminateEvent":
      return { kind, detail: names.event(effect.target_event_id) };
    case "Random":
      return {
        kind,
        detail: `p=${effect.probability} → ${effect.on_true.kind}${effect.on_false ? ` else ${effect.on_false.kind}` : ""}`,
      };
    case "BuyProperty": {
      const f = effect.financing;
      return {
        kind,
        detail: `${names.account(effect.property_account_id)} · ${amount(effect.price)}${
          f
            ? ` · ${amount(f.down_payment)} down from ${names.account(effect.from_account_id)}, ${names.account(f.loan_account_id)} over ${termLabel(f.term_months)}`
            : ` from ${names.account(effect.from_account_id)}`
        }`,
      };
    }
    case "SellProperty":
      return {
        kind,
        detail: `${names.account(effect.property_account_id)} → ${names.account(effect.to_account_id)} · ${Number((effect.selling_cost_rate * 100).toFixed(2))}% costs${
          effect.gain_exclusion > 0 ? ` · ${money(effect.gain_exclusion)} excluded` : ""
        }${effect.payoff_account_id != null ? ` · pays off ${names.account(effect.payoff_account_id)}` : ""}`,
      };
  }
}

/** `360` → "30 yr", `66` → "5 yr 6 mo". */
export function termLabel(months: number): string {
  const years = Math.floor(months / 12);
  const rest = months % 12;
  return [years ? `${years} yr` : "", rest ? `${rest} mo` : ""].filter(Boolean).join(" ") || "0 mo";
}
