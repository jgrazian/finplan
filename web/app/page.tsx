"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { LoginForm } from "@/components/auth/LoginForm";
import { AppHeader, AppShell, type TabDef } from "@/components/layout";
import {
  EmptyState,
  PlaceholderScreen,
  PlanScreen,
  PortfolioScreen,
  ResultsScreen,
} from "@/components/screens";
import { NewScenarioDialog } from "@/components/scenario/NewScenarioDialog";
import { SessionExpiredDialog, StatusBar } from "@/components/status";
import { Button } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Scenario as ApiScenario } from "@/lib/api/types";
import { useAsync } from "@/lib/hooks/useAsync";
import { useRun } from "@/lib/hooks/useRun";
import { useSession } from "@/lib/hooks/useSession";
import { useWorkspace } from "@/lib/hooks/useWorkspace";
import { useServerStatus } from "@/lib/status/useServerStatus";
import type { InflationProfile, Scenario } from "@/lib/types";

type TabId = "portfolio" | "plan" | "results" | "analysis";

const TABS: ReadonlyArray<TabDef<TabId>> = [
  { id: "portfolio", label: "Portfolio" },
  { id: "plan", label: "Plan" },
  { id: "results", label: "Results" },
  { id: "analysis", label: "Analysis" },
];

/** Iterations a Run asks for. The server caps this at `--max-iterations`. */
const ITERATIONS = 2_000;

export default function Page() {
  const session = useSession();

  if (session.user === undefined) {
    return <main style={{ padding: 24 }}>Loading…</main>;
  }
  if (session.user === null) {
    return (
      <main style={{ padding: 24 }}>
        <LoginForm session={session} />
      </main>
    );
  }

  return (
    <Workbench
      initials={initials(session.user.display_name ?? session.user.email)}
      onSignOut={session.signOut}
    />
  );
}

function initials(name: string): string {
  const parts = name.split(/[\s@._-]+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : name.slice(0, 2)).toUpperCase();
}

function Workbench({
  initials,
  onSignOut,
}: {
  initials: string;
  onSignOut: () => void;
}) {
  const [tab, setTab] = useState<TabId>("results");
  const [picked, setPicked] = useState<number>();

  const scenarios = useAsync(() => api.scenarios.list(), []);
  const libraries = useAsync(
    async () =>
      Promise.all([api.inflationProfiles.list(), api.taxConfigs.list()]),
    [],
  );
  const [inflationProfiles = [], taxConfigs = []] = libraries.data ?? [];
  const [creating, setCreating] = useState(false);
  const list = useMemo(() => scenarios.data ?? [], [scenarios.data]);

  // The list is sorted most-recently-updated first, so that is the default
  // until the switcher picks something else. Derived, so a scenario that is
  // deleted elsewhere falls back rather than leaving a dangling id.
  const scenarioId =
    picked != null && list.some((s) => s.id === picked) ? picked : list[0]?.id;

  const workspace = useWorkspace(scenarioId, ITERATIONS);
  const run = useRun(workspace.scenario, workspace.axis);
  const status = useServerStatus();

  const start = useCallback(() => {
    setTab("results");
    void run.start(ITERATIONS);
  }, [run]);

  const refresh = useCallback(() => {
    scenarios.reload();
    workspace.reload();
  }, [scenarios, workspace]);

  // Keyboard parity with the TUI: `r` runs, as the header's keycap advertises.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return;
      if (e.key === "r" && !e.metaKey && !e.ctrlKey) start();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [start]);

  const headerScenarios = useMemo(
    () => list.map((s) => toHeaderScenario(s, run.run?.finished_at)),
    [list, run.run?.finished_at],
  );

  const activateInflation = useCallback(
    async (profile: InflationProfile) => {
      if (scenarioId == null) return;
      await api.scenarios.update(scenarioId, { inflation_profile_id: profile.serverId });
      workspace.reload();
    },
    [scenarioId, workspace],
  );

  return (
    <main style={{ padding: 24 }}>
      <AppShell>
        <AppHeader
          tabs={TABS}
          activeTab={tab}
          onTabChange={setTab}
          scenarios={headerScenarios}
          activeScenarioId={scenarioId == null ? "" : String(scenarioId)}
          onScenarioChange={(id) => setPicked(Number(id))}
          userInitials={initials}
          onRun={start}
          offline={status.offline}
          trailing={
            <>
              <Button onClick={() => setCreating(true)} disabled={status.offline}>
                New scenario
              </Button>
              <Button variant="ghost" onClick={onSignOut}>
                Sign out
              </Button>
            </>
          }
        />

        {/* Server state lives here, directly under the nav and above every
            screen, so it is in the same place whatever you are looking at. */}
        <StatusBar onRetry={refresh} onRunAgain={start} onSignIn={onSignOut} />

        {scenarios.error ? (
          <EmptyState title="Cannot reach the API" detail={scenarios.error.message} />
        ) : workspace.error ? (
          <EmptyState title="Cannot load this scenario" detail={workspace.error.message} />
        ) : scenarios.data && list.length === 0 ? (
          <NoScenarios onCreate={() => setCreating(true)} />
        ) : !workspace.scenario || !workspace.params || !workspace.axis ? (
          <EmptyState title="Loading…" detail="Fetching the scenario." />
        ) : (
          <>
            {tab === "results" && (
              <ResultsScreen
                results={run.results}
                run={run.run}
                active={run.active}
                loading={run.loading}
                error={run.error}
                onRun={start}
                onCancel={run.cancel}
              />
            )}
            {tab === "portfolio" && (
              <PortfolioScreen
                offline={status.offline}
                scenarioId={workspace.scenario.id}
                accounts={workspace.accounts}
                raw={workspace.raw}
                onChanged={workspace.reload}
                returnProfiles={workspace.returnProfiles}
                inflationProfiles={workspace.inflationProfiles}
                activeInflationProfile={workspace.activeInflationProfile}
                onActivateInflation={activateInflation}
              />
            )}
            {tab === "plan" && (
              <PlanScreen
                offline={status.offline}
                scenarioId={workspace.scenario.id}
                scenarioName={workspace.scenario.name}
                params={workspace.params}
                axis={workspace.axis}
                events={workspace.events}
                raw={workspace.raw}
                onChanged={workspace.reload}
                onRun={start}
              />
            )}
            {tab === "analysis" && <PlaceholderScreen label="Analysis" />}
          </>
        )}
      </AppShell>

      {status.issue?.kind === "session" && <SessionExpiredDialog onSignIn={onSignOut} />}

      {creating && (
        <NewScenarioDialog
          inflationProfiles={inflationProfiles}
          taxConfigs={taxConfigs}
          onClose={() => setCreating(false)}
          onCreated={(created) => {
            setPicked(created.id);
            scenarios.reload();
          }}
        />
      )}
    </main>
  );
}

/**
 * `dirty` marks results that no longer describe the plan: the scenario was
 * edited after the run that produced them finished.
 */
function toHeaderScenario(scenario: ApiScenario, lastRunAt: string | null | undefined): Scenario {
  return {
    id: String(scenario.id),
    serverId: scenario.id,
    name: scenario.name,
    dirty: lastRunAt != null && scenario.updated_at > lastRunAt,
  };
}

function NoScenarios({ onCreate }: { onCreate: () => void }) {
  return (
    <div style={{ padding: "40px 24px", maxWidth: 520 }}>
      <h4 style={{ margin: "0 0 6px" }}>No scenarios yet</h4>
      <p
        style={{
          margin: "0 0 14px",
          fontSize: 13,
          lineHeight: 1.5,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        A scenario holds the accounts, assets and events that make up one plan.
        Your return-profile and tax library is already seeded.
      </p>
      <Button variant="primary" onClick={onCreate}>
        Create a scenario
      </Button>
    </div>
  );
}
