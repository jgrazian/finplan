"use client";

import { type ReactNode, useState } from "react";
import { UnsavedNote } from "@/components/status/UnsavedNote";
import { Button, CompactInput, DateInput, Field, NumberInput } from "@/components/ui";
import type { ScenarioParams } from "@/lib/types";

/** The fields this strip can save; the rest are read-only summaries. */
type Editable = "start" | "durationYears" | "birthDate";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/**
 * Artboard 10a — the scenario as a single line above the plan.
 *
 * The Scenario tab was six fields and a bracket table, never a destination of
 * its own, so it became a strip on the screen it governs. Here it goes one
 * further and rests as a sentence: what a scenario's parameters mostly need to
 * do is be *checked*, and six inputs is an expensive way to answer "what
 * horizon is this running over?". Editing is a click away and opens in place,
 * so the row the summary described is the row the fields fill in.
 *
 * Each field still saves on its own. When the server refuses one, the typed
 * value stays in the field and says why underneath — a rejected save is field
 * state, never a bar — and it can be sent again without retyping anything.
 * Once the connection is gone the fields close instead: nothing is held for
 * later, so nothing can be silently lost.
 */
export function ScenarioStrip({
  scenarioName,
  params,
  onChange,
  offline,
}: {
  scenarioName: string;
  params: ScenarioParams;
  /** Saves one field. Rejecting leaves the value in the field, unsaved. */
  onChange?: (patch: Partial<ScenarioParams>) => Promise<void>;
  /** No connection: the fields are read-only rather than held for later. */
  offline?: boolean;
}) {
  // What the user entered that the server has not confirmed. Empty in the
  // healthy case: a save that lands is followed by a reload, and `params`
  // carries the new figure.
  const [pending, setPending] = useState<Partial<ScenarioParams>>({});
  const [refused, setRefused] = useState<Partial<Record<Editable, string>>>({});
  const [savedAt, setSavedAt] = useState<string>();
  const [open, setOpen] = useState(false);

  const shown = { ...params, ...pending };
  const unsaved = Object.keys(pending).length;
  // Editing is closed rather than queued while the server is unreachable: a
  // field that takes a value it cannot save is a promise the app cannot keep.
  const readOnly = !onChange || !!offline;
  // Anything unresolved holds the fields open: closing them would hide the one
  // place that says a value was not saved, and where the value still is.
  const expanded = open || unsaved > 0;

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
    <div style={{ borderBottom: "1px solid var(--color-divider)" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          flexWrap: "wrap",
          gap: "4px 12px",
          padding: "9px 20px",
          fontSize: 12.5,
        }}
      >
        <span className="stat-l" style={{ color: "var(--color-text)" }}>
          Scenario — {scenarioName}
        </span>
        <span>
          {shown.start} · {shown.durationYears} yrs
        </span>
        <Bar />
        <span>{shown.birthDate ? `born ${shown.birthDate}` : "no birth date"}</span>
        <Bar />
        <span>{params.inflationProfile}</span>
        <Bar />
        <span>{params.taxConfig}</span>

        <div
          style={{
            marginLeft: "auto",
            display: "flex",
            alignItems: "center",
            gap: 10,
          }}
        >
          {unsaved > 0 && (
            <span style={{ fontSize: 11, color: MUTED }}>
              {unsaved} {unsaved === 1 ? "change" : "changes"} not saved
              {savedAt ? ` · last saved ${savedAt}` : ""}
            </span>
          )}
          <Button
            variant="ghost"
            aria-expanded={expanded}
            onClick={() => setOpen((v) => !v)}
            disabled={unsaved > 0}
            title={
              unsaved > 0 ? "Sort the unsaved fields out before closing." : undefined
            }
          >
            {expanded ? "Done" : "Edit scenario"}
          </Button>
        </div>
      </div>

      {expanded && (
        <div
          style={{
            padding: "0 20px 14px",
            display: "grid",
            gridTemplateColumns: "repeat(5, 1fr)",
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
              onValueChange={(iso) => iso && void save({ start: iso })}
            />
            {refused.start && <UnsavedNote>{refused.start}</UnsavedNote>}
          </Field>

          <Field
            label="Duration"
            className={refused.durationYears ? "field-unsaved" : undefined}
          >
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

          <Field
            label="Birth date"
            className={refused.birthDate ? "field-unsaved" : undefined}
          >
            <DateInput
              style={{ minHeight: 30 }}
              value={shown.birthDate}
              readOnly={readOnly}
              onValueChange={(iso) => iso && void save({ birthDate: iso })}
            />
            {refused.birthDate && <UnsavedNote>{refused.birthDate}</UnsavedNote>}
          </Field>

          <Field label="Inflation">
            <CompactInput
              style={{ minHeight: 30 }}
              value={params.inflationProfile}
              readOnly
            />
          </Field>
          <Field label="Tax config">
            <CompactInput style={{ minHeight: 30 }} value={params.taxConfig} readOnly />
          </Field>

          {unsaved > 0 && (
            <div
              style={{
                gridColumn: "1 / -1",
                display: "flex",
                alignItems: "center",
                gap: 10,
              }}
            >
              <span style={{ fontSize: 11, color: MUTED }}>
                {offline
                  ? "The server is not answering — these are held here, not saved."
                  : "Held here until the server takes them."}
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
      )}
    </div>
  );
}

function Bar(): ReactNode {
  return <span style={{ opacity: 0.35 }}>|</span>;
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
