"use client";

import { useCallback, useEffect, useState } from "react";
import { AppHeader, AppShell, type TabDef } from "@/components/layout";
import {
  PlaceholderScreen,
  PlanScreen,
  PortfolioScreen,
  ResultsScreen,
} from "@/components/screens";
import { MOCK_SCENARIOS, MOCK_USER } from "@/lib/mock/scenario";

type TabId = "portfolio" | "plan" | "results" | "analysis";

const TABS: ReadonlyArray<TabDef<TabId>> = [
  { id: "portfolio", label: "Portfolio" },
  { id: "plan", label: "Plan" },
  { id: "results", label: "Results" },
  { id: "analysis", label: "Analysis" },
];

export default function Page() {
  const [tab, setTab] = useState<TabId>("results");
  const [scenarioId, setScenarioId] = useState(MOCK_SCENARIOS[0].id);

  const activeScenario =
    MOCK_SCENARIOS.find((s) => s.id === scenarioId) ?? MOCK_SCENARIOS[0];

  const run = useCallback(() => {
    // Wired to the engine later; the mock results are already deterministic.
  }, []);

  // Keyboard parity with the TUI: `r` runs, as the header's keycap advertises.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return;
      if (e.key === "r" && !e.metaKey && !e.ctrlKey) run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [run]);

  return (
    <main style={{ padding: 24 }}>
      <AppShell>
        <AppHeader
          tabs={TABS}
          activeTab={tab}
          onTabChange={setTab}
          scenarios={MOCK_SCENARIOS}
          activeScenarioId={scenarioId}
          onScenarioChange={setScenarioId}
          userInitials={MOCK_USER.initials}
          onRun={run}
        />

        {tab === "results" && <ResultsScreen />}
        {tab === "portfolio" && <PortfolioScreen />}
        {tab === "plan" && (
          <PlanScreen scenarioName={activeScenario.name} onRun={run} />
        )}
        {tab === "analysis" && <PlaceholderScreen label="Analysis" />}
      </AppShell>
    </main>
  );
}
