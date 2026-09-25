import assert from "node:assert/strict";
import { test } from "node:test";
import {
  amountSource,
  byteSpanToText,
  completionToken,
  nameInsertion,
  parameterReference,
  renameAmountReferences,
  renameParameterReferences,
  readStaticAmount,
  replaceText,
  rootAmountMode,
  staticAmount,
  withRootAmountMode,
} from "../components/plan/amountDraft.ts";
import { collapseAmount, draftOfEffect, emptyEffect, expandAmount, toEffectSpec, updateExpression } from "../components/plan/effectDraft.ts";

const names = {
  account: (id: number) => id === 1 ? "Vanguard" : `account ${id}`,
  asset: (id: number) => id === 2 ? "VTI" : `asset ${id}`,
};

test("legacy amount trees become equivalent editable expressions", () => {
  assert.equal(amountSource({ kind: "Scale", factor: 0.04, inner: {
    kind: "AccountTotalBalance", account_id: 1,
  } }, names), '(0.04 * balance("Vanguard"))');
  assert.equal(amountSource({ kind: "AssetBalance", account_id: 1, asset_id: 2 }, names),
    'holding("Vanguard", "VTI")');
  assert.equal(amountSource({ kind: "ZeroTargetBalance" }, names), "payoff()");
});

test("static entry round trips but a parameter formula cannot become static", () => {
  assert.deepEqual(readStaticAmount(staticAmount(8750, true)),
    { value: 8750, inflationAdjusted: true });
  assert.equal(readStaticAmount({ kind: "Expression", source: "$Salary" }), null);
});

test("tax annotations preserve the expression inside them", () => {
  assert.equal(rootAmountMode('net(top_up(inflation($Salary)))'), "Net");
  assert.equal(rootAmountMode('net (top_up(inflation($Salary)))'), "Net");
  assert.equal(rootAmountMode('gross\n (100)'), "Gross");
  assert.equal(rootAmountMode('gross($Salary) + 1'), null);
  assert.equal(rootAmountMode('gross($"unfinished'), null);
  assert.equal(withRootAmountMode('net(top_up(inflation($Salary)))', "Gross"),
    'gross(top_up(inflation($Salary)))');
  assert.equal(withRootAmountMode('net (100)', "Gross"), 'gross(100)');
});

test("completion quotes names and compiler byte spans map to text offsets", () => {
  assert.equal(parameterReference("Monthly spending"), '$"Monthly spending"');
  assert.equal(parameterReference("Salary"), "$Salary");
  assert.deepEqual(byteSpanToText("é$Salary", 2, 9), [1, 8]);
});

test("completion replaces a full quoted argument or identifier while explicit insertion uses the selection", () => {
  const quoted = 'balance("Van")';
  const token = completionToken(quoted, quoted.indexOf("Van") + 3);
  assert.deepEqual(token, { start: 8, end: 13, query: "Van", kind: "quoted" });
  assert.equal(replaceText(quoted, token!, JSON.stringify('Van "A"')).source,
    'balance("Van \\"A\\"")');
  assert.deepEqual(completionToken("balance", 3),
    { start: 0, end: 7, query: "bal", kind: "identifier" });
  assert.equal(replaceText("8750", { start: 0, end: 4 }, "$Salary").source, "$Salary");
  assert.deepEqual(replaceText("12", { start: 99, end: 99 }, "3"), { source: "123", cursor: 3 });
});

test("quoted parameter completion identifies the full token for a plain-name match", () => {
  const source = '$"Salary" + 1';
  assert.deepEqual(completionToken(source, source.indexOf("Salary") + 2),
    { start: 0, end: 9, query: '$"Sa', kind: "parameter" });
  assert.equal(replaceText(source, { start: 0, end: 9 }, parameterReference("Salary")).source,
    "$Salary + 1");
});

test("inserting a name into selected quoted text preserves the surrounding quotes", () => {
  const source = 'balance("Van")';
  const selection = { start: 9, end: 12 };
  const insertion = nameInsertion(source, selection, completionToken(source, selection.start), 'Van "A"', "account");
  assert.deepEqual(insertion.range, selection);
  assert.equal(replaceText(source, insertion.range, insertion.text).source, 'balance("Van \\"A\\"")');

  const parameter = '$"Sa" + 1';
  const selectedName = { start: 2, end: 4 };
  const parameterInsertion = nameInsertion(parameter, selectedName,
    completionToken(parameter, selectedName.start), 'Monthly "spend"', "parameter");
  assert.equal(replaceText(parameter, parameterInsertion.range, parameterInsertion.text).source,
    '$"Monthly \\"spend\\"" + 1');
});

test("inserting at a caret inside quotes replaces the entire quoted token", () => {
  const source = 'balance("Van")';
  const caret = { start: 10, end: 10 };
  const insertion = nameInsertion(source, caret, completionToken(source, caret.start), "Vanguard", "account");
  assert.equal(replaceText(source, insertion.range, insertion.text).source, 'balance("Vanguard")');
  const parameter = '$"Sa" + 1';
  const paramCaret = { start: 3, end: 3 };
  const paramInsertion = nameInsertion(parameter, paramCaret, completionToken(parameter, paramCaret.start),
    "Salary", "parameter");
  assert.equal(replaceText(parameter, paramInsertion.range, paramInsertion.text).source, '$Salary + 1');
});

test("parameter rename keeps formulas bound without changing account strings", () => {
  assert.equal(renameParameterReferences('cash("$Salary") + $Salary + $"Salary"', "Salary", "Base pay"),
    'cash("$Salary") + $"Base pay" + $"Base pay"');
  assert.equal(renameParameterReferences('$"bad\\q" + $Salary', "Salary", "Base pay"),
    '$"bad\\q" + $"Base pay"');
  assert.deepEqual(renameAmountReferences({ kind: "InflationAdjusted", inner: {
    kind: "Expression", source: "$Salary",
  } }, "Salary", "Base pay"), {
    kind: "InflationAdjusted", inner: { kind: "Expression", source: '$"Base pay"' },
  });
});

test("switching entry modes retains a literal expression draft", () => {
  const initial = emptyEffect(1, 2);
  initial.amount = 8750;
  const expanded = { ...initial, ...expandAmount(initial, names) };
  assert.deepEqual(toEffectSpec(expanded).kind, "Income");
  assert.equal(expanded.rawAmount?.kind, "Expression");
  const edited = { ...expanded, ...updateExpression(expanded, "inflation(9000)") };
  const collapsed = { ...edited, ...collapseAmount(edited) };
  assert.equal(collapsed.amount, 9000);
  assert.equal(collapsed.expressionDraft, "inflation(9000)");
  assert.equal(expandAmount(collapsed, names).rawAmount?.kind, "Expression");
});

test("computed and parameter expressions cannot collapse to an old static value", () => {
  const draft = emptyEffect(1, 2);
  draft.amount = 8750;
  for (const source of ["2 + 2", "$Salary", "if(true, 10, 20)"]) {
    const expression = { ...draft, rawAmount: { kind: "Expression" as const, source } };
    assert.deepEqual(collapseAmount(expression), {});
    const saved = toEffectSpec({ ...expression, ...collapseAmount(expression) });
    assert.equal(saved.kind, "Income");
    if (saved.kind === "Income") assert.deepEqual(saved.amount, { kind: "Expression", source });
  }
});

test("root tax annotation updates a compatible effect's mode", () => {
  const income = emptyEffect(1, 2);
  const change = updateExpression(income, "gross($Salary)");
  assert.equal(change.amountMode, "Gross");
  assert.deepEqual(change.rawAmount, { kind: "Expression", source: "gross($Salary)" });
});

test("saved root annotation wins over an older amount_mode column", () => {
  const draft = draftOfEffect({
    kind: "Income", to_account_id: 1, amount: { kind: "Expression", source: "gross($Salary)" },
    amount_mode: "Net", income_type: "Taxable",
  }, 1, 2, names);
  assert.equal(draft.amountMode, "Gross");
  assert.equal(toEffectSpec(draft).kind, "Income");
});
