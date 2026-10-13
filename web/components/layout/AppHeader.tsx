"use client";

import { type ReactNode, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button, Dropdown, type DropdownOption, Tag } from "@/components/ui";
import { HOME_LABEL, HomeIcon } from "@/components/local/HomeIcon";
import type { Run } from "@/lib/api/types";
import type { PlanHome } from "@/lib/nav";
import type { Scenario } from "@/lib/types";
import { BrandMark, Wordmark } from "./Brand";

export interface TabDef<T extends string> {
  id: T;
  label: string;
}

/**
 * The switcher's last row creates a scenario instead of choosing one. Scenario
 * ids are the server's, so a decorated key cannot collide with one.
 */
const NEW_SCENARIO = "\u0000new-scenario";
/** The switcher's other commands: move the open scenario to its other home, and the plan files. */
const MOVE_SCENARIO = "\u0000move-scenario";
const PLAN_FILES = "\u0000plan-files";

/** Menu sections, in the order a scenario's home is listed. */
const HOME_GROUP: Record<PlanHome, string> = {
  local: "On this device",
  cloud: "In the cloud",
};

/** What the header says beside the switcher about where the open plan is saved. */
const HOME_STATUS: Record<PlanHome, string> = {
  local: "Saved on this device",
  cloud: "Saved to the cloud",
};

/**
 * Header grammar 6b — one row: brand, inline tabs, scenario switcher, Run,
 * avatar. Buys back the ~38px of vertical space that a two-row header
 * spends, which the Results charts use. On a phone the inline tabs hide and
 * a fixed bottom bar carries them instead.
 */
export function AppHeader<T extends string>({
  tabs,
  activeTab,
  onTabChange,
  tabsDisabled,
  start,
  scenarios,
  activeScenarioId,
  onScenarioChange,
  onNewScenario,
  userInitials,
  onAccount,
  accountOpen,
  guest,
  onRun,
  runBlocked,
  run,
  running,
  onCancel,
  runWhere,
  offline,
  trailing,
  homeOf,
  onMoveCurrent,
  onFiles,
  homeNote,
}: {
  tabs: ReadonlyArray<TabDef<T>>;
  /** Absent while a page that is not a tab (New scenario) is on screen. */
  activeTab?: T;
  onTabChange: (id: T) => void;
  /**
   * Why the tabs cannot be opened yet (there is no plan for them to show), said
   * on hover. Set, every tab is greyed out and none reads as current.
   */
  tabsDisabled?: string;
  /**
   * No plan exists yet. The switcher, which would only say so, steps out, and
   * on a phone Sign up and Sign in leave the header for the start card.
   */
  start?: boolean;
  scenarios: Scenario[];
  activeScenarioId: string;
  onScenarioChange: (id: string) => void;
  /** Picked from the switcher's last row; omit to leave that row out. */
  onNewScenario?: () => void;
  userInitials: string;
  /**
   * A guest has no account to open: Sign up and Sign in take the avatar's
   * place, and the two handlers open those forms.
   */
  guest?: { onSignUp: () => void; onSignIn: () => void };
  /** Opens account settings in place of the tab screens. */
  onAccount?: () => void;
  /** Account settings is what is on screen, so the avatar reads as current. */
  accountOpen?: boolean;
  onRun?: () => void;
  /**
   * Why Run has nothing to do yet, e.g. no scenario exists. Run is then shown
   * disabled and unfilled, so it does not outshout the button that would fix it.
   */
  runBlocked?: string;
  /** The scenario's latest run, read for its progress while `running`. */
  run?: Run;
  /** A run is queued or executing: Run gives its slot to the progress. */
  running?: boolean;
  onCancel?: () => void;
  /** The run is on FinPlan's servers (an offloaded local run), not on this device. */
  runWhere?: "server";
  /** Nothing can reach the server, so a run cannot be started. */
  offline?: boolean;
  /** Extra controls between the scenario switcher and Run. */
  trailing?: ReactNode;
  /**
   * Local mode: where each scenario lives. Every row and the picked value then
   * carry the home's mark, the menu groups by home, and the header says where
   * the open scenario is saved. Absent, the switcher is a plain list.
   */
  homeOf?: (id: string) => PlanHome;
  /** Offered in the menu's foot: move the open scenario to its other home. */
  onMoveCurrent?: () => void;
  /** The menu's last row: export plans as a file, or import one to this device. */
  onFiles?: () => void;
  /** Said on hover over the saved-where line, e.g. the privacy promise. */
  homeNote?: (home: PlanHome) => string;
}) {
  const active = scenarios.find((s) => s.id === activeScenarioId);
  const activeHome = active && homeOf ? homeOf(active.id) : undefined;

  const options = useMemo(() => {
    const rows: Array<DropdownOption<string>> = [];
    if (homeOf) {
      // Grouped by home, this device first: an empty home still shows its
      // heading, so where a new plan would go is never a mystery.
      for (const home of ["local", "cloud"] as const) {
        const group = HOME_GROUP[home];
        const mine = scenarios.filter((s) => homeOf(s.id) === home);
        for (const s of mine) {
          rows.push({ value: s.id, label: s.name, group, icon: <HomeIcon home={home} /> });
        }
        if (mine.length === 0) {
          rows.push({ value: `\u0000none-${home}`, label: "None", group, disabled: true });
        }
      }
    } else {
      for (const s of scenarios) rows.push({ value: s.id, label: s.name });
    }
    if (onNewScenario) {
      rows.push({
        value: NEW_SCENARIO,
        label: "New scenario…",
        action: true,
        disabled: offline,
      });
    }
    if (onMoveCurrent && activeHome) {
      rows.push({
        value: MOVE_SCENARIO,
        label: activeHome === "local" ? "Move to cloud…" : "Move to device…",
        action: true,
        disabled: offline,
      });
    }
    // Files need no server: a backup is how a plan on this device survives one.
    if (onFiles) rows.push({ value: PLAN_FILES, label: "Export / import…", action: true });
    return rows;
  }, [activeHome, homeOf, offline, onFiles, onMoveCurrent, onNewScenario, scenarios]);

  return (
    <>
    <header
      className={start ? "app-header app-header-start" : "app-header"}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 18,
        padding: "0 18px",
        borderBottom: "1px solid var(--color-divider)",
      }}
    >
      <span className="nav-brand" style={{ margin: 0, padding: "13px 0" }}>
        <BrandMark />
        <Wordmark />
      </span>

      <nav className="app-header-tabs" style={{ display: "flex", gap: 4, marginRight: "auto" }} aria-label="Sections">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            className="tab-inline"
            aria-current={!tabsDisabled && tab.id === activeTab ? "page" : undefined}
            disabled={tabsDisabled != null}
            title={tabsDisabled}
            onClick={() => onTabChange(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </nav>

      {activeHome && (
        <span
          className="app-header-home"
          title={homeNote?.(activeHome)}
          aria-label={`${HOME_STATUS[activeHome]}. ${HOME_LABEL[activeHome]}.`}
        >
          {HOME_STATUS[activeHome]}
        </span>
      )}

      {!start && (
      <Dropdown
        className="app-header-scenario"
        style={{ width: homeOf ? 200 : 170 }}
        ariaLabel="Scenario"
        placeholder="No scenario"
        options={options}
        value={activeScenarioId}
        maxMenuHeight={320}
        onChange={(id) =>
          id === NEW_SCENARIO
            ? onNewScenario?.()
            : id === MOVE_SCENARIO
              ? onMoveCurrent?.()
              : id === PLAN_FILES
                ? onFiles?.()
                : onScenarioChange(id)
        }
      />
      )}

      {active?.dirty && <Tag tone="outline">results stale</Tag>}
      {trailing}

      <RunSlot>
      {running ? (
        // The progress is the line along the header's foot; this slot keeps
        // Run's size and offers the way to stop.
        <Button
          className="app-header-run"
          onClick={onCancel}
          title={runProgress(run, runWhere === "server").text}
        >
          Cancel
        </Button>
      ) : (
        <Button
          className="app-header-run"
          variant={runBlocked ? "secondary" : "primary"}
          shortcut={runBlocked ? undefined : "r"}
          onClick={onRun}
          disabled={offline || runBlocked != null}
          title={offline ? "No connection to the server." : runBlocked}
        >
          Run
        </Button>
      )}
      </RunSlot>

      {guest ? (
        <>
          <Button className="app-header-signup" onClick={guest.onSignUp}>
            Sign up
          </Button>
          <button
            type="button"
            className="linkbtn app-header-signin"
            style={{ whiteSpace: "nowrap" }}
            onClick={guest.onSignIn}
          >
            Sign in
          </button>
        </>
      ) : (
        <button
          type="button"
          className="btn btn-secondary btn-icon app-avatar"
          aria-label="Account settings"
          aria-current={accountOpen ? "page" : undefined}
          title="Account settings"
          onClick={onAccount}
          style={
            accountOpen
              ? { background: "var(--color-accent)", color: "var(--color-bg)", borderColor: "var(--color-accent)" }
              : undefined
          }
        >
          {userInitials}
        </button>
      )}
      <RunLine run={run} running={running === true} server={runWhere === "server"} />
    </header>

    {/* Phones: the same tabs, as a fixed bar along the bottom edge. */}
    <nav className="mobile-tabbar" aria-label="Sections">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          type="button"
          aria-current={!tabsDisabled && tab.id === activeTab ? "page" : undefined}
          disabled={tabsDisabled != null}
          title={tabsDisabled}
          onClick={() => onTabChange(tab.id)}
        >
          {tab.label}
        </button>
      ))}
    </nav>
    </>
  );
}

/**
 * Where Run and its progress take turns. Run is sized to its label and the
 * progress is wider, so the slot eases between their widths instead of
 * snapping, and the controls left of it glide aside rather than jump. The
 * content keeps its own width inside and is clipped while the slot moves.
 */
function RunSlot({ children }: { children: ReactNode }) {
  const inner = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState<number>();
  useLayoutEffect(() => {
    const el = inner.current;
    if (!el) return;
    const measure = () => setWidth(el.offsetWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <div className="app-header-run-slot" style={{ width }}>
      <div ref={inner} className="app-header-run-inner">
        {children}
      </div>
    </div>
  );
}

/** What a run is doing and how far it has got, for the line and the words. */
function runProgress(run: Run | undefined, server: boolean) {
  const done = run?.completed_iterations ?? 0;
  // A converging run stops when its median settles, so it fills towards a
  // ceiling it is not expected to reach.
  const converging = run?.converge === true;
  const total = (converging ? run?.max_iterations : run?.iterations) ?? 0;
  const queued = run?.status === "queued";
  const f = (n: number) => n.toLocaleString("en-US");
  const word = queued ? "Queued" : server ? "Simulating on FinPlan's servers" : "Simulating";
  return {
    queued,
    pct: total > 0 ? Math.min(100, (done / total) * 100) : 0,
    text: queued ? word : `${word}: ${f(done)} of ${converging ? "at most " : ""}${f(total)} iterations`,
  };
}

/**
 * The run's progress as the header's divider filling in from the left, so
 * nothing in the bar changes size while a plan runs. It belongs to one run:
 * a new run starts its own line from empty, and a finished one fills to the
 * end and fades rather than vanishing.
 */
function RunLine({ run, running, server }: { run: Run | undefined; running: boolean; server: boolean }) {
  // One line per run seen going by, numbered rather than keyed by the run's
  // id (an offload's stand-in run always has the same one). Until a run is
  // seen there is no line: a finished run already on screen at load is not news.
  const [wasRunning, setWasRunning] = useState(false);
  const [line, setLine] = useState<{ n: number; id: number | undefined }>();
  if (running !== wasRunning) {
    setWasRunning(running);
    if (running) setLine({ n: (line?.n ?? 0) + 1, id: run?.id });
  } else if (running && line && run?.id !== line.id) {
    // The next run took over with no pause between them.
    setLine({ n: line.n + 1, id: run?.id });
  }
  if (!line) return null;
  const { queued, pct, text } = runProgress(run, server);
  return (
    <div
      key={line.n}
      className={`app-header-progress${running ? "" : " done"}${running && queued ? " queued" : ""}`}
      role="progressbar"
      aria-label={running ? text : "Run finished"}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(running ? pct : 100)}
    >
      <i style={{ width: `${running ? pct : 100}%` }} />
    </div>
  );
}
