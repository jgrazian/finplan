"use client";

import { useCallback, useState } from "react";
import { SubTabBar } from "@/components/layout";
import { EmptyState } from "@/components/screens/EmptyState";
import type { SegmentOption } from "@/components/ui";
import { useParameters } from "@/lib/hooks/useAnalysis";
import { useNav } from "@/lib/nav";
import { SolvePanel } from "./SolvePanel";
import { SweepPanel } from "./SweepPanel";

type Mode = "whatif" | "sweep" | "solve";

/**
 * Analysis is three jobs, not one screen: What-if asks what happens if, Sweep
 * asks what happens across a range, Solve asks for the value that just works.
 * What-if is not built yet, and is disabled rather than dropped — the mode
 * switch is the shape of the tab, and a missing third makes the other two read
 * as the whole of it.
 */
const MODES: ReadonlyArray<SegmentOption<Mode>> = [
  {
    value: "whatif",
    label: "What-if",
    disabled: true,
    title: "Not built yet — overrides on sliders, against the plan ghosted behind them",
  },
  { value: "sweep", label: "Sweep" },
  { value: "solve", label: "Solve" },
];

const CAPTIONS: Record<Mode, string> = {
  whatif: "",
  sweep: "One question across a range: run the plan over a grid and colour it by what survived.",
  solve: "One question exactly: the value that just clears a bar you set.",
};

/** Analysis tab: the sweep grid, and the goal seek that reads exactly. */
export function AnalysisScreen({ scenarioId }: { scenarioId: number }) {
  const nav = useNav();
  const mode: Mode = nav.section === "solve" ? "solve" : "sweep";
  const { parameters, loading, error } = useParameters(scenarioId);

  // Sweep points at Solve: pinning a cell and then asking for the exact value
  // is the move the grid exists to set up.
  const [seed, setSeed] = useState<string>();
  const solveFor = useCallback(
    (parameterId: string) => {
      setSeed(parameterId);
      nav.setSection("solve");
    },
    [nav],
  );

  if (error) {
    return <EmptyState title="Cannot read this plan's parameters" detail={error} />;
  }
  if (loading || !parameters) {
    return <EmptyState title="Loading…" detail="Reading what this plan can vary." />;
  }
  if (parameters.length === 0) {
    return (
      <EmptyState
        title="Nothing to analyse yet"
        detail="Analysis varies numbers the plan already has. Give an event an age trigger or a fixed amount on the Plan tab, and both modes will have something to move."
      />
    );
  }

  return (
    <>
      <SubTabBar
        ariaLabel="Analysis mode"
        options={MODES}
        value={mode}
        onChange={nav.setSection}
        caption={CAPTIONS[mode]}
      />
      {mode === "solve" ? (
        // Keyed on the handed-over parameter: arriving from Sweep with a
        // different one is a fresh question, and remounting is how it opens on
        // that one without an effect overwriting an edit already made.
        <SolvePanel
          key={seed ?? "default"}
          scenarioId={scenarioId}
          parameters={parameters}
          initialParameterId={seed}
        />
      ) : (
        // Keyed on the scenario: the swept set and the graph layout are this
        // plan's, and carrying them onto another plan's parameters would leave
        // a workspace built over variables it does not have.
        <SweepPanel
          key={scenarioId}
          scenarioId={scenarioId}
          parameters={parameters}
          onSolveFor={solveFor}
        />
      )}
    </>
  );
}
