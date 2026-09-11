"use client";

import { useCallback, useMemo, useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  Blueprint,
  Button,
  Field,
  InlineStat,
  NumberInput,
  StatLabel,
  Table,
  Td,
  Th,
} from "@/components/ui";
import { fmtCompact, fmtInt, fmtPercent } from "@/lib/format";
import type { AnalysisParameter } from "@/lib/api/types";
import { useAnalysis } from "@/lib/hooks/useAnalysis";
import {
  frontierRows,
  paramId,
  paramValue,
  sliceAlongX,
  sliceAlongY,
  sweepView,
} from "@/lib/view/analysis";
import { AxisPicker, axisFor, type Axis } from "./AxisPicker";
import { JobProgress } from "./JobProgress";
import { SensitivityPanel } from "./SensitivityPanel";
import { SliceChart } from "./SliceChart";
import { SweepGrid, SweepLegend, type Focus } from "./SweepGrid";

/** Where a fresh threshold field starts, as a fraction. */
const DEFAULT_THRESHOLD = 0.95;

/**
 * Sweep: the grid, the threshold drawn on it, and everything read off it.
 *
 * Only a new range or a new axis costs a run. The threshold, the frontier, the
 * frontier table and both slice charts are derived from the finished grid, so
 * moving the safety bar is instant and the footer's "last run" stays honest.
 */
export function SweepPanel({
  scenarioId,
  parameters,
  onSolveFor,
}: {
  scenarioId: number;
  parameters: AnalysisParameter[];
  /** Hand a parameter to Solve, which is where a grid usually points next. */
  onSolveFor: (parameterId: string) => void;
}) {
  const sweep = useAnalysis(scenarioId, "sweep");
  const sensitivity = useAnalysis(scenarioId, "sensitivity");

  // The axes are derived until they are touched, so the screen opens on the
  // plan's first two parameters without an effect writing them into state.
  // `null` is the third state a plain `undefined` cannot say: an axis the user
  // took away, which must not spring back from the default.
  const [xPick, setXAxis] = useState<Axis>();
  const [yPick, setYAxis] = useState<Axis | null>();
  const xAxis = xPick ?? (parameters[0] ? axisFor(parameters[0]) : undefined);
  const yAxis =
    yPick === undefined ? (parameters[1] ? axisFor(parameters[1]) : undefined) : (yPick ?? undefined);

  const [threshold, setThreshold] = useState(DEFAULT_THRESHOLD);
  const [hover, setHover] = useState<Focus>();
  const [pinned, setPinned] = useState<Focus>();

  const run = useCallback(() => {
    if (!xAxis) return;
    setPinned(undefined);
    setHover(undefined);
    void sweep.start({
      kind: "sweep",
      axes: [xAxis, yAxis]
        .filter((axis): axis is Axis => axis != null)
        .map((axis) => ({
          parameter_id: axis.parameterId,
          min: axis.min,
          max: axis.max,
          steps: axis.steps,
        })),
    });
  }, [sweep, xAxis, yAxis]);

  const rank = useCallback(() => {
    void sensitivity.start({ kind: "sensitivity", parameter_ids: [], fraction: 0.2 });
  }, [sensitivity]);

  const view = useMemo(
    () => (sweep.results ? sweepView(sweep.results, threshold) : undefined),
    [sweep.results, threshold],
  );

  const pickAxis = useCallback(
    (slot: "x" | "y", parameterId: string) => {
      const parameter = parameters.find((p) => p.id === parameterId);
      if (!parameter) return;
      const axis = axisFor(parameter);
      if (slot === "x") {
        setXAxis(axis);
        // The same parameter cannot hold both axes; the one it just left is
        // cleared rather than silently duplicated.
        if (yAxis?.parameterId === parameterId) setYAxis(null);
      } else {
        setYAxis(axis);
        if (xAxis?.parameterId === parameterId && parameters[0]) {
          setXAxis(axisFor(parameters[0]));
        }
      }
    },
    [parameters, xAxis, yAxis],
  );

  const toolbar = (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 20px",
        borderBottom: "1px solid var(--color-divider)",
        flexWrap: "wrap",
      }}
    >
      {xAxis && (
        <AxisPicker
          slot="x"
          parameters={parameters}
          value={xAxis}
          onChange={setXAxis}
          taken={yAxis ? [yAxis.parameterId] : []}
          disabled={sweep.active}
        />
      )}
      {yAxis ? (
        <AxisPicker
          slot="y"
          parameters={parameters}
          value={yAxis}
          onChange={setYAxis}
          taken={xAxis ? [xAxis.parameterId] : []}
          disabled={sweep.active}
        />
      ) : (
        parameters.length > 1 && (
          <Button
            variant="ghost"
            disabled={sweep.active}
            onClick={() => {
              const free = parameters.find((p) => p.id !== xAxis?.parameterId);
              if (free) setYAxis(axisFor(free));
            }}
          >
            Add a second axis
          </Button>
        )
      )}
      {xAxis && yAxis && (
        <Button
          variant="ghost"
          disabled={sweep.active}
          onClick={() => {
            setXAxis(yAxis);
            setYAxis(xAxis ?? null);
            setPinned(undefined);
          }}
        >
          Swap axes
        </Button>
      )}

      <div style={{ marginLeft: "auto", display: "flex", gap: 10, alignItems: "flex-end" }}>
        <Field label="Safe when success ≥" style={{ width: 150 }}>
          <NumberInput
            aria-label="Safety threshold, in percent"
            value={Math.round(threshold * 100)}
            decimals={0}
            min={0}
            max={100}
            suffix="%"
            onCommit={(percent) => setThreshold(percent / 100)}
            style={{ minHeight: 30 }}
          />
        </Field>
        <Button variant="primary" onClick={run} disabled={sweep.active || !xAxis}>
          Run analysis
        </Button>
      </div>
    </div>
  );

  if (sweep.active) {
    return (
      <>
        {toolbar}
        <JobProgress job={sweep.job} onCancel={sweep.cancel} label="Sweeping" />
      </>
    );
  }

  if (!view) {
    // No grid yet. The ranking is the honest thing to offer first: it costs a
    // fraction of a sweep and it says which axes are worth the sweep.
    return (
      <>
        {toolbar}
        {sweep.error && <Problem message={sweep.error} />}
        <Empty
          sensitivity={sensitivity}
          parameters={parameters}
          axes={{ x: xAxis?.parameterId, y: yAxis?.parameterId }}
          onPickAxis={pickAxis}
          onRank={rank}
          onSweep={run}
        />
      </>
    );
  }

  const focus = hover ?? pinned ?? view.planCell ?? { x: 0, y: 0 };
  const focused = view.at(focus.x, focus.y);
  const rows = frontierRows(view);
  const sliceX = sliceAlongX(view, focus.y);
  const sliceY = sliceAlongY(view, focus.x);
  const simulations = view.cells.length * view.iterations;

  return (
    <>
      {toolbar}
      <SplitPane
        railWidth={320}
        main={
          <div style={{ padding: "16px 18px 18px" }}>
            <div
              style={{
                display: "flex",
                alignItems: "baseline",
                justifyContent: "space-between",
                marginBottom: 6,
              }}
            >
              <h4 style={{ margin: 0 }}>
                Success rate{" "}
                <span className="text-muted" style={{ fontSize: 12, letterSpacing: 0 }}>
                  {view.yAxis ? `${view.yAxis.label} × ${view.xAxis.label}` : view.xAxis.label}
                </span>
              </h4>
              <span
                style={{
                  fontSize: 11,
                  color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
                }}
              >
                hover reads · click pins
              </span>
            </div>

            <SweepGrid
              view={view}
              hover={hover}
              onHover={setHover}
              pinned={pinned}
              onPin={(next) =>
                setPinned((current) =>
                  current && current.x === next.x && current.y === next.y ? undefined : next,
                )
              }
            />
            <SweepLegend view={view} />

            {view.yAxis && (
              <>
                <h6 style={{ margin: "18px 0 6px" }}>
                  {view.fallsWithY ? "Highest" : "Lowest"} safe {view.yAxis.label}{" "}
                  <span className="text-muted" style={{ letterSpacing: 0 }}>
                    clearing {fmtPercent(threshold, 0)}, by {view.xAxis.label}
                  </span>
                </h6>
                <Table compact>
                  <thead>
                    <tr>
                      <Th>{view.xAxis.role}</Th>
                      <Th align="right">Safe {view.yAxis.role}</Th>
                      <Th align="right">Success</Th>
                      <Th align="right">P50 terminal</Th>
                      <Th align="right">vs plan</Th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((row) => (
                      <tr key={row.x}>
                        <Td style={{ fontFamily: "var(--font-heading)", fontWeight: 600 }}>
                          {paramValue(view.xAxis.kind, row.xValue)}
                        </Td>
                        <Td align="right" style={{ fontWeight: 500 }}>
                          {row.yValue == null
                            ? "nothing clears it"
                            : paramValue(view.yAxis!.kind, row.yValue)}
                        </Td>
                        <Td align="right">
                          {row.point ? fmtPercent(row.point.success_rate) : "—"}
                        </Td>
                        <Td align="right">{row.point ? fmtCompact(row.point.p50) : "—"}</Td>
                        <Td align="right" muted>
                          {row.delta == null
                            ? "—"
                            : row.delta === 0
                              ? "as planned"
                              : (row.delta > 0 ? "+" : "−") +
                                paramValue(view.yAxis!.kind, Math.abs(row.delta))}
                        </Td>
                      </tr>
                    ))}
                  </tbody>
                </Table>
              </>
            )}
          </div>
        }
        rail={
          <div
            style={{
              padding: "16px 16px 18px",
              display: "flex",
              flexDirection: "column",
              gap: 16,
            }}
          >
            <Blueprint style={{ padding: "12px 14px" }}>
              <StatLabel>{hover ? "under the pointer" : pinned ? "pinned cell" : "the plan"}</StatLabel>
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "1fr 1fr",
                  gap: "10px 14px",
                  marginTop: 8,
                }}
              >
                <InlineStat
                  label={view.xAxis.role}
                  value={paramValue(view.xAxis.kind, view.xAxis.values[focus.x])}
                />
                {view.yAxis && (
                  <InlineStat
                    label={view.yAxis.role}
                    value={paramValue(view.yAxis.kind, view.yAxis.values[focus.y])}
                  />
                )}
                <InlineStat
                  label="success"
                  emphasis
                  value={focused ? fmtPercent(focused.point.success_rate) : "—"}
                />
                <InlineStat
                  label="P50 terminal"
                  value={focused ? fmtCompact(focused.point.p50) : "—"}
                />
              </div>
              <p
                style={{
                  fontSize: 11.5,
                  margin: "10px 0 0",
                  color: "color-mix(in srgb, var(--color-text) 60%, transparent)",
                  textWrap: "pretty",
                }}
              >
                {focused
                  ? describe(focused.point.success_rate, view.plan.success_rate, threshold)
                  : "This combination was not measured."}
              </p>
              {view.yAxis && (
                <Button
                  block
                  variant="secondary"
                  className="mt-[10px]"
                  title={`Goal seek ${view.yAxis.label}`}
                  onClick={() => onSolveFor(view.yAxis!.parameter_id)}
                >
                  Solve this axis exactly
                </Button>
              )}
            </Blueprint>

            <div>
              <h6 style={{ margin: "0 0 6px" }}>
                Success vs {view.xAxis.role}{" "}
                {view.yAxis && (
                  <span className="text-muted" style={{ letterSpacing: 0 }}>
                    at {paramValue(view.yAxis.kind, view.yAxis.values[focus.y])}
                  </span>
                )}
              </h6>
              <SliceChart slice={sliceX} threshold={threshold} marked={focus.x} />
            </div>

            {sliceY && view.yAxis && (
              <div>
                <h6 style={{ margin: "0 0 6px" }}>
                  Success vs {view.yAxis.role}{" "}
                  <span className="text-muted" style={{ letterSpacing: 0 }}>
                    at {paramValue(view.xAxis.kind, view.xAxis.values[focus.x])}
                  </span>
                </h6>
                <SliceChart slice={sliceY} threshold={threshold} marked={focus.y} />
              </div>
            )}

            <p
              style={{
                fontSize: 11.5,
                margin: "auto 0 0",
                color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
                textWrap: "pretty",
              }}
            >
              The threshold and the axis swap re-read the finished sweep. Only a
              new range or a new parameter needs another run.
            </p>
          </div>
        }
      />
      <div
        style={{
          padding: "8px 20px",
          borderTop: "1px solid var(--color-divider)",
          fontSize: 11,
          color: "color-mix(in srgb, var(--color-text) 50%, transparent)",
        }}
      >
        {view.cells.length} points × {fmtInt(view.iterations)} iterations ·{" "}
        {fmtInt(simulations)} simulations
        {sweep.job?.elapsed_ms != null && ` · ${(sweep.job.elapsed_ms / 1000).toFixed(1)}s`}
      </div>
    </>
  );
}

/** What the focused cell means, relative to the plan and the threshold. */
function describe(rate: number, plan: number, threshold: number): string {
  const delta = Math.round((rate - plan) * 1000) / 10;
  const move =
    Math.abs(delta) < 0.1
      ? "the same success as the plan"
      : `${Math.abs(delta).toFixed(1)} points ${delta > 0 ? "above" : "below"} the plan`;
  return `${rate >= threshold ? "Clears" : "Misses"} the threshold — ${move}.`;
}

function Problem({ message }: { message: string }) {
  return (
    <p role="alert" style={{ padding: "10px 20px", margin: 0, fontSize: 12.5 }}>
      The analysis did not finish: {message}
    </p>
  );
}

/** No grid yet: offer the cheap ranking that says which axes deserve one. */
function Empty({
  sensitivity,
  parameters,
  axes,
  onPickAxis,
  onRank,
  onSweep,
}: {
  sensitivity: ReturnType<typeof useAnalysis<"sensitivity">>;
  parameters: AnalysisParameter[];
  axes: { x: string | undefined; y: string | undefined };
  onPickAxis: (slot: "x" | "y", parameterId: string) => void;
  onRank: () => void;
  onSweep: () => void;
}) {
  if (sensitivity.active) {
    return <JobProgress job={sensitivity.job} onCancel={sensitivity.cancel} label="Ranking" />;
  }
  if (sensitivity.results) {
    return (
      <SensitivityPanel
        results={sensitivity.results}
        parameters={parameters}
        axes={axes}
        onPickAxis={onPickAxis}
        onSweep={onSweep}
      />
    );
  }

  return (
    <div style={{ padding: "34px 24px 40px", maxWidth: 560 }}>
      <h4 style={{ margin: "0 0 6px" }}>Nothing swept yet</h4>
      <p
        style={{
          margin: "0 0 14px",
          fontSize: 13,
          lineHeight: 1.5,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        A sweep runs the plan once for every combination of the axes above and
        colours the grid by how often it survives. Ranking first is cheaper: two
        runs a parameter, and it says which two are worth the axes.
      </p>
      {sensitivity.error && (
        <p role="alert" style={{ fontSize: 12.5 }}>
          The ranking did not finish: {sensitivity.error}
        </p>
      )}
      <div style={{ display: "flex", gap: 8 }}>
        <Button variant="primary" onClick={onRank}>
          Rank the parameters
        </Button>
        <Button onClick={onSweep} disabled={axes.x == null}>
          Sweep {axes.x ? paramIdOf(parameters, axes.x) : ""}
          {axes.y ? ` × ${paramIdOf(parameters, axes.y)}` : ""}
        </Button>
      </div>
    </div>
  );
}

function paramIdOf(parameters: AnalysisParameter[], id: string): string {
  const parameter = parameters.find((p) => p.id === id);
  return parameter ? paramId(parameter) : id;
}
