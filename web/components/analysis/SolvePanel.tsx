"use client";

import { useCallback, useMemo, useState } from "react";
import {
  Blueprint,
  Button,
  Dropdown,
  Field,
  NumberInput,
  Table,
  Td,
  Th,
} from "@/components/ui";
import { fmtInt, fmtPercent } from "@/lib/format";
import type { AnalysisParameter, ConstraintRequest, ObjectiveRequest, SolveOutcome } from "@/lib/api/types";
import { useAnalysis } from "@/lib/hooks/useAnalysis";
import {
  paramId,
  paramValue,
  solveHeadline,
  solveRows,
  solvedParameter,
} from "@/lib/view/analysis";
import { AxisPicker, axisFor, type Axis } from "./AxisPicker";
import { ConvergenceChart } from "./ConvergenceChart";
import { JobProgress } from "./JobProgress";

/** The most parameters a solve will vary at once, matching the server's cap. */
const MAX_VARIED = 3;

const OBJECTIVES: ReadonlyArray<{ value: ObjectiveRequest; label: string; detail: string }> = [
  {
    value: "max-parameter",
    label: "Largest value that still holds",
    detail: "max sustainable withdrawal",
  },
  {
    value: "min-parameter",
    label: "Smallest value that still holds",
    detail: "earliest retirement",
  },
  { value: "max-median-net-worth", label: "Most median terminal wealth", detail: "P50" },
  { value: "max-floor-net-worth", label: "Best floor", detail: "P5 terminal wealth" },
];

/**
 * Solve: goal seek as its own flow.
 *
 * Objective, constraint, what to vary, then the answer and the search that
 * found it. The method is not a control — one parameter whose own value is the
 * objective bisects, anything else searches a grid — so it is reported rather
 * than chosen.
 */
export function SolvePanel({
  scenarioId,
  parameters,
  /**
   * The parameter to open on — the one Sweep handed over, when it did.
   *
   * Read once, at mount: the screen above remounts this panel when it hands
   * over a different one, which is what keeps the initial selection out of an
   * effect that would fight the user's own edits.
   */
  initialParameterId,
}: {
  scenarioId: number;
  parameters: AnalysisParameter[];
  initialParameterId?: string;
}) {
  const solve = useAnalysis(scenarioId, "solve");

  const [vary, setVary] = useState<Axis[]>(() => {
    const seed =
      parameters.find((p) => p.id === initialParameterId) ?? parameters[0];
    return seed ? [axisFor(seed)] : [];
  });
  const [objective, setObjective] = useState<ObjectiveRequest>("max-parameter");
  const [constraint, setConstraint] = useState<ConstraintRequest>("funding-success-rate");
  const [minValue, setMinValue] = useState(0.95);
  const [iterations, setIterations] = useState(250);

  const varied = useMemo(
    () =>
      vary
        .map((axis) => parameters.find((p) => p.id === axis.parameterId))
        .filter((p): p is AnalysisParameter => p != null),
    [vary, parameters],
  );

  // The same rule the server applies, stated here so the screen can say what
  // will happen before it happens.
  const bisects = vary.length === 1 && varied[0]?.kind !== "date" && varied[0]?.kind !== "age"
    && (objective === "max-parameter" || objective === "min-parameter");

  const run = useCallback(() => {
    if (vary.length === 0) return;
    void solve.start({
      kind: "solve",
      objective,
      constraint,
      min_value: minValue,
      iterations,
      vary: vary.map((axis) => ({
        parameter_id: axis.parameterId,
        min: axis.min,
        max: axis.max,
        steps: axis.steps,
      })),
    });
  }, [solve, vary, objective, constraint, minValue, iterations]);

  const sidebar = (
    <aside
      style={{
        padding: "16px 18px 18px",
        borderRight: "1px solid var(--color-divider)",
        display: "flex",
        flexDirection: "column",
        gap: 14,
      }}
    >
      <Field label="Objective">
        <Dropdown
          ariaLabel="Objective"
          options={OBJECTIVES.map((o) => ({
            value: o.value,
            label: o.label,
            detail: o.detail,
          }))}
          value={objective}
          onChange={setObjective}
          disabled={solve.active}
        />
      </Field>

      <Field label="Outcome to constrain">
        <Dropdown ariaLabel="Outcome to constrain" value={constraint} onChange={setConstraint}
          disabled={solve.active} options={[
            { value: "funding-success-rate", label: "Cash funding check" },
            { value: "success-rate", label: "Positive ending net worth" },
          ]} />
      </Field>
      <Field label="Subject to">
        <div style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 13 }}>
          rate ≥
          <NumberInput
            aria-label="Minimum selected outcome rate, in percent"
            value={Math.round(minValue * 1000) / 10}
            decimals={1}
            min={0}
            max={100}
            suffix="%"
            onCommit={(percent) => setMinValue(percent / 100)}
            disabled={solve.active}
            style={{ width: 92, minHeight: 30 }}
          />
        </div>
      </Field>

      <div>
        <h6 style={{ margin: "0 0 6px" }}>Vary</h6>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {vary.map((axis, i) => (
            <div key={axis.parameterId} style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <div style={{ flex: 1, minWidth: 0 }}>
                <AxisPicker
                  slot={i === 0 ? "vary" : "and"}
                  parameters={parameters}
                  value={axis}
                  onChange={(next) =>
                    setVary((current) => current.map((a, j) => (j === i ? next : a)))
                  }
                  taken={vary.filter((_, j) => j !== i).map((a) => a.parameterId)}
                  showSteps={!bisects}
                  disabled={solve.active}
                />
              </div>
              {vary.length > 1 && (
                <Button
                  variant="ghost"
                  aria-label={`Stop varying ${paramId(varied[i] ?? { name: "" })}`}
                  disabled={solve.active}
                  onClick={() => setVary((current) => current.filter((_, j) => j !== i))}
                >
                  ×
                </Button>
              )}
            </div>
          ))}
        </div>
        {vary.length < MAX_VARIED && vary.length < parameters.length && (
          <Button
            variant="ghost"
            className="mt-[8px]"
            disabled={solve.active}
            onClick={() => {
              const free = parameters.find((p) => !vary.some((a) => a.parameterId === p.id));
              if (free) setVary((current) => [...current, axisFor(free, 5)]);
            }}
          >
            Vary another
          </Button>
        )}
        <p
          style={{
            fontSize: 11.5,
            margin: "10px 0 0",
            color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
            textWrap: "pretty",
          }}
        >
          {bisects
            ? "Bisection assumes the selected outcome changes monotonically across this range. The search stops at one thousandth of the range or after 16 probes; simulation uncertainty remains. Use a sweep to check for multiple feasible regions."
            : "Calendar inputs, multiple parameters, and outcome objectives use grid search. Each point is tested, and the best feasible point is selected. Dates use whole days and ages use whole months."}
        </p>
      </div>

      <div style={{ display: "flex", gap: 10, alignItems: "flex-end", marginTop: "auto" }}>
        <Field label="Iterations a probe" style={{ width: 120 }}>
          <NumberInput
            aria-label="Monte Carlo iterations per probe"
            value={iterations}
            decimals={0}
            min={25}
            max={2000}
            onCommit={setIterations}
            disabled={solve.active}
            style={{ minHeight: 30 }}
          />
        </Field>
        <Button
          variant="primary"
          style={{ flex: 1 }}
          onClick={run}
          disabled={solve.active || vary.length === 0}
        >
          Solve
        </Button>
      </div>
    </aside>
  );

  return (
    <div style={{ display: "grid", gridTemplateColumns: "340px 1fr", alignItems: "stretch" }}>
      {sidebar}
      <div style={{ minWidth: 0 }}>
        {solve.active ? (
          <JobProgress job={solve.job} onCancel={solve.cancel} label="Solving" />
        ) : solve.results ? (
          <Answer outcome={solve.results} elapsedMs={solve.job?.elapsed_ms ?? null} />
        ) : (
          <div style={{ padding: "34px 24px 40px", maxWidth: 520 }}>
            <h4 style={{ margin: "0 0 6px" }}>Nothing solved yet</h4>
            <p
              style={{
                margin: 0,
                fontSize: 13,
                lineHeight: 1.5,
                color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
              }}
            >
              A sweep reads an answer off a grid, to the nearest cell. Solving
              searches for a feasible value: pick what to optimise, the bar it has to clear,
              and what is allowed to move.
            </p>
            {solve.error && (
              <p role="alert" style={{ fontSize: 12.5, marginTop: 12 }}>
                The solve did not finish: {solve.error}
              </p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function Answer({
  outcome,
  elapsedMs,
}: {
  outcome: SolveOutcome & { kind: "solve" };
  elapsedMs: number | null;
}) {
  const rows = solveRows(outcome);
  const parameter = solvedParameter(outcome);
  const simulations = outcome.steps.length * outcome.iterations;
  const error = outcome.std_error;
  // The range actually probed, which is the request's range rather than the
  // parameter's own suggestion.
  const values = outcome.steps.map((step) => step.values[0]).filter((v) => v != null);
  const searched =
    parameter && values.length > 0
      ? `${paramValue(parameter.kind, Math.min(...values))} – ${paramValue(parameter.kind, Math.max(...values))}`
      : undefined;

  return (
    <div
      style={{
        padding: "16px 18px 18px",
        display: "flex",
        flexDirection: "column",
        gap: 14,
      }}
    >
      <div>
        <div className="stat-l">
          {parameter ? paramId(parameter) : "best combination"} ·{" "}
          {outcome.method === "bisection" ? "bisection" : "grid search"} ·{" "}
          {outcome.steps.length} probes
        </div>
        <div
          style={{
            fontFamily: "var(--font-heading)",
            fontWeight: 600,
            fontSize: 34,
            lineHeight: 1.05,
            color: outcome.best
              ? "var(--color-accent-800)"
              : "color-mix(in srgb, var(--color-text) 50%, transparent)",
          }}
        >
          {solveHeadline(outcome)}
        </div>
        {!outcome.best && (
          <p style={{ fontSize: 12.5, margin: "6px 0 0", maxWidth: 560 }}>
            No tested point clears the constraint. Bisection assumes one monotone boundary; a sweep can check the interior. Widen the range,
            lower the bar, or let another parameter move.
          </p>
        )}
      </div>

      <Table compact>
        <thead>
          <tr>
            <Th />
            <Th align="right">Plan</Th>
            <Th align="right">Solved</Th>
            <Th align="right">Δ</Th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.label}>
              <Td
                style={
                  row.mono
                    ? { fontFamily: "ui-monospace, Menlo, monospace", fontSize: 12 }
                    : undefined
                }
              >
                {row.label}
              </Td>
              <Td align="right">{row.plan}</Td>
              <Td align="right" style={{ fontWeight: 500 }}>
                {row.best}
              </Td>
              <Td align="right" muted>
                {row.delta}
              </Td>
            </tr>
          ))}
        </tbody>
      </Table>

      {outcome.method === "bisection" && (
        <div>
          <h6 style={{ margin: "0 0 6px" }}>
            Convergence{" "}
            <span className="text-muted" style={{ letterSpacing: 0 }}>
              the bracket, one probe at a time
            </span>
          </h6>
          <ConvergenceChart steps={outcome.steps} parameter={parameter} constraint={outcome.constraint} />
        </div>
      )}

      {outcome.best && error != null && (
        <Blueprint
          style={{ padding: "10px 12px", display: "flex", gap: 12, alignItems: "center", fontSize: 12 }}
        >
          <span style={{ textWrap: "pretty" }}>
            At {fmtInt(outcome.iterations)} iterations per probe, {outcome.constraint === "funding-success-rate" ? "cash funding" : "positive ending net worth"}
            at the answer is {fmtPercent((outcome.constraint === "funding-success-rate" ? outcome.best.funding_success_rate : outcome.best.success_rate) ?? NaN)}. The estimated standard error is {(error * 100).toFixed(1)} percentage points (one standard error, not a confidence guarantee).
            {error > 0.02
              ? " That is wider than the last digit of the answer — raise the iterations before trusting it."
              : " Search resolution does not remove simulation uncertainty."}
          </span>
        </Blueprint>
      )}

      <p
        style={{
          fontSize: 11,
          margin: 0,
          color: "color-mix(in srgb, var(--color-text) 50%, transparent)",
        }}
      >
        {outcome.steps.length} probes × {fmtInt(outcome.iterations)} iterations ·{" "}
        {fmtInt(simulations + outcome.iterations)} simulations including baseline · fixed seed 24301 reused across probes
        {elapsedMs != null && ` · ${(elapsedMs / 1000).toFixed(1)}s`}
        {parameter && searched && ` · searched ${searched}`}
      </p>
    </div>
  );
}
