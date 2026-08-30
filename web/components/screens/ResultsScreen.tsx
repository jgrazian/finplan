"use client";

import { useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  CashFlowTable,
  type ChartView,
  ChartToolbar,
  NetWorthChart,
  RunSummary,
  SuccessRate,
  WarningList,
} from "@/components/results";
import { Button, Hr } from "@/components/ui";
import type { Percentile, ResultsData } from "@/lib/types";
import type { Run } from "@/lib/api/types";
import { EmptyState } from "./EmptyState";

const NEXT_PERCENTILE: Record<Percentile, Percentile> = {
  p50: "p95",
  p95: "p5",
  p5: "p50",
};

function chartCopy(view: ChartView, percentile: Percentile, ages: string) {
  if (view === "stack") {
    return {
      title: "Net worth by account",
      subtitle: `${percentile.toUpperCase()} representative run · ${ages}`,
    };
  }
  if (view === "bar") {
    return {
      title: "Net worth by year",
      subtitle: `${percentile.toUpperCase()} representative run · ${ages}`,
    };
  }
  return {
    title: "Net worth — P5 / P50 / P95 band",
    subtitle: "shaded band spans the 5th to 95th percentile run",
  };
}

/** Results tab: the run's success rate, its net-worth paths and its warnings. */
export function ResultsScreen({
  results,
  run,
  active,
  loading,
  error,
  onRun,
  onCancel,
}: {
  results: ResultsData | undefined;
  run: Run | undefined;
  /** A run is queued or executing right now. */
  active: boolean;
  loading: boolean;
  error: string | undefined;
  onRun: () => void;
  onCancel: () => void;
}) {
  const [view, setView] = useState<ChartView>("fan");
  const [percentile, setPercentile] = useState<Percentile>("p50");

  if (error) {
    return <EmptyState title="The run did not finish" detail={error} />;
  }
  if (active) {
    return <RunProgress run={run} onCancel={onCancel} />;
  }
  if (loading) {
    return <EmptyState title="Loading results…" detail="Fetching the scenario's last run." />;
  }
  if (!results) {
    return (
      <div style={{ padding: "40px 24px", maxWidth: 520 }}>
        <h4 style={{ margin: "0 0 6px" }}>No results yet</h4>
        <p
          style={{
            margin: "0 0 14px",
            fontSize: 13,
            lineHeight: 1.5,
            color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
          }}
        >
          Nothing has been simulated for this scenario. A run compiles the plan and
          executes it thousands of times against sampled market returns.
        </p>
        <Button variant="primary" shortcut="r" onClick={onRun}>
          Run
        </Button>
      </div>
    );
  }

  const { bands, stats } = results;
  const span =
    bands.ages.length > 0
      ? `${bands.ages[0]}–${bands.ages[bands.ages.length - 1]}`
      : "no horizon";
  const copy = chartCopy(view, percentile, span);

  return (
    <SplitPane
      railWidth={296}
      main={
        <div style={{ padding: "22px 24px" }}>
          <SuccessRate
            successRate={stats.successRate}
            iterations={stats.numIterations}
            converged={stats.converged}
            horizonLabel={results.horizonLabel}
          />

          <ChartToolbar
            title={copy.title}
            subtitle={copy.subtitle}
            view={view}
            onViewChange={setView}
            percentile={percentile}
            onCyclePercentile={() => setPercentile((p) => NEXT_PERCENTILE[p])}
          />

          <NetWorthChart
            bands={bands}
            accountSeries={results.accountSeries}
            view={view}
            percentile={percentile}
          />

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "1fr 1fr",
              gap: 28,
              marginTop: 24,
            }}
          >
            <CashFlowTable rows={results.cashFlows} percentile={percentile} />
            <WarningList warnings={results.warnings} />
          </div>
        </div>
      }
      rail={
        <div
          style={{
            padding: "22px 20px",
            display: "flex",
            flexDirection: "column",
            gap: 20,
          }}
        >
          <RunSummary stats={stats} bands={bands} />
          <Hr flush />
          <Button block variant="primary" shortcut="r" onClick={onRun}>
            Re-run
          </Button>
        </div>
      }
    />
  );
}

/** Live progress while the worker pool chews through the iterations. */
function RunProgress({ run, onCancel }: { run: Run | undefined; onCancel: () => void }) {
  const done = run?.completed_iterations ?? 0;
  const total = run?.iterations ?? 0;
  const pct = total > 0 ? Math.min(100, (done / total) * 100) : 0;

  return (
    <div style={{ padding: "40px 24px", maxWidth: 520 }}>
      <h4 style={{ margin: "0 0 8px" }}>
        {run?.status === "queued" ? "Queued…" : "Simulating…"}
      </h4>
      <div style={{ display: "flex", height: 10, border: "1px solid var(--color-divider)" }}>
        <div style={{ width: `${pct}%`, background: "var(--color-accent)" }} />
      </div>
      <p
        style={{
          margin: "8px 0 14px",
          fontSize: 12,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        {done.toLocaleString("en-US")} of {total.toLocaleString("en-US")} iterations
      </p>
      <Button onClick={onCancel}>Cancel run</Button>
    </div>
  );
}
