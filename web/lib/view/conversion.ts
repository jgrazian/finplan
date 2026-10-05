/**
 * Roth conversions on the Plan tab: the accounts a conversion can name, the
 * defaults "Add Roth conversions" offers, the event it writes (the plan
 * crate's `RothConversions` template, built here against the plan the screen
 * already holds), and the hint for a `bracket_room` amount that fires before
 * December.
 */
import type { Account, AmountSpec, EffectSpec, EventBody, NamedParameter, TriggerSpec } from "@/lib/api/types";

/**
 * Month and day the conversions fire. Not Dec 31: the engine captures
 * year-end balances (the next RMD's base and the year-end snapshot) as Dec 31
 * begins, before that day's events, so a Dec 31 conversion would stay in next
 * year's RMD base. Mirrors `templates::CONVERSION_DAY`.
 */
export const CONVERSION_DAY = "12-30";

/** RMDs begin at 73 (born 1951–1959): conversions stop the year before. */
export const RMD_AGE = 73;

/** The brackets the dialog offers to fill to. */
export const CEILINGS = [0.1, 0.12, 0.22, 0.24, 0.32] as const;

type Investment = Extract<Account, { flavor: "Investment" }>;

/** What an account holds at cost: enough to rank, before any run has priced it. */
function held(account: Account): number {
  if (account.flavor !== "Investment" && account.flavor !== "Bank") return 0;
  return account.cash_value + account.positions.reduce((sum, p) => sum + p.cost_basis, 0);
}

const largest = (accounts: Account[]) =>
  [...accounts].sort((a, b) => held(b) - held(a) || a.id - b.id)[0];

/** The accounts each slot of a conversion may name. */
export function conversionAccounts(accounts: Account[]) {
  const investment = (status: Investment["tax_status"]) =>
    accounts.filter((a): a is Investment => a.flavor === "Investment" && a.tax_status === status);
  return {
    from: investment("TaxDeferred"),
    to: investment("TaxFree"),
    payers: accounts.filter((a) => a.flavor === "Bank" || (a.flavor === "Investment" && a.tax_status === "Taxable")),
  };
}

/** Why the plan cannot take conversions yet, or null when it can. */
export function conversionUnavailable(accounts: Account[]): string | null {
  const { from, to } = conversionAccounts(accounts);
  if (from.length === 0) return "Roth conversions need a tax-deferred account (a 401(k) or traditional IRA) to convert from.";
  if (to.length === 0) return "Roth conversions need a Roth (tax-free) account to convert into.";
  return null;
}

export interface ConversionChoice {
  name: string;
  fromAccountId: number;
  toAccountId: number;
  /** Fraction: 0.22 fills to the top of the 22% bracket. */
  ceilingRate: number;
  /** First year converted; the conversion is on Dec 30 of it. */
  startYear: number;
  /** Age conversions stop at (exclusive): the last is the year before. */
  untilAge: number;
  /** Bank or taxable account paying the tax; null withholds it. */
  payTaxFromAccountId: number | null;
}

/**
 * What the dialog opens with: the largest pre-tax account into the largest
 * Roth, filling the 22% bracket, tax paid from the largest taxable account
 * (else the largest bank), from the year of the plan's retirement-age
 * parameter (else its first year) until RMDs begin.
 */
export function conversionDefaults({
  accounts,
  parameters,
  start,
  birthDate,
}: {
  accounts: Account[];
  parameters: NamedParameter[];
  /** The plan's start date, YYYY-MM-DD. */
  start: string;
  /** YYYY-MM-DD, or empty. */
  birthDate: string;
}): ConversionChoice | null {
  const { from, to, payers } = conversionAccounts(accounts);
  if (from.length === 0 || to.length === 0) return null;
  const taxable = payers.filter((a) => a.flavor === "Investment");
  const payer = largest(taxable) ?? largest(payers.filter((a) => a.flavor === "Bank"));
  const startYear = Number(start.slice(0, 4));
  const birthYear = Number(birthDate.slice(0, 4));
  const retirement = parameters.find(
    (p) => p.value.kind === "Age" && p.name.toLowerCase().includes("retire"),
  );
  // An age's date is the birthday plus its months; only the year matters here.
  const retires =
    retirement?.value.kind === "Age" && birthYear
      ? birthYear + retirement.value.years +
        Math.floor((Number(birthDate.slice(5, 7)) - 1 + retirement.value.months) / 12)
      : null;
  return {
    name: "Roth conversions",
    fromAccountId: largest(from).id,
    toAccountId: largest(to).id,
    ceilingRate: 0.22,
    startYear: Math.max(startYear, retires ?? startYear),
    untilAge: RMD_AGE,
    payTaxFromAccountId: payer?.id ?? null,
  };
}

/** `0.22` → `22`, `0.125` → `12.5`: the figure `bracket_room(…%)` is written with. */
export function percentFigure(rate: number): string {
  return String(Math.round(rate * 100 * 1e6) / 1e6);
}

/**
 * The event: yearly on Dec 30 from `startYear`, ending at `untilAge`, with no
 * sort order so it lands last and runs after the day's other events.
 */
export function conversionEvent(choice: ConversionChoice): EventBody {
  const rate = percentFigure(choice.ceilingRate);
  return {
    name: choice.name,
    description:
      `Each Dec 30, convert pre-tax money to the Roth up to the top of the ${rate}% bracket. ` +
      "Not modeled: IRMAA, ACA credits, the taxable share of Social Security, gains stacking on ordinary income.",
    fires_once: false,
    enabled: true,
    sort_order: null,
    trigger: {
      kind: "Repeating",
      interval: "Yearly",
      start_condition: { kind: "Date", on_date: `${choice.startYear}-${CONVERSION_DAY}` },
      end_condition: { kind: "Age", years: choice.untilAge, months: null },
      max_occurrences: null,
    },
    effects: [
      {
        kind: "RothConversion",
        from_account_id: choice.fromAccountId,
        to_account_id: choice.toAccountId,
        amount: { kind: "Expression", source: `bracket_room(${rate}%)` },
        pay_tax_from_account_id: choice.payTaxFromAccountId,
      },
    ],
  };
}

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August",
  "September", "October", "November", "December"];

function usesBracketRoom(amount: AmountSpec | undefined): boolean {
  return amount?.kind === "Expression" && /\bbracket_room\s*\(/.test(amount.source);
}

function effectUsesBracketRoom(effect: EffectSpec): boolean {
  switch (effect.kind) {
    case "Random":
      return effectUsesBracketRoom(effect.on_true) || (effect.on_false ? effectUsesBracketRoom(effect.on_false) : false);
    case "BuyProperty":
      return usesBracketRoom(effect.price);
    default:
      return "amount" in effect && usesBracketRoom(effect.amount);
  }
}

/** The month a trigger fires in, when that can be read off it: 1–12, or null. */
function firingMonth(trigger: TriggerSpec, birthDate?: string): number | null {
  switch (trigger.kind) {
    case "Date":
      return Number(trigger.on_date.slice(5, 7)) || null;
    case "Age":
      return birthDate ? ((Number(birthDate.slice(5, 7)) - 1 + (trigger.months ?? 0)) % 12) + 1 : null;
    case "Repeating":
      // Anything more often than yearly fires before December somewhere.
      if (trigger.interval !== "Yearly" && trigger.interval !== "Never") return 1;
      return trigger.start_condition ? firingMonth(trigger.start_condition, birthDate) : null;
    default:
      return null;
  }
}

/**
 * The hint for an event whose amount uses `bracket_room` and fires before
 * December: the room is measured against the income so far, so income still
 * to come that year can push the year past the ceiling. Null otherwise, or
 * when the firing month cannot be read off the trigger.
 */
export function bracketRoomTimingHint(
  trigger: TriggerSpec,
  effects: EffectSpec[],
  birthDate?: string,
): string | null {
  if (!effects.some(effectUsesBracketRoom)) return null;
  const month = firingMonth(trigger, birthDate);
  if (month == null || month === 12) return null;
  const when = trigger.kind === "Repeating" && trigger.interval !== "Yearly" && trigger.interval !== "Never"
    ? `every ${trigger.interval === "Monthly" ? "month" : trigger.interval.toLowerCase().replace("ly", "")}`
    : `in ${MONTHS[month - 1]}`;
  return `bracket_room fills against this year's ordinary income so far, and this fires ${when}: income that lands later in the year can push the year past the bracket. Year-end conversions fire on Dec 30, last in the event list.`;
}
