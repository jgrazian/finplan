"use client";

import { useEffect, useRef, useState } from "react";
import { Button, CompactInput, CurrencyInput, DateInput, Dropdown, Field, NumberInput, Tag } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { NamedParameter, ParameterValueSpec } from "@/lib/api/types";

type Kind = ParameterValueSpec["kind"];
const KINDS: Kind[] = ["Money", "Rate", "Date", "Age"];
const muted = "color-mix(in srgb, var(--color-text) 58%, transparent)";

export function blankParameterValue(kind: Kind): ParameterValueSpec {
  switch (kind) {
    case "Money": return { kind, value: 0 };
    case "Rate": return { kind, value: 0 };
    case "Date": return { kind, value: new Date().toISOString().slice(0, 10) };
    case "Age": return { kind, years: 65, months: 0 };
  }
}

function valueLabel(value: ParameterValueSpec): string {
  switch (value.kind) {
    case "Money": return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(value.value);
    case "Rate": return `${(value.value * 100).toLocaleString("en-US", { maximumFractionDigits: 2 })}%`;
    case "Date": return value.value;
    case "Age": return `${value.years} yr${value.months ? ` ${value.months} mo` : ""}`;
  }
}

export function ParameterRail({ parameters, selectedId, onSelect, onAdd, adding, loading, error, submitError, offline }: {
  parameters: NamedParameter[];
  selectedId?: number;
  onSelect: (id: number) => void;
  onAdd: (kind: Kind) => void;
  adding?: boolean;
  loading?: boolean;
  error?: Error;
  submitError?: string;
  offline?: boolean;
}) {
  const [expanded, setExpanded] = useState(true);
  return (
    <div style={{ borderTop: "1px solid var(--color-divider)", display: "flex", flexDirection: "column", minHeight: 0, maxHeight: 230, overflow: "hidden" }}>
      <div style={{ padding: "14px 16px 8px", display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 8, flexShrink: 0 }}>
        <h4 style={{ margin: 0 }}>
          <button type="button" aria-expanded={expanded} aria-label="Toggle parameters" onClick={() => setExpanded((open) => !open)}
            style={{ background: "none", border: 0, padding: 0, color: "inherit", cursor: "pointer", font: "inherit", textAlign: "left" }}>
            <span aria-hidden="true">{expanded ? "▾" : "▸"}</span> Parameters <span className="text-muted" style={{ fontSize: 13 }}>{parameters.length}</span>
          </button>
        </h4>
        <Button variant="ghost" onClick={() => { setExpanded(true); onAdd("Money"); }} disabled={offline || adding || !!error}
          aria-label="Add parameter" title={offline ? "No connection to the server." : undefined}>
          Add
        </Button>
      </div>
      {expanded && <div style={{ flex: "0 1 auto", minHeight: 0, overflowY: "auto", overflowX: "hidden" }}>
        {loading && parameters.length === 0 && <p style={{ padding: "4px 16px", fontSize: 12, color: muted }}>Loading parameters…</p>}
        {error && <p role="alert" style={{ padding: "4px 16px", fontSize: 12, color: "var(--color-accent-900)" }}>{error.message}</p>}
        {submitError && <p role="alert" style={{ padding: "4px 16px", fontSize: 12, color: "var(--color-accent-900)" }}>{submitError}</p>}
        {!loading && !error && parameters.length === 0 && <p style={{ padding: "4px 16px", fontSize: 12, color: muted }}>Name values to reuse in events and analysis.</p>}
        <div role="listbox" aria-label="Parameters">
          {parameters.map((p) => <div key={p.id} role="option" tabIndex={0} aria-selected={p.id === selectedId}
            onClick={() => onSelect(p.id)} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onSelect(p.id); } }}
            style={{ cursor: "pointer", borderTop: "1px solid var(--color-divider)", padding: "9px 16px", background: p.id === selectedId ? "color-mix(in srgb, var(--color-accent) 14%, transparent)" : undefined, boxShadow: p.id === selectedId ? "inset 3px 0 0 var(--color-accent)" : undefined }}>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 6, fontSize: 12.5 }}><strong style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{p.name}</strong><Tag tone="outline">{p.value.kind}</Tag></div>
            <div style={{ fontSize: 12, color: muted, marginTop: 3 }}>{valueLabel(p.value)}</div>
          </div>)}
        </div>
      </div>}
    </div>
  );
}

export function ParameterEditor({ parameter, scenarioId, onSaved, onDeleted, onSelectEvent, offline }: {
  parameter: NamedParameter;
  scenarioId: number;
  onSaved: () => void;
  onDeleted: () => void;
  onSelectEvent: (name: string) => void;
  offline?: boolean;
}) {
  const [name, setName] = useState(parameter.name);
  const [value, setValue] = useState<ParameterValueSpec>(parameter.value);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const previous = useRef(parameter);
  useEffect(() => {
    const before = previous.current;
    previous.current = parameter;
    if (before.id !== parameter.id || before.scenario_id !== parameter.scenario_id) {
      setName(parameter.name);
      setValue(parameter.value);
      return;
    }
    if (before.name !== parameter.name) {
      setName((current) => current === before.name ? parameter.name : current);
    }
    if (JSON.stringify(before.value) !== JSON.stringify(parameter.value)) {
      setValue((current) => JSON.stringify(current) === JSON.stringify(before.value)
        ? parameter.value : current);
    }
  }, [parameter]);
  const dirty = name !== parameter.name || JSON.stringify(value) !== JSON.stringify(parameter.value);
  const save = async () => {
    if (!name.trim()) return setError("Enter a parameter name.");
    const submittedName = name;
    const submittedValue = value;
    setBusy(true); setError(undefined);
    try {
      const saved = await api.parameters.update(scenarioId, parameter.id, { name: name.trim(), value });
      setName((current) => current === submittedName ? saved.name : current);
      setValue((current) => JSON.stringify(current) === JSON.stringify(submittedValue)
        ? saved.value : current);
      onSaved();
    }
    catch (err) { setError(err instanceof Error ? err.message : String(err)); }
    finally { setBusy(false); }
  };
  const remove = async () => {
    if (parameter.uses.length) return;
    if (!confirm(`Delete ${parameter.name}?`)) return;
    setBusy(true); setError(undefined);
    try { await api.parameters.remove(scenarioId, parameter.id); onDeleted(); }
    catch (err) { setError(err instanceof Error ? err.message : String(err)); }
    finally { setBusy(false); }
  };
  return <div style={{ padding: "18px 24px", minWidth: 0 }}>
    <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap", borderBottom: "1px solid var(--color-divider)", paddingBottom: 16 }}>
      <h4 style={{ margin: 0 }}>Parameter</h4>
      <span style={{ color: muted, fontSize: 12 }}>Used in this scenario</span>
      {dirty && <span style={{ color: muted, fontSize: 12 }}>Unsaved</span>}
      <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
        {dirty && <Button variant="ghost" disabled={busy} onClick={() => { setName(parameter.name); setValue(parameter.value); setError(undefined); }}>Revert</Button>}
        {dirty && <Button variant="primary" disabled={busy || offline} onClick={save}>Apply</Button>}
        <Button variant="ghost" disabled={busy || offline || parameter.uses.length > 0} title={parameter.uses.length ? "Remove its references before deleting this parameter." : undefined} onClick={remove}>Delete</Button>
      </div>
    </div>
    {error && <p role="alert" style={{ fontSize: 12, color: "var(--color-accent-900)" }}>{error}</p>}
    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(190px, 1fr))", gap: 16, maxWidth: 720, marginTop: 20 }}>
      <Field label="Name"><CompactInput aria-label="Parameter name" value={name} readOnly={offline} onChange={(e) => setName(e.target.value)} /></Field>
      <Field label="Type"><Dropdown className="dd-field" value={value.kind} options={KINDS.map((kind) => ({ value: kind, label: kind }))} ariaLabel="Parameter type" disabled={offline || parameter.uses.length > 0} onChange={(kind) => setValue(blankParameterValue(kind))} /></Field>
      {value.kind === "Money" && <Field label="Value"><CurrencyInput value={value.value} allowNegative readOnly={offline} onValueChange={(next) => setValue({ ...value, value: next })} aria-label="Parameter money value" /></Field>}
      {value.kind === "Rate" && <Field label="Rate"><NumberInput value={value.value * 100} readOnly={offline} onValueChange={(next) => setValue({ ...value, value: next / 100 })} suffix="%" decimals={6} allowNegative aria-label="Parameter rate" /></Field>}
      {value.kind === "Date" && <Field label="Date"><DateInput value={value.value} disabled={offline} onValueChange={(next) => setValue({ ...value, value: next })} ariaLabel="Parameter date" /></Field>}
      {value.kind === "Age" && <><Field label="Years"><NumberInput value={value.years} readOnly={offline} onValueChange={(years) => setValue({ ...value, years })} min={0} max={255} decimals={0} aria-label="Parameter age years" /></Field><Field label="Months"><NumberInput value={value.months} readOnly={offline} onValueChange={(months) => setValue({ ...value, months })} min={0} max={11} decimals={0} aria-label="Parameter age months" /></Field></>}
    </div>
    <div style={{ borderTop: "1px solid var(--color-divider)", marginTop: 32, paddingTop: 18 }}>
      <h4 style={{ margin: "0 0 8px" }}>Used by {parameter.uses.length}</h4>
      {parameter.uses.length === 0 ? <p style={{ color: muted, fontSize: 12 }}>No events refer to this parameter yet.</p> :
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 8 }}>
          {parameter.uses.map((use, index) => <Button key={`${use.event_id}-${use.location}-${index}`} variant="ghost" onClick={() => onSelectEvent(use.event_name)}>{use.event_name} · {use.location}</Button>)}
        </div>}
      {parameter.uses.length > 0 && <p style={{ color: muted, fontSize: 12 }}>Remove these references before changing its type or deleting this parameter.</p>}
    </div>
  </div>;
}
