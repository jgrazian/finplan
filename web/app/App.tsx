"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AccountScreen } from "@/components/account";
import { AnalysisScreen } from "@/components/analysis";
import { LoginForm } from "@/components/auth/LoginForm";
import { AppHeader, AppShell, type TabDef } from "@/components/layout";
import { nearestStop, type RunEffort } from "@/components/results";
import {
  EmptyState,
  PlanScreen,
  PortfolioScreen,
  ResultsScreen,
} from "@/components/screens";
import { NewScenarioDialog } from "@/components/scenario/NewScenarioDialog";
import { SessionExpiredDialog, StatusBar } from "@/components/status";
import { Button } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Scenario as ApiScenario, UserResponse } from "@/lib/api/types";
import { useAsync } from "@/lib/hooks/useAsync";
import { useRun } from "@/lib/hooks/useRun";
import { type Session, useSession } from "@/lib/hooks/useSession";
import { useWorkspace } from "@/lib/hooks/useWorkspace";
import { NavProvider, type TabId, useNav } from "@/lib/nav";
import { useServerStatus } from "@/lib/status/useServerStatus";
import { useAppearance } from "@/lib/theme";
import type { InflationProfile, Scenario } from "@/lib/types";

/** The four tabs that describe the scenario; account settings is not one. */
type ScreenTab = Exclude<TabId, "account">;

const TABS: ReadonlyArray<TabDef<ScreenTab>> = [
  { id: "portfolio", label: "Portfolio" },
  { id: "plan", label: "Plan" },
  { id: "results", label: "Results" },
  { id: "analysis", label: "Analysis" },
];

/**
 * How long an edit has to settle before an automatic re-run starts.
 *
 * Long enough that saving three fields in a row queues one run rather than
 * three, short enough that the chart is not visibly lagging the plan.
 */
const AUTO_RUN_SETTLE_MS = 1_500;

/**
 * The application, from the session outwards.
 *
 * A client component, so the route above it can stay a server one and
 * enumerate the tab paths it serves.
 */
export function App() {
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
    <NavProvider>
      <Workbench session={session} user={session.user} />
    </NavProvider>
  );
}

function initials(name: string): string {
  const parts = name.split(/[\s@._-]+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : name.slice(0, 2)).toUpperCase();
}

function Workbench({ session, user }: { session: Session; user: UserResponse }) {
  // Which scenario, which tab and which row all come out of the query string,
  // so a refresh returns to the screen it left. Account settings is a
  // destination rather than a fifth tab: it is about the account, not the
  // scenario the tabs all describe, so it is a tab id the header does not list.
  const nav = useNav();

  // The palette is the account's, so it is applied here rather than at the
  // root: signed out, whatever the pre-paint script replayed stands, and the
  // login screen is not repainted in a stranger's colours on the way past.
  useAppearance({ mode: user.theme_mode, accent: user.accent });

  // How hard a run should work is a property of the question being asked, not
  // of the plan, so it lives here for the session rather than on the scenario.
  // The account preference seeds it; the Results slider moves it from there.
  const [effort, setEffort] = useState<RunEffort>(() =>
    nearestStop(user.default_iterations),
  );

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
  // until the query names something else. Derived, so a scenario deleted
  // elsewhere — or a stale id in a bookmarked URL — falls back rather than
  // leaving the screen pointed at nothing.
  const scenarioId =
    nav.scenario != null && list.some((s) => s.id === nav.scenario)
      ? nav.scenario
      : list[0]?.id;

  // Write that fallback back, so the URL names the scenario actually open and
  // the next refresh is not a second guess. Quietly: nothing was navigated to.
  useEffect(() => {
    if (scenarioId != null) nav.adoptScenario(scenarioId);
  }, [nav, scenarioId]);

  const workspace = useWorkspace(scenarioId);
  const run = useRun(workspace.scenario, workspace.axis);
  const status = useServerStatus();

  const start = useCallback(() => {
    nav.setTab("results");
    void run.start(effort);
  }, [nav, run, effort]);

  const refresh = useCallback(() => {
    scenarios.reload();
    workspace.reload();
  }, [scenarios, workspace]);

  // Auto re-run, when the preference asks for it.
  //
  // Driven off the scenario's own `updated_at` rather than a local edit flag,
  // so it counts what the server actually accepted. The first sighting of a
  // scenario is not an edit — otherwise opening the app would start a run —
  // and the run is left in the background rather than pulling the screen to
  // Results out from under whatever is being edited.
  const autoRun = user.auto_run && !status.offline;
  const updatedAt = workspace.scenario?.updated_at;
  const seen = useRef<{ id?: number; at?: string }>({});
  const startQuietly = useRef(run.start);
  useEffect(() => {
    startQuietly.current = run.start;
  }, [run.start]);

  useEffect(() => {
    if (scenarioId == null || updatedAt == null) return;
    const previous = seen.current;
    seen.current = { id: scenarioId, at: updatedAt };
    if (previous.id !== scenarioId || previous.at == null || previous.at === updatedAt) return;
    if (!autoRun) return;

    const timer = setTimeout(() => void startQuietly.current(effort), AUTO_RUN_SETTLE_MS);
    return () => clearTimeout(timer);
  }, [scenarioId, updatedAt, autoRun, effort]);

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
          activeTab={nav.tab}
          onTabChange={nav.setTab}
          scenarios={headerScenarios}
          activeScenarioId={scenarioId == null ? "" : String(scenarioId)}
          onScenarioChange={(id) => nav.setScenario(Number(id))}
          onNewScenario={() => setCreating(true)}
          userInitials={initials(user.display_name ?? user.email)}
          onAccount={() => {
            // The Data list shows each scenario's last run, which a run
            // started since the list loaded would have moved on from.
            scenarios.reload();
            nav.setTab("account");
          }}
          accountOpen={nav.tab === "account"}
          onRun={start}
          offline={status.offline}
        />

        {/* Server state lives here, directly under the nav and above every
            screen, so it is in the same place whatever you are looking at. */}
        <StatusBar
          onRetry={refresh}
          onRunAgain={start}
          onSignIn={() => void session.signOut()}
        />

        {nav.tab === "account" ? (
          <AccountScreen
            user={user}
            scenarios={list}
            offline={status.offline}
            onUserChange={session.update}
            onSignOut={() => void session.signOut()}
            onDeleted={session.forget}
          />
        ) : scenarios.error ? (
          <EmptyState title="Cannot reach the API" detail={scenarios.error.message} />
        ) : workspace.error ? (
          <EmptyState title="Cannot load this scenario" detail={workspace.error.message} />
        ) : scenarios.data && list.length === 0 ? (
          <NoScenarios onCreate={() => setCreating(true)} />
        ) : !workspace.scenario || !workspace.params || !workspace.axis ? (
          <EmptyState title="Loading…" detail="Fetching the scenario." />
        ) : (
          <>
            {nav.tab === "results" && (
              <ResultsScreen
                results={run.results}
                run={run.run}
                active={run.active}
                loading={run.loading}
                error={run.error}
                percentile={run.percentile}
                onPercentileChange={run.setPercentile}
                effort={effort}
                onEffortChange={setEffort}
                offline={status.offline}
                onRun={start}
                onCancel={run.cancel}
              />
            )}
            {nav.tab === "portfolio" && (
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
            {nav.tab === "plan" && (
              <PlanScreen
                offline={status.offline}
                scenarioId={workspace.scenario.id}
                scenarioName={workspace.scenario.name}
                params={workspace.params}
                axis={workspace.axis}
                events={workspace.events}
                raw={workspace.raw}
                onChanged={workspace.reload}
              />
            )}
            {nav.tab === "analysis" && (
              <AnalysisScreen scenarioId={workspace.scenario.id} />
            )}
          </>
        )}
      </AppShell>

      {status.issue?.kind === "session" && (
        <SessionExpiredDialog onSignIn={() => void session.signOut()} />
      )}

      {creating && (
        <NewScenarioDialog
          defaults={user}
          inflationProfiles={inflationProfiles}
          taxConfigs={taxConfigs}
          onClose={() => setCreating(false)}
          onCreated={(created) => {
            nav.setScenario(created.id);
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
