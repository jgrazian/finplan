"use client";

import { Button, CompactInput, Field, SectionHeading, Select } from "@/components/ui";
import { fmtInt } from "@/lib/format";
import type { ScenarioParams, WithdrawalOrder } from "@/lib/types";

const WITHDRAWAL_ORDERS: WithdrawalOrder[] = [
  "TaxEfficientEarly",
  "ProRata",
  "PenaltyAware",
];

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
  onChange?: (next: ScenarioParams) => void;
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
        <Field label="Start">
          <CompactInput
            style={{ minHeight: 30 }}
            value={params.start}
            onChange={(e) => onChange?.({ ...params, start: e.target.value })}
          />
        </Field>
        <Field label="Duration">
          <CompactInput style={{ minHeight: 30 }} value={`${params.durationYears} years`} readOnly />
        </Field>
        <Field label="Birth date">
          <CompactInput
            style={{ minHeight: 30 }}
            value={params.birthDate}
            onChange={(e) => onChange?.({ ...params, birthDate: e.target.value })}
          />
        </Field>
        <Field label="Iterations">
          <CompactInput style={{ minHeight: 30 }} value={fmtInt(params.iterations)} readOnly />
        </Field>
        <Field label="Inflation">
          <CompactInput style={{ minHeight: 30 }} value={params.inflationProfileId} readOnly />
        </Field>
        <Field label="Withdrawal order">
          <Select
            style={{ minHeight: 30 }}
            value={params.withdrawalOrder}
            onChange={(e) =>
              onChange?.({ ...params, withdrawalOrder: e.target.value as WithdrawalOrder })
            }
          >
            {WITHDRAWAL_ORDERS.map((o) => (
              <option key={o}>{o}</option>
            ))}
          </Select>
        </Field>
      </div>
    </div>
  );
}
