"use client";

import { useCallback, useState } from "react";
import { SubTabBar } from "@/components/layout";
import { EmptyState } from "@/components/screens/EmptyState";
import type { SegmentOption } from "@/components/ui";
import { useParameters } from "@/lib/hooks/useAnalysis";
import { useNav } from "@/lib/nav";
import { SolvePanel } from "./SolvePanel";
import { SweepPanel } from "./SweepPanel";

type Mode = "sweep" | "solve";

/** Offer only implemented analysis workflows. */
const MODES: ReadonlyArray<SegmentOption<Mode>> = [
  { value: "sweep", label: "Sweep" },
  { value: "solve", label: "Solve" },
];

const CAPTIONS: Record<Mode, string> = {
  sweep: "Compare simulated outcomes across a range of plan inputs.",
  solve: "Search for a value that meets your chosen outcome threshold.",
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
        detail="Add named parameters on the Plan tab and reference them in amounts or schedules. Analysis varies these shared inputs across runs."
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
