"use client";

import { useCallback, useMemo, useState } from "react";
import { SplitPane } from "@/components/layout";
import { Button } from "@/components/ui";
import { fmtClock, fmtInt } from "@/lib/format";
import type { AnalysisParameter } from "@/lib/api/types";
import { useAnalysis, useCachedSweep, useSweepLayout } from "@/lib/hooks/useAnalysis";
import { paramId } from "@/lib/view/analysis";
import {
  defaultGraphs,
  graphView,
  newGraph,
  parseLayout,
  reconcile,
  sweepCsv,
  sweepSpace,
  type GraphSpec,
} from "@/lib/view/sweep";
import { GraphCard, GraphGap } from "./GraphCard";
import { GraphInspector } from "./GraphInspector";
import { JobProgress } from "./JobProgress";
import { SensitivityPanel } from "./SensitivityPanel";
import {
  MAX_ITERATIONS,
  MIN_ITERATIONS,
  VariableStrip,
  combinations,
  variableFor,
  type SweptVariable,
} from "./VariableStrip";

/** How many variables a plan's parameters are seeded into, and at what resolution. */
const SEED_VARIABLES = 2;
const SEED_STEPS = 6;

/**
 * Iterations behind each combination, to start.
 *
 * Deliberately lighter than a Results run: a sweep pays this over every cell,
 * and the shape of a grid reads long before its individual cells are precise.
 */
const SEED_ITERATIONS = 250;

/** The server's own ceiling on a grid, restated so the strip can warn before a 400. */
const MAX_POINTS = 512;

/**
 * Sweep as a workspace.
 *
 * The sweep is one thing and the graphs are another. Every combination of every
 * swept variable is evaluated once, and then any number of graphs read that one
 * result — each picking its own kind, its own axes out of the swept set and its
 * own dependent measure. Nothing in the layout costs a run; only the variables
 * and their ranges do, which is why they are the one part of the screen folded
 * away above everything else.
 */
export function SweepPanel({
  scenarioId,
  parameters,
  onSolveFor,
}: {
  scenarioId: number;
  parameters: AnalysisParameter[];
  /** Hand a parameter to Solve, which is where a sweep usually points next. */
  onSolveFor: (parameterId: string) => void;
}) {
  const sweep = useAnalysis(scenarioId, "sweep");
  const sensitivity = useAnalysis(scenarioId, "sensitivity");
  // The last sweep the server kept, which is what a reloaded page opens on.
  const restored = useCachedSweep(scenarioId);

  /**
   * What the strip offers before anyone touches it.
   *
   * The restored grid where there is one, so a reload lands on the variables
   * that produced the graphs on screen rather than on the plan's first two —
   * pressing Run on a strip that disagrees with the graphs below it would
   * silently answer a different question. Ranges and resolution come off the
   * axes themselves, which carry every value they were stepped over.
   */
  const seeded = useMemo<SweptVariable[]>(() => {
    const axes = (restored.results?.axes ?? []).filter((axis) =>
      parameters.some((p) => p.id === axis.parameter_id),
    );
    if (axes.length > 0) {
      return axes.map((axis) => ({
        parameterId: axis.parameter_id,
        min: axis.values[0],
        max: axis.values[axis.values.length - 1],
        steps: axis.values.length,
        enabled: true,
      }));
    }
    return parameters.slice(0, SEED_VARIABLES).map((p) => variableFor(p, SEED_STEPS));
  }, [restored.results, parameters]);

  // Edits win over the seed; before there are any, the seed is the strip. Held
  // this way rather than copied into state by an effect, which would race the
  // restore and flash the default over it.
  const [edited, setEdited] = useState<SweptVariable[]>();
  const variables = edited ?? seeded;

  const [chosenIterations, setChosenIterations] = useState<number>();
  const iterations = chosenIterations ?? restored.results?.iterations ?? SEED_ITERATIONS;

  // Open until a sweep has run, and a restored one counts: the ranges are what
  // the strip is for, and there is nothing to decide once a grid is on screen.
  const [openedStrip, setOpenedStrip] = useState<boolean>();
  const varsOpen = openedStrip ?? restored.results == null;

  const [ranWith, setRanWith] = useState<string>();
  const [ranAt, setRanAt] = useState<number>();

  // The arrangement of graphs over the grid, restored with it and stored as it
  // is edited: a workspace built card by card is work, and reopening the tab on
  // a default layout would throw it away as surely as losing the sweep did.
  const storedLayout = useMemo(() => parseLayout(restored.layout), [restored.layout]);
  const { layout, setLayout } = useSweepLayout(scenarioId, storedLayout);
  const [selected, setSelected] = useState<string>();

  const enabledCount = variables.filter((v) => v.enabled).length;
  const points = combinations(variables);
  const signature = [
    String(iterations),
    ...variables
      .filter((v) => v.enabled)
      .map((v) => `${v.parameterId}:${v.min}:${v.max}:${v.steps}`),
  ].join("|");

  // This session's answer where there is one, and the stored one otherwise. A
  // job that exists but has produced nothing — running, failed, canceled —
  // speaks for itself: the restored grid would read as its result.
  const results = sweep.job ? sweep.results : restored.results;
  const space = useMemo(() => (results ? sweepSpace(results) : undefined), [results]);

  // A finished sweep decides what the layout can draw, so the graphs on screen
  // are the edited layout resolved against it rather than a second copy kept in
  // step by an effect. Graphs are carried where their variables survived and
  // moved where they did not, which is what lets adding a variable and
  // re-running extend the screen instead of clearing it.
  const graphs = useMemo(
    () => (space ? reconcile(layout ?? [], space.axes) : []),
    [space, layout],
  );

  const run = useCallback(() => {
    const enabled = variables.filter((v) => v.enabled);
    if (enabled.length === 0) return;
    setRanWith(signature);
    setRanAt(Date.now());
    setOpenedStrip(false);
    void sweep.start({
      kind: "sweep",
      iterations,
      axes: enabled.map((v) => ({
        parameter_id: v.parameterId,
        min: v.min,
        max: v.max,
        steps: v.steps,
      })),
    });
  }, [variables, iterations, signature, sweep]);

  const rank = useCallback(() => {
    void sensitivity.start({ kind: "sensitivity", parameter_ids: [], fraction: 0.2 });
  }, [sensitivity]);

  const editVariable = useCallback(
    (parameterId: string, next: SweptVariable) => {
      setEdited(variables.map((v) => (v.parameterId === parameterId ? next : v)));
    },
    [variables],
  );

  const addVariable = useCallback(() => {
    const free = parameters.find((p) => !variables.some((v) => v.parameterId === p.id));
    if (free) setEdited([...variables, variableFor(free, stepsFor(variables.length + 1))]);
  }, [parameters, variables]);

  const removeVariable = useCallback(
    (parameterId: string) => setEdited(variables.filter((v) => v.parameterId !== parameterId)),
    [variables],
  );

  /**
   * Point a row at a different parameter.
   *
   * The new parameter brings its own range: the old bounds are meaningless on
   * a different quantity, and keeping them silently is how a sweep ends up
   * over ages 3,000 to 12,000.
   */
  const retargetVariable = useCallback(
    (parameterId: string, next: string) => {
      const parameter = parameters.find((p) => p.id === next);
      if (!parameter) return;
      setEdited(
        variables.map((v) =>
          v.parameterId === parameterId
            ? { ...variableFor(parameter, v.steps), enabled: v.enabled }
            : v,
        ),
      );
    },
    [parameters, variables],
  );

  /** Include or drop a parameter from the swept set, from the ranking's rows. */
  const toggleVariable = useCallback(
    (parameterId: string) => {
      if (variables.some((v) => v.parameterId === parameterId)) {
        setEdited(variables.filter((v) => v.parameterId !== parameterId));
        return;
      }
      const parameter = parameters.find((p) => p.id === parameterId);
      if (parameter) {
        setEdited([...variables, variableFor(parameter, stepsFor(variables.length + 1))]);
      }
    },
    [parameters, variables],
  );

  const exportCsv = useCallback(() => {
    if (!space) return;
    const blob = new Blob([sweepCsv(space)], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `sweep-${new Date().toISOString().slice(0, 10)}.csv`;
    link.click();
    URL.revokeObjectURL(url);
  }, [space]);

  const over = points > MAX_POINTS;
  // A restored grid keeps the clock of the sweep that produced it, so the
  // footer never dates someone else's answer to this session.
  const lastRunAt = ranAt ?? restored.at;
  const toolbar = (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 14,
        padding: "10px 20px",
        borderBottom: "1px solid var(--color-divider)",
        flexWrap: "wrap",
      }}
    >
      <span
        style={{
          fontSize: 12,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        {space
          ? `Last run ${lastRunAt ? fmtWhen(lastRunAt) : "—"} · ${fmtInt(space.points)} points × ` +
            `${fmtInt(space.iterations)} iterations` +
            (sweep.job?.elapsed_ms != null ? ` · ${(sweep.job.elapsed_ms / 1000).toFixed(1)}s` : "")
          : "Every combination is simulated once; the graphs below read the result."}
      </span>
      <div style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
        {over && (
          <span style={{ fontSize: 11.5 }} role="alert">
            {fmtInt(points)} combinations is past the {fmtInt(MAX_POINTS)} a sweep will
            evaluate — drop a variable or cut its steps.
          </span>
        )}
        <Button variant="ghost" disabled={!space} onClick={exportCsv}>
          Export CSV
        </Button>
        <Button
          variant="primary"
          shortcut="r"
          onClick={run}
          disabled={sweep.active || enabledCount === 0 || over}
        >
          Run sweep
        </Button>
      </div>
    </div>
  );

  const strip = (
    <VariableStrip
      parameters={parameters}
      variables={variables}
      open={varsOpen}
      onToggleOpen={() => setOpenedStrip(!varsOpen)}
      onChange={editVariable}
      onRetarget={retargetVariable}
      onAdd={addVariable}
      onRemove={removeVariable}
      iterations={iterations}
      onIterations={(next) =>
        setChosenIterations(Math.min(MAX_ITERATIONS, Math.max(MIN_ITERATIONS, next)))
      }
      dirty={ranWith != null && ranWith !== signature}
      disabled={sweep.active}
    />
  );

  if (sweep.active) {
    return (
      <>
        {toolbar}
        {strip}
        <JobProgress job={sweep.job} onCancel={sweep.cancel} label="Sweeping" />
      </>
    );
  }

  if (!space) {
    return (
      <>
        {toolbar}
        {strip}
        {sweep.error && <Problem message={sweep.error} />}
        {restored.loading ? (
          // The stored grid is one request away; offering to run a sweep before
          // it lands would flash the empty state over an answer that exists.
          <p style={{ padding: "10px 20px", margin: 0, fontSize: 12.5 }}>Loading the last sweep…</p>
        ) : (
          <Empty
            sensitivity={sensitivity}
            parameters={parameters}
            swept={new Set(variables.filter((v) => v.enabled).map((v) => v.parameterId))}
            onToggle={toggleVariable}
            onRank={rank}
            onSweep={run}
            canSweep={enabledCount > 0 && !over}
          />
        )}
      </>
    );
  }

  const current = graphs.find((g) => g.id === selected) ?? graphs[0];
  const currentView = current ? graphView(space, current) : undefined;
  const update = (next: GraphSpec) =>
    setLayout(graphs.map((g) => (g.id === next.id ? next : g)));

  return (
    <>
      {toolbar}
      {strip}
      <SplitPane
        railWidth={272}
        main={
          <div style={{ padding: "14px 18px 20px" }}>
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "1fr 1fr",
                gap: 14,
                alignItems: "start",
              }}
            >
              {graphs.map((spec, index) => {
                const view = graphView(space, spec);
                return view ? (
                  <GraphCard
                    key={spec.id}
                    view={view}
                    position={index + 1}
                    selected={spec.id === current?.id}
                    onSelect={() => setSelected(spec.id)}
                    onTurn={
                      spec.kind === "surface"
                        ? (azimuth, elevation) => update({ ...spec, azimuth, elevation })
                        : undefined
                    }
                  />
                ) : (
                  <GraphGap key={spec.id} spec={spec} position={index + 1} />
                );
              })}
              <button
                type="button"
                className="blueprint add-graph"
                onClick={() => {
                  const spec = newGraph(space.axes, graphs);
                  if (!spec) return;
                  setLayout([...graphs, spec]);
                  setSelected(spec.id);
                }}
              >
                <i className="corner tl" />
                <i className="corner tr" />
                <i className="corner bl" />
                <i className="corner br" />
                <span style={{ fontSize: 22, lineHeight: 1 }}>+</span> Add graph
              </button>
            </div>
          </div>
        }
        rail={
          currentView && current ? (
            <GraphInspector
              space={space}
              view={currentView}
              position={graphs.indexOf(current) + 1}
              count={graphs.length}
              onChange={update}
              onRemove={() => {
                setLayout(graphs.filter((g) => g.id !== current.id));
                setSelected(undefined);
              }}
              onReset={() => {
                setLayout(defaultGraphs(space.axes));
                setSelected(undefined);
              }}
              onSolveFor={onSolveFor}
            />
          ) : (
            <div style={{ padding: "14px 16px", fontSize: 12.5 }}>
              <p style={{ margin: 0 }}>
                Nothing selected. Click a graph to edit it, or add one.
              </p>
              <Button className="mt-[10px]" onClick={() => setLayout(defaultGraphs(space.axes))}>
                Reset layout
              </Button>
            </div>
          )
        }
      />
    </>
  );
}

/**
 * When a sweep was run.
 *
 * The clock alone for one run in this session, and the date as well for a grid
 * restored from the server — which may be days old, and would otherwise read as
 * having been run this afternoon.
 */
function fmtWhen(at: number): string {
  const then = new Date(at);
  if (then.toDateString() === new Date().toDateString()) return fmtClock(at);
  const day = then.toLocaleDateString("en-US", { month: "short", day: "numeric" });
  return `${day} ${fmtClock(at)}`;
}

/** More variables, fewer steps each: the budget is the product, not the count. */
function stepsFor(count: number): number {
  if (count <= 2) return SEED_STEPS;
  return count === 3 ? 4 : 3;
}

function Problem({ message }: { message: string }) {
  return (
    <p role="alert" style={{ padding: "10px 20px", margin: 0, fontSize: 12.5 }}>
      The sweep did not finish: {message}
    </p>
  );
}

/** No sweep yet: offer the cheap ranking that says which variables deserve one. */
function Empty({
  sensitivity,
  parameters,
  swept,
  onToggle,
  onRank,
  onSweep,
  canSweep,
}: {
  sensitivity: ReturnType<typeof useAnalysis<"sensitivity">>;
  parameters: AnalysisParameter[];
  swept: Set<string>;
  onToggle: (parameterId: string) => void;
  onRank: () => void;
  onSweep: () => void;
  canSweep: boolean;
}) {
  if (sensitivity.active) {
    return <JobProgress job={sensitivity.job} onCancel={sensitivity.cancel} label="Ranking" />;
  }
  if (sensitivity.results) {
    return (
      <SensitivityPanel
        results={sensitivity.results}
        parameters={parameters}
        swept={swept}
        onToggle={onToggle}
        onSweep={onSweep}
        canSweep={canSweep}
      />
    );
  }

  return (
    <div style={{ padding: "34px 24px 40px", maxWidth: 600 }}>
      <h4 style={{ margin: "0 0 6px" }}>Nothing swept yet</h4>
      <p
        style={{
          margin: "0 0 14px",
          fontSize: 13,
          lineHeight: 1.5,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        A sweep runs the plan once for every combination of the variables above,
        and then any number of graphs read that one result. Ranking first is
        cheaper: two runs a parameter, and it says which variables are worth
        sweeping at all.
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
        <Button onClick={onSweep} disabled={!canSweep}>
          Sweep {[...swept].map((id) => idOf(parameters, id)).join(" × ") || "nothing"}
        </Button>
      </div>
    </div>
  );
}

function idOf(parameters: AnalysisParameter[], id: string): string {
  const parameter = parameters.find((p) => p.id === id);
  return parameter ? paramId(parameter) : id;
}
