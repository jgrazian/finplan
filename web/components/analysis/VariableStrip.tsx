"use client";

import { Button, Dropdown, Field, NumberInput, StatLabel, Table, Td, Th } from "@/components/ui";
import type { AnalysisParameter } from "@/lib/api/types";
import { ParameterRangeInput } from "./ParameterRangeInput";
import { paramId, paramValue } from "@/lib/view/analysis";

/** One variable the sweep steps over, as the screen holds it. */
export interface SweptVariable {
  parameterId: string;
  min: number;
  max: number;
  steps: number;
  /** Unticked variables stay in the list but are left out of the next run. */
  enabled: boolean;
}

/** A variable seeded from a parameter's own suggested range. */
export function variableFor(parameter: AnalysisParameter, steps: number): SweptVariable {
  return {
    parameterId: parameter.id,
    min: parameter.min,
    max: parameter.max,
    steps,
    enabled: true,
  };
}

/** Combinations a set of variables would evaluate. */
export function combinations(variables: SweptVariable[]): number {
  return variables
    .filter((v) => v.enabled)
    .reduce((total, variable) => total * Math.max(1, variable.steps), 1);
}

/**
 * Monte Carlo iterations behind each combination.
 *
 * The server's own floor and ceiling, restated so the field refuses a figure
 * here rather than sending one that comes back a 400.
 */
export const MIN_ITERATIONS = 25;
export const MAX_ITERATIONS = 2_000;

/**
 * The swept set, collapsed to a line.
 *
 * What is swept is decided once and then read off for the rest of a session, so
 * it sits above the graphs as a summary that opens rather than as a permanent
 * table: the ranges are the only thing on this screen that costs another run,
 * and folding them away is what makes room for the graphs that do not.
 */
export function VariableStrip({
  parameters,
  variables,
  open,
  onToggleOpen,
  onChange,
  onRetarget,
  onAdd,
  onRemove,
  iterations,
  onIterations,
  dirty,
  disabled,
}: {
  parameters: AnalysisParameter[];
  variables: SweptVariable[];
  open: boolean;
  onToggleOpen: () => void;
  onChange: (parameterId: string, next: SweptVariable) => void;
  /** Point a row at a different parameter, which brings its own range. */
  onRetarget: (parameterId: string, next: string) => void;
  onAdd: () => void;
  onRemove: (parameterId: string) => void;
  /** Monte Carlo iterations behind every combination. */
  iterations: number;
  onIterations: (iterations: number) => void;
  /** The set has been edited since the sweep on screen was run. */
  dirty?: boolean;
  disabled?: boolean;
}) {
  const byId = new Map(parameters.map((p) => [p.id, p]));
  const enabled = variables.filter((v) => v.enabled);
  const summary =
    enabled.length === 0
      ? "nothing selected"
      : enabled.map((v) => label(byId, v.parameterId)).join(" × ");
  const free = parameters.filter((p) => !variables.some((v) => v.parameterId === p.id));
  const points = combinations(variables);

  return (
    <div style={{ borderBottom: "1px solid var(--color-divider)" }}>
      <button
        type="button"
        onClick={onToggleOpen}
        aria-expanded={open}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          width: "100%",
          padding: "9px 20px",
          background: "none",
          border: 0,
          cursor: "pointer",
          textAlign: "left",
          font: "inherit",
          color: "inherit",
        }}
      >
        <StatLabel>swept</StatLabel>
        <span
          style={{
            fontFamily: "ui-monospace, Menlo, monospace",
            fontSize: 11.5,
            color: "color-mix(in srgb, var(--color-text) 75%, transparent)",
          }}
        >
          {summary}
        </span>
        <span
          style={{
            marginLeft: "auto",
            fontSize: 11.5,
            color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
          }}
        >
          {enabled.length === 0
            ? "—"
            : `${points} combinations × ${iterations} iterations`}
          {dirty && " · not run yet"}
        </span>
        <span className="sbtn">{open ? "Done" : "Edit"}</span>
      </button>

      {open && (
        <div style={{ padding: "4px 20px 14px" }}>
          <Table compact>
            <thead>
              <tr>
                <Th style={{ width: 36 }}> </Th>
                <Th>Variable</Th>
                <Th>From</Th>
                <Th>To</Th>
                <Th>Steps</Th>
                <Th>Plan value</Th>
                <Th> </Th>
              </tr>
            </thead>
            <tbody>
              {variables.map((variable) => {
                const parameter = byId.get(variable.parameterId);
                const kind = parameter?.kind ?? "amount";
                return (
                  <tr key={variable.parameterId}>
                    <Td>
                      <input
                        type="checkbox"
                        aria-label={`Sweep ${label(byId, variable.parameterId)}`}
                        checked={variable.enabled}
                        disabled={disabled}
                        onChange={(e) =>
                          onChange(variable.parameterId, {
                            ...variable,
                            enabled: e.target.checked,
                          })
                        }
                        style={{ accentColor: "var(--color-accent)", margin: 0 }}
                      />
                    </Td>
                    <Td>
                      {/* A row is a variable, and which variable it is is as
                          editable as its range: swapping one is the same edit
                          as moving its bounds, and removing and re-adding to
                          do it would lose the row's place in the list. */}
                      <Dropdown
                        inline
                        ariaLabel="Variable"
                        disabled={disabled}
                        options={parameters.map((p) => ({
                          value: p.id,
                          label: paramId(p),
                          detail: p.kind,
                          disabled:
                            p.id !== variable.parameterId &&
                            variables.some((v) => v.parameterId === p.id),
                        }))}
                        value={variable.parameterId}
                        onChange={(next) => onRetarget(variable.parameterId, next)}
                      />
                    </Td>
                    <Td>
                      <ParameterRangeInput
                        label="from"
                        kind={kind}
                        value={variable.min}
                        disabled={disabled}
                        onCommit={(min) => onChange(variable.parameterId, { ...variable, min })}
                      />
                    </Td>
                    <Td>
                      <ParameterRangeInput
                        label="to"
                        kind={kind}
                        value={variable.max}
                        disabled={disabled}
                        onCommit={(max) => onChange(variable.parameterId, { ...variable, max })}
                      />
                    </Td>
                    <Td>
                      <Cell
                        label="steps"
                        value={variable.steps}
                        width={54}
                        min={2}
                        max={12}
                        disabled={disabled}
                        onCommit={(steps) =>
                          onChange(variable.parameterId, { ...variable, steps: Math.round(steps) })
                        }
                      />
                    </Td>
                    <Td muted>
                      {parameter ? paramValue(parameter.kind, parameter.current) : "—"}
                    </Td>
                    <Td align="right">
                      <button
                        type="button"
                        className="sbtn"
                        disabled={disabled}
                        onClick={() => onRemove(variable.parameterId)}
                      >
                        Remove
                      </button>
                    </Td>
                  </tr>
                );
              })}
            </tbody>
          </Table>

          <div style={{ display: "flex", gap: 14, alignItems: "flex-end", marginTop: 8 }}>
            <Button variant="add" disabled={disabled || free.length === 0} onClick={onAdd}>
              Add variable
            </Button>
            {free.length === 0 && (
              <span
                style={{
                  fontSize: 11.5,
                  paddingBottom: 6,
                  color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
                }}
              >
                every parameter this plan can vary is already here.
              </span>
            )}
            {/* The other half of what a sweep costs. Steps decide how many
                combinations there are; this decides how hard each one is
                measured, and a grid read at 100 iterations is a grid whose
                neighbouring cells differ by sampling noise. */}
            <Field label="Simulations per point" style={{ marginLeft: "auto", width: 150 }}>
              <NumberInput
                aria-label="Monte Carlo iterations behind each combination"
                value={iterations}
                decimals={0}
                min={MIN_ITERATIONS}
                max={MAX_ITERATIONS}
                disabled={disabled}
                onCommit={(next) => onIterations(Math.round(next))}
                style={{ minHeight: 28 }}
              />
            </Field>
            <span
              style={{
                fontSize: 11.5,
                paddingBottom: 6,
                color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
              }}
            >
              {(points * iterations).toLocaleString("en-US")} simulations
            </span>
          </div>
        </div>
      )}
    </div>
  );
}

function Cell({
  label,
  value,
  onCommit,
  money,
  width = 92,
  min,
  max,
  disabled,
}: {
  label: string;
  value: number;
  onCommit: (value: number) => void;
  money?: boolean;
  width?: number;
  min?: number;
  max?: number;
  disabled?: boolean;
}) {
  return (
    <NumberInput
      aria-label={label}
      value={value}
      onCommit={onCommit}
      decimals={0}
      min={min}
      max={max}
      disabled={disabled}
      prefix={money ? "$" : undefined}
      style={{ width, minHeight: 28 }}
    />
  );
}

function label(byId: Map<string, AnalysisParameter>, id: string): string {
  const parameter = byId.get(id);
  return parameter ? paramId(parameter) : id;
}
