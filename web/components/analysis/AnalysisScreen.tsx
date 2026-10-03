"use client";

import { useCallback, useState } from "react";
import { LockedFeature, useGuest } from "@/components/auth/GuestContext";
import { SubTabBar } from "@/components/layout";
import { EmptyState } from "@/components/screens/EmptyState";
import type { SegmentOption } from "@/components/ui";
import type { Scenario } from "@/lib/api/types";
import { useParameters } from "@/lib/hooks/useAnalysis";
import { useNav } from "@/lib/nav";
import { SolvePanel } from "./SolvePanel";
import { SweepPanel } from "./SweepPanel";
import { WhatIfPanel } from "./WhatIfPanel";

type Mode = "what-if" | "sweep" | "solve";

/** Offer only implemented analysis workflows. */
const MODES: ReadonlyArray<SegmentOption<Mode>> = [
  { value: "what-if", label: "What-if" },
  { value: "sweep", label: "Sweep" },
  { value: "solve", label: "Solve" },
];

const CAPTIONS: Record<Mode, string> = {
  "what-if": "Stack overrides on a copy of the plan and see what each one costs or buys.",
  sweep: "Compare simulated outcomes across a range of plan inputs.",
  solve: "Search for a value that meets your chosen outcome threshold.",
};

function modeOf(section: string | undefined): Mode {
  return section === "sweep" || section === "solve" ? section : "what-if";
}

/** Analysis tab: the what-if stack, the sweep grid, and the goal seek that reads exactly. */
export function AnalysisScreen({
  scenario,
  onPlanChanged,
  onScenarioCreated,
}: {
  scenario: Scenario;
  /** The plan was written to from here: reload what the app holds of it. */
  onPlanChanged: () => void;
  /** A what-if was saved as a new scenario: open it. */
  onScenarioCreated: (created: Scenario) => void;
}) {
  const nav = useNav();
  const mode = modeOf(nav.section);
  const scenarioId = scenario.id;
  const { parameters, error, reload } = useParameters(scenarioId, scenario.updated_at);

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

  const planChanged = useCallback(() => {
    reload();
    onPlanChanged();
  }, [reload, onPlanChanged]);

  // Analysis runs many simulations a request, which the guest limits switch
  // off: the server refuses what-if, sweep and goal seek alike for a guest.
  const { restricted } = useGuest();

  let body;
  if (restricted) {
    body = (
      <LockedFeature title="Analysis needs an account">
        What-if, Sweep and Solve each run many simulations of your plan. Create a free account
        to use them; your guest plan comes with you.
      </LockedFeature>
    );
  } else if (mode === "what-if") {
    // What-if stands without named parameters — market shocks and one-off
    // events need none — so only the parameter rows of its menu go missing.
    body = !parameters && !error ? (
      <EmptyState title="Loading…" detail="Reading what this plan can vary." />
    ) : (
      // Keyed on the scenario: the stack, and the answer on screen, are this plan's.
      <WhatIfPanel
        key={scenarioId}
        scenario={scenario}
        parameters={parameters ?? []}
        onPlanChanged={planChanged}
        onScenarioCreated={onScenarioCreated}
      />
    );
  } else if (error) {
    body = <EmptyState title="Cannot read this plan's parameters" detail={error} />;
  } else if (!parameters) {
    body = <EmptyState title="Loading…" detail="Reading what this plan can vary." />;
  } else if (parameters.length === 0) {
    body = (
      <EmptyState
        title="Nothing to analyse yet"
        detail="Add named parameters on the Plan tab and reference them in amounts or schedules. Sweep and Solve vary these shared inputs across runs."
      />
    );
  } else if (mode === "solve") {
    body = (
      // Keyed on the handed-over parameter: arriving from Sweep with a
      // different one is a fresh question, and remounting is how it opens on
      // that one without an effect overwriting an edit already made.
      <SolvePanel
        key={seed ?? "default"}
        scenarioId={scenarioId}
        parameters={parameters}
        initialParameterId={seed}
      />
    );
  } else {
    body = (
      // Keyed on the scenario: the swept set and the graph layout are this
      // plan's, and carrying them onto another plan's parameters would leave
      // a workspace built over variables it does not have.
      <SweepPanel
        key={scenarioId}
        scenarioId={scenarioId}
        parameters={parameters}
        onSolveFor={solveFor}
      />
    );
  }

  return (
    <>
      {/* The wrapper is a hook for app/mobile/analysis.css: on a phone the
          mode switch goes full width and the caption drops under it. */}
      <div className="an-modes">
        <SubTabBar
          ariaLabel="Analysis mode"
          options={MODES}
          value={mode}
          onChange={nav.setSection}
          caption={CAPTIONS[mode]}
        />
      </div>
      {body}
    </>
  );
}
