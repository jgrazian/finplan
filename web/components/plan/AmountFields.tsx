"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import type { EffectSpec } from "@/lib/api/types";
import { api } from "@/lib/api/client";
import { Button, Field } from "@/components/ui";
import { Note, type TriggerContext } from "./TriggerFields";
import { byteSpanToText, completionToken, nameInsertion, parameterReference, replaceText, type TextRange } from "./amountDraft";

type Validation = Awaited<ReturnType<typeof api.expressions.validate>>;
const FUNCTIONS = [
  "inflation(", "balance(", "cash(", "holding(", "endpoint_balance(",
  "source_balance()", "target_balance()", "net_worth()", "min(", "max(",
  "clamp(", "abs(", "if(", "top_up(", "payoff()", "age()", "age_years(",
  "year()", "month()", "years_since_start()", "days_until(", "years_until(",
];

function contextHint(effect: EffectSpec): string {
  switch (effect.kind) {
    case "Income": return "target is the destination cash account; source is unavailable.";
    case "Expense": return "source is the paying cash account; target is unavailable.";
    case "CashTransfer": return "source is the paying cash account; target is the receiving account.";
    case "AssetPurchase": return "source is the paying cash account; target is the purchased holding.";
    case "AssetSale": return "source is the selling account; target is its cash. source_balance() selects the holding when one is named.";
    case "Sweep": return "target is the destination cash account. source is available only for a single account or holding.";
    case "AdjustBalance": return "target is the adjusted account; source is unavailable.";
    default: return "Balance references depend on this effect's accounts.";
  }
}

/** A source editor for the compiler's amount DSL. Parent owns the unsaved text. */
export function AmountExpression({ source, effect, context, onChange, disabled }: {
  source: string;
  effect: EffectSpec;
  context: TriggerContext;
  onChange: (next: string) => void;
  disabled?: boolean;
}) {
  const input = useRef<HTMLTextAreaElement>(null);
  const [selection, setSelection] = useState<TextRange>({ start: source.length, end: source.length });
  const [validated, setValidated] = useState<{ key: string; result: Validation } | null>(null);
  const [checkingKey, setCheckingKey] = useState<string | null>(null);
  const [requestError, setRequestError] = useState<{ key: string; message: string } | null>(null);
  const [showSuggestions, setShowSuggestions] = useState(false);
  const effectKey = JSON.stringify(effect);
  const parameterKey = JSON.stringify(context.parameters ?? []);
  const requestKey = `${context.scenarioId}:${effectKey}:${parameterKey}`;
  const validation = validated?.key === requestKey ? validated.result : null;
  const visibleError = requestError?.key === requestKey ? requestError.message : null;
  const selected = useMemo(() => ({
    start: Math.min(selection.start, source.length),
    end: Math.min(selection.end, source.length),
  }), [selection, source.length]);
  const token = useMemo(() => completionToken(source, selected.start), [source, selected.start]);
  const candidates = useMemo(() => {
    if (!token) return [];
    const query = token.query.toLowerCase();
    if (token.kind === "parameter") {
      const namePrefix = token.query.startsWith('$"')
        ? token.query.slice(2).toLowerCase() : token.query.slice(1).toLowerCase();
      return (context.parameters ?? [])
        .filter((p) => p.value.kind === "Money" || p.value.kind === "Rate")
        .map((p) => ({ label: p.name, insert: parameterReference(p.name) }))
        .filter((p) => p.label.toLowerCase().startsWith(namePrefix));
    }
    return [
      ...(token.kind === "quoted" ? [] : FUNCTIONS.map((fn) => ({ label: fn, insert: fn }))),
      ...context.accounts.map((a) => ({ label: `Account: ${a.name}`, insert: JSON.stringify(a.name) })),
      ...context.assets.map((a) => ({ label: `Asset: ${a.name}`, insert: JSON.stringify(a.name) })),
    ].filter((c) => c.label.toLowerCase().includes(query)).slice(0, 8);
  }, [context.accounts, context.assets, context.parameters, token]);

  useEffect(() => {
    if (context.scenarioId == null || !source.trim()) {
      return;
    }
    let active = true;
    const timer = setTimeout(() => {
      setCheckingKey(requestKey);
      api.expressions.validate(context.scenarioId!, { effect })
        .then((result) => {
          if (!active) return;
          setValidated({ key: requestKey, result });
          setRequestError(null);
        })
        .catch((error: unknown) => {
          if (!active) return;
          setValidated(null);
          setRequestError({ key: requestKey, message: error instanceof Error ? error.message : "Validation unavailable" });
        })
        .finally(() => { if (active) setCheckingKey(null); });
    }, 350);
    return () => { active = false; clearTimeout(timer); };
  }, [source, effect, context.scenarioId, requestKey]);

  function commit(range: TextRange, value: string) {
    const result = replaceText(source, range, value);
    onChange(result.source);
    setShowSuggestions(false);
    setSelection({ start: result.cursor, end: result.cursor });
    requestAnimationFrame(() => {
      input.current?.focus();
      input.current?.setSelectionRange(result.cursor, result.cursor);
    });
  }

  function insert(value: string, mode: "selection" | "completion" = "selection") {
    const range = mode === "completion" && token
      && selected.start >= token.start && selected.end <= token.end ? token : selected;
    commit(range, value);
  }

  function insertName(name: string, kind: "parameter" | "account") {
    const insertion = nameInsertion(source, selected, token, name, kind);
    commit(insertion.range, insertion.text);
  }

  const diagnostic = validation?.diagnostics[0];
  const [errorStart, errorEnd] = diagnostic
    ? byteSpanToText(source, diagnostic.start, diagnostic.end)
    : [0, 0];

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <Field label="Value expression">
        <textarea
          ref={input}
          className="input"
          aria-label="Value expression"
          aria-invalid={!!diagnostic}
          value={source}
          readOnly={disabled}
          spellCheck={false}
          rows={3}
          style={{ width: "100%", minHeight: 84, resize: "vertical", fontFamily: "var(--font-mono, monospace)" }}
          onChange={(event) => {
            onChange(event.target.value);
            setSelection({ start: event.target.selectionStart, end: event.target.selectionEnd });
            setShowSuggestions(true);
            setValidated(null);
          }}
          onSelect={(event) => setSelection({ start: event.currentTarget.selectionStart, end: event.currentTarget.selectionEnd })}
          onClick={(event) => setSelection({ start: event.currentTarget.selectionStart, end: event.currentTarget.selectionEnd })}
          onKeyUp={(event) => setSelection({ start: event.currentTarget.selectionStart, end: event.currentTarget.selectionEnd })}
        />
      </Field>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        <select
          aria-label="Insert parameter"
          disabled={disabled || !context.parameters?.length}
          value=""
          onChange={(event) => {
            const parameter = context.parameters?.find((p) => p.id === Number(event.target.value));
            if (parameter) insertName(parameter.name, "parameter");
          }}
        >
          <option value="">Insert parameter…</option>
          {(context.parameters ?? []).map((p) => <option key={p.id} value={p.id}>{p.name} · {p.value.kind}</option>)}
        </select>
        <select aria-label="Insert account" disabled={disabled || context.accounts.length === 0} value=""
          onChange={(event) => {
            const account = context.accounts.find((a) => a.id === Number(event.target.value));
            if (account) insertName(account.name, "account");
          }}>
          <option value="">Insert account…</option>
          {context.accounts.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
        </select>
        <select aria-label="Insert asset" disabled={disabled || context.assets.length === 0} value=""
          onChange={(event) => {
            const asset = context.assets.find((a) => a.id === Number(event.target.value));
            if (asset) insertName(asset.name, "account");
          }}>
          <option value="">Insert asset…</option>
          {context.assets.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
        </select>
        <Button variant="ghost" disabled={disabled} onClick={() => insert("inflation(")}>Inflation</Button>
        <Button variant="ghost" disabled={disabled} onClick={() => insert("balance(")}>Account balance</Button>
        {checkingKey === requestKey && <span style={{ fontSize: 12 }}>Checking expression…</span>}
      </div>
      {showSuggestions && candidates.length > 0 && (
        <div role="listbox" aria-label="Expression suggestions" style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
          {candidates.map((candidate) => (
            <Button key={candidate.insert} variant="ghost" disabled={disabled} onClick={() => insert(candidate.insert, "completion")}>
              {candidate.label}
            </Button>
          ))}
        </div>
      )}
      <Note>{contextHint(effect)}</Note>
      {diagnostic && (
        <div role="alert" style={{ fontSize: 12, color: "var(--color-danger, #a22)" }}>
          {diagnostic.message}
          <div style={{ fontFamily: "monospace", whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
            {source.slice(0, errorStart)}<span style={{ textDecoration: "underline wavy currentColor", fontWeight: 700 }}>
              {source.slice(errorStart, errorEnd) || "▴"}
            </span>{source.slice(errorEnd)}
          </div>
        </div>
      )}
      {visibleError && <Note>Expression validation unavailable: {visibleError}</Note>}
      {validation?.valid && (
        <Note>
          {validation.preview_value == null
            ? validation.preview_label ?? "Value depends on simulation state."
            : `${validation.preview_label ?? "Value at plan start"}: ${validation.preview_value.toLocaleString(undefined, { style: "currency", currency: "USD" })}`}
        </Note>
      )}
    </div>
  );
}
