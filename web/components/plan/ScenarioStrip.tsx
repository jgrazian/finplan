"use client";

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

/**
 * Scenario parameters, pinned above the event list. The Scenario tab was six
 * fields and a bracket table — never a destination of its own — so it becomes
 * a strip on the screen it governs.
 */
export function ScenarioStrip({
  scenarioName,
  params,
  onChange,
  onRun,
}: {
  scenarioName: string;
  params: ScenarioParams;
  /** Emits only what the edited field changed; absent leaves the strip read-only. */
  onChange?: (patch: Partial<ScenarioParams>) => void;
  onRun?: () => void;
}) {
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
            applies to every event below
          </span>
          <Button variant="primary" shortcut="r" onClick={onRun}>
            Run
          </Button>
        </div>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "repeat(6, 1fr)", gap: 12 }}>
        {/* A date field empties itself mid-pick, so only a real date is a change. */}
        <Field label="Start">
          <DateInput
            style={{ minHeight: 30 }}
            value={params.start}
            readOnly={!onChange}
            onChange={(e) => e.target.value && onChange?.({ start: e.target.value })}
          />
        </Field>
        <Field label="Duration">
          <NumberInput
            style={{ minHeight: 30 }}
            value={params.durationYears}
            suffix="years"
            decimals={0}
            min={1}
            max={120}
            readOnly={!onChange}
            aria-label="Duration in years"
            onCommit={(years) => onChange?.({ durationYears: years })}
          />
        </Field>
        <Field label="Birth date">
          <DateInput
            style={{ minHeight: 30 }}
            value={params.birthDate}
            readOnly={!onChange}
            onChange={(e) => e.target.value && onChange?.({ birthDate: e.target.value })}
          />
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
    </div>
  );
}
