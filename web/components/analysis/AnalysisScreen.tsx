"use client";

import { useCallback } from "react";
import { LockedFeature, useGuest } from "@/components/auth/GuestContext";
import { usePlanCapabilities } from "@/lib/hooks/usePlanCapabilities";
import { SubTabBar } from "@/components/layout";
import { EmptyState } from "@/components/screens/EmptyState";
import type { SegmentOption } from "@/components/ui";
import type { Scenario } from "@/lib/api/types";
import { useParameters } from "@/lib/hooks/useAnalysis";
import { useNav } from "@/lib/nav";
import { SweepPanel } from "./SweepPanel";
import { WhatIfPanel } from "./WhatIfPanel";
import { DrawdownPanel } from "./DrawdownPanel";

type Mode = "what-if" | "sweep" | "drawdown";

/** Offer only implemented analysis workflows. */
const MODES: ReadonlyArray<SegmentOption<Mode>> = [
  { value: "what-if", label: "What-if" },
  { value: "sweep", label: "Sweep" },
  { value: "drawdown", label: "Drawdown" },
];

const CAPTIONS: Record<Mode, string> = {
  "what-if": "Stack overrides on a copy of the plan and see what each one costs or buys.",
  sweep: "Compare simulated outcomes across a range of plan inputs.",
  drawdown: "Plan which accounts fund each year of retirement spending.",
};

function modeOf(section: string | undefined): Mode {
  return section === "sweep" || section === "drawdown" ? section : "what-if";
}

/** Analysis tab: the what-if stack, the sweep grid, and the drawdown of a run. */
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

  const planChanged = useCallback(() => {
    reload();
    onPlanChanged();
  }, [reload, onPlanChanged]);

  // Analysis runs many simulations a request, which the guest limits switch
  // off: the server refuses what-if and sweep alike for a guest.
  // The limits are the server's, so they do not follow a plan kept on this
  // device, which runs on the visitor's own CPU.
  const { restricted } = useGuest();
  const { home } = usePlanCapabilities();

  let body;
  if (restricted && home === "cloud") {
    body = (
      <LockedFeature title="Analysis needs an account">
        What-if and Sweep each run many simulations of your plan. Create a free account to use
        them; your guest plan comes with you.
      </LockedFeature>
    );
  } else if (mode === "drawdown") {
    // Drawdown reads a finished run, not named parameters, so it stands alone.
    body = <DrawdownPanel key={scenarioId} scenario={scenario} onPlanChanged={onPlanChanged} />;
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
        detail="Add named parameters on the Plan tab and reference them in amounts or schedules. Sweep varies these shared inputs across runs."
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
