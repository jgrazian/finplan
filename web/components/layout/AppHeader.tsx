"use client";

import { type ReactNode, useMemo } from "react";
import { Button, Dropdown, type DropdownOption, Tag } from "@/components/ui";
import type { Run } from "@/lib/api/types";
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
  scenarios,
  activeScenarioId,
  onScenarioChange,
  onNewScenario,
  userInitials,
  onAccount,
  accountOpen,
  onRun,
  run,
  running,
  onCancel,
  offline,
  trailing,
}: {
  tabs: ReadonlyArray<TabDef<T>>;
  /** Absent while a page that is not a tab (New scenario) is on screen. */
  activeTab?: T;
  onTabChange: (id: T) => void;
  scenarios: Scenario[];
  activeScenarioId: string;
  onScenarioChange: (id: string) => void;
  /** Picked from the switcher's last row; omit to leave that row out. */
  onNewScenario?: () => void;
  userInitials: string;
  /** Opens account settings in place of the tab screens. */
  onAccount?: () => void;
  /** Account settings is what is on screen, so the avatar reads as current. */
  accountOpen?: boolean;
  onRun?: () => void;
  /** The scenario's latest run, read for its progress while `running`. */
  run?: Run;
  /** A run is queued or executing: Run gives its slot to the progress. */
  running?: boolean;
  onCancel?: () => void;
  /** Nothing can reach the server, so a run cannot be started. */
  offline?: boolean;
  /** Extra controls between the scenario switcher and Run. */
  trailing?: ReactNode;
}) {
  const active = scenarios.find((s) => s.id === activeScenarioId);

  const options = useMemo(() => {
    const rows: Array<DropdownOption<string>> = scenarios.map((s) => ({
      value: s.id,
      label: s.name,
    }));
    if (onNewScenario) {
      rows.push({
        value: NEW_SCENARIO,
        label: "New scenario…",
        action: true,
        disabled: offline,
      });
    }
    return rows;
  }, [offline, onNewScenario, scenarios]);

  return (
    <>
    <header
      className="app-header"
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
            aria-current={tab.id === activeTab ? "page" : undefined}
            onClick={() => onTabChange(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </nav>

      <Dropdown
        className="app-header-scenario"
        style={{ width: 170 }}
        ariaLabel="Scenario"
        placeholder="No scenario"
        options={options}
        value={activeScenarioId}
        maxMenuHeight={280}
        onChange={(id) => (id === NEW_SCENARIO ? onNewScenario?.() : onScenarioChange(id))}
      />

      {active?.dirty && <Tag tone="outline">results stale</Tag>}
      {trailing}

      {running ? (
        <RunProgress run={run} onCancel={onCancel} />
      ) : (
        <Button
          className="app-header-run"
          variant="primary"
          shortcut="r"
          onClick={onRun}
          disabled={offline}
          title={offline ? "No connection to the server." : undefined}
        >
          Run
        </Button>
      )}

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
    </header>

    {/* Phones: the same tabs, as a fixed bar along the bottom edge. */}
    <nav className="mobile-tabbar" aria-label="Sections">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          type="button"
          aria-current={tab.id === activeTab ? "page" : undefined}
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
 * Run's slot while a run is in flight: status and count over a thin fill,
 * with Cancel at the end. It holds Run's width, so the header never reflows
 * when a run starts or ends, and it shows from every tab.
 */
function RunProgress({ run, onCancel }: { run: Run | undefined; onCancel?: () => void }) {
  const done = run?.completed_iterations ?? 0;
  // A converging run stops when its median settles, so the bar is filling
  // towards a ceiling it is not expected to reach. Reading it against the
  // minimum sample instead would sit at 100% for most of the run.
  const converging = run?.converge === true;
  const total = (converging ? run?.max_iterations : run?.iterations) ?? 0;
  const pct = total > 0 ? Math.min(100, (done / total) * 100) : 0;
  const queued = run?.status === "queued";
  const f = (n: number) => n.toLocaleString("en-US");

  return (
    <div
      className="app-header-run run-progress"
      role="status"
      title={
        converging
          ? `${f(done)} iterations · sampling until the median settles, up to ${f(total)}`
          : `${f(done)} of ${f(total)} iterations`
      }
    >
      <div className="run-progress-body">
        <span className="run-progress-word">{queued ? "Queued" : "Simulating"}</span>
        <span className="run-progress-count">
          {f(done)} / {converging ? "≤" : ""}
          {f(total)}
        </span>
        <i className="run-progress-fill" style={{ width: `${pct}%` }} />
      </div>
      <button type="button" className="run-progress-cancel" title="Cancel run" onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}
