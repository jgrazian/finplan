"use client";

import { useEffect, useRef, useState } from "react";
import { Button, CompactInput, Dialog, CurrencyInput, DateInput, Dropdown, Field, NumberInput } from "@/components/ui";
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

/**
 * Artboard 15a — the parameters as the rail's second list, at the same weight
 * as the events. The name is monospace because it is the name you type into an
 * expression; the second line trades a type tag for type and usage, which is
 * what you want to know before deleting one.
 */
export function ParameterRail({ parameters, selectedId, onSelect, error }: {
  parameters: NamedParameter[];
  selectedId?: number;
  onSelect: (id: number) => void;
  error?: string;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", minWidth: 0, height: "100%" }}>
      {error && <p role="alert" style={{ margin: 0, padding: "4px 16px 8px", fontSize: 12, color: "var(--color-accent-900)" }}>{error}</p>}
      <div role="listbox" aria-label="Parameters">
        {parameters.map((p) => {
          const on = p.id === selectedId;
          return <div key={p.id} className="rowsel" role="option" tabIndex={0} aria-selected={on}
            onClick={() => onSelect(p.id)} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onSelect(p.id); } }}
            style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) auto", gap: "2px 8px", padding: "10px 18px", borderTop: "1px solid var(--color-divider)", background: on ? "color-mix(in srgb, var(--color-accent) 14%, transparent)" : undefined, boxShadow: on ? "inset 3px 0 0 var(--color-accent)" : undefined }}>
            <span className="cd-name" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{p.name}</span>
            <span style={{ fontSize: 13 }}>{valueLabel(p.value)}</span>
            <span style={{ fontSize: 11.5, color: muted }}>{p.value.kind} · {p.uses.length ? `used by ${p.uses.length}` : "unused"}</span>
          </div>;
        })}
      </div>
      {parameters.length > 0 && <div style={{ borderTop: "1px solid var(--color-divider)" }} />}
      <p style={{ margin: "auto 0 0", padding: "12px 16px 14px", fontSize: 11.5, color: muted }}>
        {parameters.length === 0 ? "Name values to reuse in events and analysis." : "Reference one in an amount as $name."}
      </p>
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
  // Asked in the app's own dialog rather than the browser's, so the question
  // can name what goes and say why it is safe.
  const [confirming, setConfirming] = useState(false);
  const [deleteError, setDeleteError] = useState<string>();
  const remove = async () => {
    if (parameter.uses.length) return;
    setBusy(true); setDeleteError(undefined);
    try { await api.parameters.remove(scenarioId, parameter.id); setConfirming(false); onDeleted(); }
    catch (err) { setDeleteError(err instanceof Error ? err.message : String(err)); }
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
        <Button variant="ghost" disabled={busy || offline || parameter.uses.length > 0} title={parameter.uses.length ? "Remove its references before deleting this parameter." : undefined} onClick={() => { setDeleteError(undefined); setConfirming(true); }}>Delete</Button>
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
    {confirming && <Dialog
      title="Delete parameter"
      submitLabel="Delete"
      busy={busy}
      error={deleteError}
      onClose={() => setConfirming(false)}
      onSubmit={remove}
    >
      <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
        Delete <span className="cd-name">{parameter.name}</span> ({valueLabel(parameter.value)})?
        No event refers to it, so the plan runs the same without it. Analysis will stop offering it as an axis.
      </p>
    </Dialog>}
  </div>;
}
