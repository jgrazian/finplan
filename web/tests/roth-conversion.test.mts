import assert from "node:assert/strict";
import { test } from "node:test";
import { draftOfEffect, effectProblem, emptyEffect, shape, toEffectSpec } from "../components/plan/effectDraft.ts";
import {
  bracketRoomTimingHint,
  conversionDefaults,
  conversionEvent,
  conversionUnavailable,
  conversionWrites,
} from "../lib/view/conversion.ts";
import type { Account, EffectSpec, Event, NamedParameter, TriggerSpec } from "../lib/api/types.ts";

const investment = (id: number, tax_status: "Taxable" | "TaxDeferred" | "TaxFree", cash: number): Account => ({
  id,
  name: `Account ${id}`,
  description: null,
  sort_order: id,
  positions: [{ id: id * 10, asset_id: 1, purchase_date: "2020-01-01", units: 10, cost_basis: cash }],
  flavor: "Investment",
  tax_status,
  cash_value: 0,
  cash_return_profile_id: 1,
  contribution_limit: null,
  contribution_period: null,
  plan_type: null,
  catch_up: [],
});
const bank = (id: number, cash: number): Account => ({
  id,
  name: `Bank ${id}`,
  description: null,
  sort_order: id,
  positions: [],
  flavor: "Bank",
  cash_value: cash,
  return_profile_id: 1,
});
const accounts = [
  investment(1, "TaxDeferred", 50_000),
  investment(2, "TaxDeferred", 400_000),
  investment(3, "TaxFree", 90_000),
  investment(4, "Taxable", 250_000),
  bank(5, 30_000),
];
const retirement: NamedParameter = {
  id: 9,
  scenario_id: 1,
  name: "Retirement age",
  value: { kind: "Age", years: 55, months: 6 },
  uses: [],
};

test("a conversion effect round-trips through the editor's draft", () => {
  const spec: EffectSpec = {
    kind: "RothConversion",
    from_account_id: 2,
    to_account_id: 3,
    amount: { kind: "Expression", source: "bracket_room(0.22)" },
    pay_tax_from_account_id: 5,
  };
  const draft = draftOfEffect(spec, 1, 1);
  assert.equal(draft.form, "RothConversion");
  assert.equal(draft.payTaxFromAccountId, 5);
  assert.deepEqual(toEffectSpec(draft), spec);
  // Withheld: no payer.
  const withheld = toEffectSpec({ ...draft, payTaxFromAccountId: 0 });
  assert.equal(withheld.kind === "RothConversion" && withheld.pay_tax_from_account_id, null);
  assert.ok(shape("RothConversion").conversion && shape("RothConversion").amount);
  assert.match(effectProblem({ ...emptyEffect(2, 1), form: "RothConversion" }, 0) ?? "", /different/);
});

test("the dialog's defaults: largest pre-tax into largest Roth, 22%, from retirement, tax from taxable", () => {
  const choice = conversionDefaults({
    accounts,
    parameters: [retirement],
    start: "2026-10-04",
    birthDate: "1975-09-15",
  });
  assert.ok(choice);
  assert.equal(choice.fromAccountId, 2);
  assert.equal(choice.toAccountId, 3);
  assert.equal(choice.ceilingRate, 0.22);
  assert.equal(choice.payTaxFromAccountId, 4);
  // 55 years 6 months past September 1975 is March 2031.
  assert.equal(choice.startYear, 2031);
  // Born 1975: RMDs begin at 75.
  assert.equal(choice.untilAge, 75);

  // No retirement parameter: the plan's first year; no taxable account: a bank.
  const plain = conversionDefaults({
    accounts: accounts.filter((a) => a.id !== 4),
    parameters: [],
    start: "2026-10-04",
    birthDate: "1975-09-15",
  });
  assert.equal(plain?.startYear, 2026);
  assert.equal(plain?.payTaxFromAccountId, 5);

  assert.equal(conversionDefaults({ accounts: [bank(5, 1)], parameters: [], start: "2026-01-01", birthDate: "" }), null);
  assert.match(conversionUnavailable([bank(5, 1)]) ?? "", /tax-deferred/);
  assert.match(conversionUnavailable([investment(2, "TaxDeferred", 1)]) ?? "", /Roth/);
  assert.equal(conversionUnavailable(accounts), null);
});

test("the event is the template's: yearly on Dec 30 until 73, bracket_room, appended last", () => {
  const body = conversionEvent({
    name: "Roth conversions",
    fromAccountId: 2,
    toAccountId: 3,
    ceilingRate: 0.12,
    startYear: 2031,
    untilAge: 73,
    payTaxFromAccountId: null,
  });
  assert.equal(body.sort_order, null);
  assert.deepEqual(body.trigger, {
    kind: "Repeating",
    interval: "Yearly",
    start_condition: { kind: "Date", on_date: "2031-12-30" },
    end_condition: { kind: "Age", years: 73, months: null },
    max_occurrences: null,
  });
  assert.deepEqual(body.effects, [{
    kind: "RothConversion",
    from_account_id: 2,
    to_account_id: 3,
    amount: { kind: "Expression", source: "bracket_room(12%)" },
    pay_tax_from_account_id: null,
  }]);
  assert.match(body.description ?? "", /Not modeled: IRMAA/);
});

test("a bracket_room amount that fires before December gets a hint", () => {
  const conversion: EffectSpec = {
    kind: "RothConversion",
    from_account_id: 2,
    to_account_id: 3,
    amount: { kind: "Expression", source: "bracket_room(22%)" },
    pay_tax_from_account_id: null,
  };
  const repeating = (on_date: string, interval: "Yearly" | "Monthly"): TriggerSpec => ({
    kind: "Repeating",
    interval,
    start_condition: { kind: "Date", on_date },
    end_condition: null,
    max_occurrences: null,
  });
  const yearly = (on_date: string) => repeating(on_date, "Yearly");
  assert.equal(bracketRoomTimingHint(yearly("2030-12-30"), [conversion]), null);
  assert.match(bracketRoomTimingHint(yearly("2030-06-15"), [conversion]) ?? "", /in June/);
  assert.match(
    bracketRoomTimingHint(repeating("2030-12-30", "Monthly"), [conversion]) ?? "",
    /every month/,
  );
  // An Age trigger fires in the birthday's month.
  assert.match(bracketRoomTimingHint({ kind: "Age", years: 60, months: null }, [conversion], "1970-03-02") ?? "", /in March/);
  // A sweep's amount counts too; a fixed amount does not.
  const sweep: EffectSpec = {
    kind: "Sweep",
    to_account_id: 5,
    amount: { kind: "Expression", source: "min(bracket_room(0.12), 50000)" },
    sources: null,
    amount_mode: "Gross",
    lot_method: "Fifo",
    income_type: "Taxable",
  };
  assert.ok(bracketRoomTimingHint(yearly("2030-03-01"), [sweep]));
  assert.equal(
    bracketRoomTimingHint(yearly("2030-03-01"), [{ ...conversion, amount: { kind: "Fixed", value: 1 } }]),
    null,
  );
  // Unknown month: no hint rather than a guess.
  assert.equal(bracketRoomTimingHint({ kind: "Manual" }, [conversion]), null);
});

test("Drawdown's Apply retargets the plan's conversions, adds the overlay's, or switches them off", () => {
  const overlay = {
    from_account_id: 3,
    to_account_id: 4,
    pay_tax_from_account_id: 1,
    start_year: 2036,
    until_age: 75,
  };
  const upTo22 = { kind: "UpTo", ceiling_rate: 0.22 } as const;
  const expense: EffectSpec = { kind: "Expense", from_account_id: 6, amount: { kind: "Fixed", value: 10 } } as EffectSpec;
  const conversion: EffectSpec = {
    kind: "RothConversion",
    from_account_id: 3,
    to_account_id: 2,
    amount: { kind: "Fixed", value: 20000 },
    pay_tax_from_account_id: null,
  };
  const event = (id: number, effects: EffectSpec[], enabled = true): Event => ({
    id,
    name: `Event ${id}`,
    description: null,
    fires_once: false,
    enabled,
    sort_order: id,
    trigger: { kind: "Repeating", interval: "Yearly", start_condition: null, end_condition: null, max_occurrences: null },
    effects,
  });

  // None to retarget: the overlay's event, at the rate picked.
  const [added] = conversionWrites([event(1, [expense])], upTo22, overlay);
  assert.equal(added.kind, "create");
  const created = added.body;
  assert.deepEqual(created.trigger, {
    kind: "Repeating",
    interval: "Yearly",
    start_condition: { kind: "Date", on_date: "2036-12-30" },
    end_condition: { kind: "Age", years: 75, months: null },
    max_occurrences: null,
  });
  assert.deepEqual(created.effects, [{
    kind: "RothConversion",
    from_account_id: 3,
    to_account_id: 4,
    amount: { kind: "Expression", source: "bracket_room(22%)" },
    pay_tax_from_account_id: 1,
  }]);
  assert.deepEqual(conversionWrites([], { kind: "Off" }, overlay), []);
  assert.deepEqual(conversionWrites([], upTo22, null), []);

  // The plan's own (enabled) conversions are retargeted in place; a disabled
  // one is left alone.
  const plan = [event(1, [expense]), event(2, [conversion]), event(3, [conversion], false)];
  const writes = conversionWrites(plan, upTo22, overlay);
  assert.equal(writes.length, 1);
  const [write] = writes;
  assert.equal(write.kind, "replace");
  assert.equal(write.kind === "replace" && write.id, 2);
  assert.equal(write.body.sort_order, 2, "it keeps its place");
  assert.deepEqual(write.body.effects, [{ ...conversion, amount: { kind: "Expression", source: "bracket_room(22%)" } }]);

  // None: a conversion-only event is disabled; a mixed one loses its conversion.
  const off = conversionWrites([event(2, [conversion]), event(4, [expense, conversion])], { kind: "Off" }, overlay);
  assert.deepEqual(off.map((w) => [w.body.enabled, w.body.effects.length]), [[false, 1], [true, 1]]);
  assert.equal(off[1].body.effects[0].kind, "Expense");
});
