"use client";

import { useState } from "react";
import { UnsavedNote } from "@/components/status/UnsavedNote";
import {
  Button,
  CompactInput,
  DateInput,
  Field,
  NumberInput,
  SectionHeading,
} from "@/components/ui";
import { fmtInt } from "@/lib/format";
import type { ScenarioParams } from "@/lib/types";

/** The fields this strip can save; the rest are read-only summaries. */
type Editable = "start" | "durationYears" | "birthDate";

/**
 * Scenario parameters, pinned above the event list. The Scenario tab was six
 * fields and a bracket table — never a destination of its own — so it becomes
 * a strip on the screen it governs.
 *
 * Each field saves on its own. When the server refuses one, the typed value
 * stays in the field and says why underneath — a rejected save is field state,
 * never a bar — and it can be sent again without retyping anything. Once the
 * connection is gone the fields close instead: nothing is held for later, so
 * nothing can be silently lost.
 */
export function ScenarioStrip({
  scenarioName,
  params,
  onChange,
  onRun,
  offline,
}: {
  scenarioName: string;
  params: ScenarioParams;
  /** Saves one field. Rejecting leaves the value in the field, unsaved. */
  onChange?: (patch: Partial<ScenarioParams>) => Promise<void>;
  onRun?: () => void;
  /** No connection: the fields are read-only rather than held for later. */
  offline?: boolean;
}) {
  // What the user entered that the server has not confirmed. Empty in the
  // healthy case: a save that lands is followed by a reload, and `params`
  // carries the new figure.
  const [pending, setPending] = useState<Partial<ScenarioParams>>({});
  const [refused, setRefused] = useState<Partial<Record<Editable, string>>>({});
  const [savedAt, setSavedAt] = useState<string>();

  const shown = { ...params, ...pending };
  const unsaved = Object.keys(pending).length;
  // Editing is closed rather than queued while the server is unreachable: a
  // field that takes a value it cannot save is a promise the app cannot keep.
  const readOnly = !onChange || !!offline;

  const save = async (patch: Partial<ScenarioParams>) => {
    if (!onChange || offline) return;
    setPending((held) => ({ ...held, ...patch }));
    const keys = Object.keys(patch);
    try {
      await onChange(patch);
      setPending((held) => without(held, keys));
      setRefused((held) => without(held, keys));
      setSavedAt(clock());
    } catch (err) {
      const message =
        err instanceof Error && err.name === "NetworkError"
          ? "Not saved — the server did not answer. Your value is kept here."
          : `Not saved — ${err instanceof Error ? err.message : String(err)}`;
      setRefused((held) => ({
        ...held,
        ...Object.fromEntries(keys.map((key) => [key, message])),
      }));
    }
  };

  return (
    <div style={{ padding: "14px 20px", borderBottom: "1px solid var(--color-divider)" }}>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          marginBottom: 8,
        }}
      >
        <SectionHeading>Scenario — {scenarioName}</SectionHeading>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <span
            style={{
              fontSize: 11,
              color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
            }}
          >
            {offline ? "read-only until the server answers" : "applies to every event below"}
          </span>
          <Button variant="primary" shortcut="r" onClick={onRun} disabled={offline}>
            Run
          </Button>
        </div>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(6, 1fr)",
          gap: 12,
          alignItems: "start",
        }}
      >
        {/* A date field empties itself mid-pick, so only a real date is a change. */}
        <Field label="Start" className={refused.start ? "field-unsaved" : undefined}>
          <DateInput
            style={{ minHeight: 30 }}
            value={shown.start}
            readOnly={readOnly}
            onChange={(e) => e.target.value && void save({ start: e.target.value })}
          />
          {refused.start && <UnsavedNote>{refused.start}</UnsavedNote>}
        </Field>

        <Field label="Duration" className={refused.durationYears ? "field-unsaved" : undefined}>
          <NumberInput
            style={{ minHeight: 30 }}
            value={shown.durationYears}
            suffix="years"
            decimals={0}
            min={1}
            max={120}
            readOnly={readOnly}
            aria-label="Duration in years"
            onCommit={(years) => void save({ durationYears: years })}
          />
          {refused.durationYears && <UnsavedNote>{refused.durationYears}</UnsavedNote>}
        </Field>

        <Field label="Birth date" className={refused.birthDate ? "field-unsaved" : undefined}>
          <DateInput
            style={{ minHeight: 30 }}
            value={shown.birthDate}
            readOnly={readOnly}
            onChange={(e) => e.target.value && void save({ birthDate: e.target.value })}
          />
          {refused.birthDate && <UnsavedNote>{refused.birthDate}</UnsavedNote>}
        </Field>

        <Field label="Iterations">
          <CompactInput style={{ minHeight: 30 }} value={fmtInt(params.iterations)} readOnly />
        </Field>
        <Field label="Inflation">
          <CompactInput style={{ minHeight: 30 }} value={params.inflationProfile} readOnly />
        </Field>
        <Field label="Tax config">
          <CompactInput style={{ minHeight: 30 }} value={params.taxConfig} readOnly />
        </Field>
      </div>

      {unsaved > 0 && (
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 10 }}>
          <span
            style={{
              fontSize: 11,
              color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
            }}
          >
            {unsaved} {unsaved === 1 ? "change" : "changes"} not saved
            {savedAt ? ` · last saved ${savedAt}` : ""}
          </span>
          <Button
            onClick={() => {
              setPending({});
              setRefused({});
            }}
          >
            Revert
          </Button>
          <Button
            variant="primary"
            onClick={() => void save(pending)}
            disabled={offline}
            title={offline ? "No connection to the server." : undefined}
          >
            Try again
          </Button>
        </div>
      )}
    </div>
  );
}

function without<T extends object>(source: T, keys: string[]): T {
  const copy = { ...source };
  for (const key of keys) delete copy[key as keyof T];
  return copy;
}

function clock(): string {
  return new Date().toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}
