"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AccountScreen } from "@/components/account";
import { AnalysisScreen } from "@/components/analysis";
import { LoginForm } from "@/components/auth/LoginForm";
import { AppFooter, AppHeader, AppShell, type TabDef } from "@/components/layout";
import { nearestStop, type RunEffort } from "@/components/results";
import {
  EmptyState,
  PlanScreen,
  PortfolioScreen,
  ResultsScreen,
} from "@/components/screens";
import { ReviewScreen } from "@/components/review";
import { NewScenarioScreen } from "@/components/scenario/NewScenarioScreen";
import { SessionExpiredDialog, StatusBar } from "@/components/status";
import { Button, Tag } from "@/components/ui";
import { api } from "@/lib/api/client";
import { historyApi } from "@/lib/api/history";
import type { Scenario as ApiScenario, PreflightIssue, UserResponse } from "@/lib/api/types";
import { useAsync } from "@/lib/hooks/useAsync";
import { useReview } from "@/lib/hooks/useReview";
import { useRun } from "@/lib/hooks/useRun";
import { type Session, useSession } from "@/lib/hooks/useSession";
import { useWorkspace } from "@/lib/hooks/useWorkspace";
import { NavProvider, type TabId, useNav } from "@/lib/nav";
import { resolveScenario } from "@/lib/nav/url";
import { useServerStatus } from "@/lib/status/useServerStatus";
import { useAppearance } from "@/lib/theme";
import type { InflationProfile, Scenario } from "@/lib/types";
import { BETA_ACCESS_NOTICE } from "@/lib/view/access";
import { clockTime, pathChecks, summarizeIssues } from "@/lib/view/issues";
import { type Destination, noteCounts } from "@/lib/view/review";

/** The tabs that describe the scenario; account settings is not one. */
type ScreenTab = Exclude<TabId, "account">;

const TABS: ReadonlyArray<TabDef<ScreenTab>> = [
  { id: "portfolio", label: "Portfolio" },
  { id: "plan", label: "Plan" },
  { id: "results", label: "Results" },
  { id: "analysis", label: "Analysis" },
  { id: "review", label: "Review" },
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
    return <main className="app-main app-main-plain">Loading…</main>;
  }
  if (session.user === null) {
    return (
      <main className="app-main app-main-plain">
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
  const access = useAsync(() => historyApi.entitlements(), []);
  const libraries = useAsync(
    async () =>
      Promise.all([api.inflationProfiles.list(), api.taxConfigs.list()]),
    [],
  );
  const [inflationProfiles = [], taxConfigs = []] = libraries.data ?? [];
  const [creating, setCreating] = useState(false);
  const [recentlyCreated, setRecentlyCreated] = useState<ApiScenario>();
  // Bridge the list refresh without resurrecting this row after later deletion.
  if (recentlyCreated && scenarios.data?.some((row) => row.id === recentlyCreated.id)) {
    setRecentlyCreated(undefined);
  }
  // A draft (design 2c) is opened on Review but is never a plan in the list:
  // the server does not list it, and it is looked up here, after the plans, so
  // a stale link falls back to a plan rather than to the draft.
  const [draft, setDraft] = useState<ApiScenario>();
  const list = useMemo(() => {
    const rows = scenarios.data ?? [];
    return recentlyCreated && !rows.some((row) => row.id === recentlyCreated.id)
      ? [recentlyCreated, ...rows] : rows;
  }, [scenarios.data, recentlyCreated]);

  // The list is sorted most-recently-updated first, so that is the default
  // until the query names something else. Derived, so a scenario deleted
  // elsewhere — or a stale id in a bookmarked URL — falls back rather than
  // leaving the screen pointed at nothing.
  const selectedScenario =
    (draft != null && nav.scenario === draft.slug ? draft : undefined) ??
    resolveScenario(list, nav.scenario);
  const isDraft = selectedScenario?.status === "draft";
  const scenarioId = selectedScenario?.id;
  const scenarioSlug = selectedScenario?.slug;

  // Canonicalize fallbacks and legacy numeric bookmarks without adding history.
  useEffect(() => {
    if (scenarioSlug != null) nav.adoptScenario(scenarioSlug);
  }, [nav, scenarioSlug]);

  const workspace = useWorkspace(scenarioId);
  const run = useRun(workspace.scenario);
  const review = useReview(scenarioId);
  const status = useServerStatus();

  // What would stop the next run. Re-read on every accepted edit, so fixing
  // an issue clears it from the strip without a reload.
  const preflight = useAsync(
    async () => (scenarioId == null ? undefined : api.scenarios.preflight(scenarioId)),
    [scenarioId, workspace.scenario?.updated_at],
  );
  const loadedRun = run.history.find((r) => r.id === run.selectedRunId);
  // Names for the rows warnings refer to, and each account's kind for the
  // path checks. Both come from the live plan, so an account deleted since the
  // run is left unnamed rather than guessed at.
  const accounts = workspace.accounts;
  const events = workspace.events;
  const results = run.results;
  const issues = useMemo(() => {
    const accountById = new Map(accounts.map((a) => [a.serverId, a]));
    const eventById = new Map(events.map((e) => [e.serverId, e.id]));
    return summarizeIssues({
      run: run.run,
      // A superseded scenario's report must not describe this one.
      preflight: preflight.loading ? undefined : preflight.data,
      stale: run.stale,
      lastRunAt: clockTime(loadedRun?.created_at),
      fundingSuccessRate: results?.stats.fundingSuccessRate,
      iterations: results?.stats.numIterations,
      warnings: results?.warnings ?? [],
      pathLabel: results?.pathLabel,
      checks: results
        ? pathChecks({
            years: results.bands.years,
            cashFlows: results.cashFlows,
            accountSeries: results.accountSeries,
            flavorOf: (id) => accountById.get(Number(id))?.flavor,
          })
        : [],
      names: {
        event: (id) => eventById.get(id),
        account: (id) => accountById.get(id)?.name,
      },
    });
  }, [run.run, run.stale, results, preflight.loading, preflight.data, loadedRun?.created_at, accounts, events]);

  const reviewEvent = useCallback(
    (eventId: number) => {
      const event = events.find((e) => e.serverId === eventId);
      if (scenarioSlug == null) nav.setTab("plan");
      else nav.openScenario(scenarioSlug, "plan", event?.id);
    },
    [nav, scenarioSlug, events],
  );

  // An issue names its section; one about a single event opens on that event.
  // Portfolio rows are picked by account, which an asset issue does not name.
  const reviewIssue = useCallback(
    (issue: PreflightIssue) => {
      const tab = issue.section === "portfolio" ? "portfolio" : "plan";
      const event =
        tab === "plan" ? workspace.events.find((e) => e.serverId === issue.record_id) : undefined;
      if (scenarioSlug == null) nav.setTab(tab);
      else nav.openScenario(scenarioSlug, tab, event?.id);
    },
    [nav, scenarioSlug, workspace.events],
  );

  // Review notes name rows by id; these put names on them and open them.
  const reviewNames = useMemo(() => {
    const accountById = new Map(accounts.map((a) => [a.serverId, a.name]));
    const eventById = new Map(events.map((e) => [e.serverId, e.id]));
    return {
      account: (id: number) => accountById.get(id),
      event: (id: number) => eventById.get(id),
    };
  }, [accounts, events]);
  const openSuggestions = review.review?.suggestions;
  const reviewCounts = useMemo(() => noteCounts(openSuggestions ?? []), [openSuggestions]);
  const portfolioNotes = useMemo(
    () => (openSuggestions ?? []).filter((s) => s.status === "open" && s.section === "portfolio").length,
    [openSuggestions],
  );
  const openReview = useCallback(() => nav.setTab("review"), [nav]);

  const followEvidence = useCallback(
    (to: Destination) => {
      if (to.tab === "portfolio") {
        const account = accounts.find((a) => a.serverId === to.accountId);
        nav.openTab("portfolio", { section: "accounts", selection: account?.accountId });
      } else if (to.tab === "plan") {
        const event = events.find((e) => e.serverId === to.eventId);
        nav.openTab("plan", { selection: event?.id });
      } else {
        nav.setTab("results");
      }
    },
    [nav, accounts, events],
  );

  // A stress note applied to a copy: open the copy on its plan, the way a new
  // scenario is bridged into the list before the reload lands. It is not run;
  // the copy's own Run button does that.
  const openCopy = useCallback(
    async (copyId: number) => {
      const created = await api.scenarios.get(copyId);
      setRecentlyCreated(created);
      nav.openScenario(created.slug, "plan");
      scenarios.reload();
    },
    [nav, scenarios],
  );

  const start = useCallback(() => {
    // A draft has nothing to run until Create & run makes it a plan.
    if (isDraft) return;
    nav.setTab("results");
    void run.start(effort);
  }, [nav, run, effort, isDraft]);

  const refresh = useCallback(() => {
    scenarios.reload();
    workspace.reload();
  }, [scenarios, workspace]);

  const saved = useCallback(() => {
    run.markInputsChanged();
    refresh();
    libraries.reload();
  }, [run, refresh, libraries]);

  // Auto re-run, when the preference asks for it.
  //
  // Driven off the scenario's own `updated_at` rather than a local edit flag,
  // so it counts what the server actually accepted. The first sighting of a
  // scenario is not an edit — otherwise opening the app would start a run —
  // and the run is left in the background rather than pulling the screen to
  // Results out from under whatever is being edited. Not on Review: applying
  // notes there batches edits for one deliberate re-run, which its banner offers.
  const autoRun = user.auto_run && !status.offline && nav.tab !== "review" && !isDraft;
  const updatedAt = workspace.scenario?.updated_at;
  const seen = useRef<{ id?: number; at?: string }>({});
  const startQuietly = useRef(() => run.start(effort));
  useEffect(() => {
    startQuietly.current = () => run.start(effort);
  }, [run, effort]);

  useEffect(() => {
    if (scenarioId == null || updatedAt == null) return;
    const previous = seen.current;
    seen.current = { id: scenarioId, at: updatedAt };
    if (previous.id !== scenarioId || previous.at == null || previous.at === updatedAt) return;
    if (!autoRun) return;

    const timer = setTimeout(() => void startQuietly.current(), AUTO_RUN_SETTLE_MS);
    return () => clearTimeout(timer);
  }, [scenarioId, updatedAt, autoRun, effort]);

  // Keyboard parity with the TUI: `r` runs, as the header's keycap advertises.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return;
      // New scenario covers the plan that `r` would run.
      if (creating) return;
      if (e.key === "r" && !e.metaKey && !e.ctrlKey) start();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [start, creating]);

  // Create & run made the draft a plan: it joins the list like any new one.
  const draftCreated = useCallback(
    (created: ApiScenario) => {
      setDraft(undefined);
      setRecentlyCreated(created);
      nav.openScenario(created.slug, "results");
      scenarios.reload();
      access.reload();
    },
    [nav, scenarios, access],
  );

  // The draft was deleted: back to the plans.
  const draftDiscarded = useCallback(() => {
    setDraft(undefined);
    if (list[0]) nav.openScenario(list[0].slug, "review");
    else nav.setTab("review");
    access.reload();
  }, [nav, list, access]);

  const headerScenarios = useMemo(
    () => list.map((s) => ({
      ...toHeaderScenario(s),
      ...(s.id === scenarioId ? { dirty: run.stale } : {}),
    })),
    [list, scenarioId, run.stale],
  );

  const activateInflation = useCallback(
    async (profile: InflationProfile) => {
      if (scenarioId == null) return;
      await api.scenarios.update(scenarioId, { inflation_profile_id: profile.serverId });
      saved();
    },
    [scenarioId, saved],
  );

  return (
    <main className="app-main">
      <AppShell>
        <AppHeader
          tabs={TABS}
          activeTab={creating ? undefined : nav.tab}
          onTabChange={(tab) => {
            // New scenario is a page, not a tab: any tab leaves it.
            setCreating(false);
            nav.setTab(tab);
          }}
          scenarios={headerScenarios}
          activeScenarioId={isDraft ? "" : (scenarioSlug ?? "")}
          trailing={isDraft ? <Tag tone="accent">AI draft</Tag> : undefined}
          onScenarioChange={(slug) => {
            setCreating(false);
            nav.setScenario(slug);
          }}
          onNewScenario={() => setCreating(true)}
          userInitials={initials(user.display_name ?? user.email)}
          onAccount={() => {
            // The Data list shows each scenario's last run, which a run
            // started since the list loaded would have moved on from.
            scenarios.reload();
            setCreating(false);
            nav.setTab("account");
          }}
          accountOpen={!creating && nav.tab === "account"}
          onRun={() => {
            setCreating(false);
            start();
          }}
          offline={status.offline}
        />

        {/* Server state lives here, directly under the nav and above every
            screen, so it is in the same place whatever you are looking at. */}
        <StatusBar
          onRetry={refresh}
          onRunAgain={start}
          onSignIn={() => void session.signOut()}
          exclude={nav.tab === "results" ? "run" : undefined}
        />

        {access.data?.access_mode === "beta" && (
          <p style={{ margin: 0, padding: "10px 16px", fontSize: 12, borderBottom: "1px solid var(--color-divider)" }}>
            {BETA_ACCESS_NOTICE} Share feedback through Contact below.
          </p>
        )}

        {creating ? (
          <NewScenarioScreen
            defaults={user}
            inflationProfiles={inflationProfiles}
            taxConfigs={taxConfigs}
            access={access.data}
            onClose={() => {
              setCreating(false);
              // A draft started here spent one of the month's drafts.
              access.reload();
            }}
            onCreated={(created) => {
              setRecentlyCreated(created);
              nav.openScenario(created.slug, "plan");
              scenarios.reload();
            }}
            onDraftCreated={(created) => {
              setCreating(false);
              draftCreated(created);
            }}
            onReviewDraft={(created) => {
              setCreating(false);
              setDraft(created);
              nav.openScenario(created.slug, "review");
            }}
          />
        ) : nav.tab === "account" ? (
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
                issues={issues}
                onReviewIssue={reviewIssue}
                onReviewEvent={reviewEvent}
              />
            )}
            {nav.tab === "portfolio" && (
              <PortfolioScreen
                offline={status.offline}
                scenarioId={workspace.scenario.id}
                accounts={workspace.accounts}
                raw={workspace.raw}
                onChanged={saved}
                reviewNotes={portfolioNotes}
                onOpenReview={openReview}
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
                params={workspace.params}
                assumptions={workspace.assumptions}
                axis={workspace.axis}
                events={workspace.events}
                raw={workspace.raw}
                onChanged={saved}
                reviewNotes={reviewCounts.events}
                onOpenReview={openReview}
              />
            )}
            {nav.tab === "review" && (
              <ReviewScreen
                draft={
                  isDraft && selectedScenario
                    ? { scenario: selectedScenario, onCreated: draftCreated, onDiscarded: draftDiscarded }
                    : undefined
                }
                state={review}
                runs={run.history}
                planChanged={run.stale}
                running={run.active}
                names={reviewNames}
                offline={status.offline}
                onPlanChanged={saved}
                onRerun={() => void run.start(effort)}
                onOpenCopy={(id) => void openCopy(id)}
                onNavigate={followEvidence}
              />
            )}
            {nav.tab === "analysis" && (
              <AnalysisScreen
                scenario={workspace.scenario}
                onPlanChanged={saved}
                onScenarioCreated={(created) => {
                  // The same bridge a new scenario gets: the list reload
                  // lands after the navigation, and the switcher must not
                  // fall back to another plan in between.
                  setRecentlyCreated(created);
                  nav.openScenario(created.slug, "analysis");
                  scenarios.reload();
                }}
              />
            )}
          </>
        )}
      </AppShell>

      <AppFooter />

      {status.issue?.kind === "session" && (
        <SessionExpiredDialog onSignIn={() => void session.signOut()} />
      )}

    </main>
  );
}

/**
 * `dirty` marks results that no longer describe the plan: the scenario was
 * edited after the run that produced them finished.
 */
function toHeaderScenario(scenario: ApiScenario): Scenario {
  return {
    id: scenario.slug,
    serverId: scenario.id,
    name: scenario.name,
    dirty: scenario.last_run_at != null && scenario.updated_at > scenario.last_run_at,
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
