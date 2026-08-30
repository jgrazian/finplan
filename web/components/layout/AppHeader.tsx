"use client";

import type { ReactNode } from "react";
import { Button, Select, Tag } from "@/components/ui";
import type { Scenario } from "@/lib/types";

export interface TabDef<T extends string> {
  id: T;
  label: string;
}

/**
 * Header grammar 6b — one row: brand, inline tabs, scenario switcher, Run,
 * avatar. Buys back the ~38px of vertical space that a two-row header
 * spends, which the Results charts use.
 */
export function AppHeader<T extends string>({
  tabs,
  activeTab,
  onTabChange,
  scenarios,
  activeScenarioId,
  onScenarioChange,
  userInitials,
  onRun,
  trailing,
}: {
  tabs: ReadonlyArray<TabDef<T>>;
  activeTab: T;
  onTabChange: (id: T) => void;
  scenarios: Scenario[];
  activeScenarioId: string;
  onScenarioChange: (id: string) => void;
  userInitials: string;
  onRun?: () => void;
  /** Extra controls between the scenario switcher and Run. */
  trailing?: ReactNode;
}) {
  const active = scenarios.find((s) => s.id === activeScenarioId);

  return (
    <header
      style={{
        display: "flex",
        alignItems: "center",
        gap: 18,
        padding: "0 18px",
        borderBottom: "1px solid var(--color-divider)",
      }}
    >
      <span className="nav-brand" style={{ margin: 0, padding: "12px 0" }}>
        FINPLAN
      </span>

      <nav style={{ display: "flex", gap: 2, marginRight: "auto" }} aria-label="Sections">
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

      <Select
        style={{ minHeight: 30, width: 170 }}
        aria-label="Scenario"
        value={activeScenarioId}
        onChange={(e) => onScenarioChange(e.target.value)}
      >
        {scenarios.map((s) => (
          <option key={s.id} value={s.id}>
            {s.name}
          </option>
        ))}
      </Select>

      {active?.dirty && <Tag tone="outline">results stale</Tag>}
      {trailing}

      <Button variant="primary" shortcut="r" onClick={onRun}>
        Run
      </Button>

      <button
        type="button"
        className="btn btn-secondary btn-icon"
        aria-label="Account"
        style={{
          fontSize: 9.5,
          fontFamily: "ui-monospace, Menlo, monospace",
          width: 30,
          height: 30,
        }}
      >
        {userInitials}
      </button>
    </header>
  );
}
