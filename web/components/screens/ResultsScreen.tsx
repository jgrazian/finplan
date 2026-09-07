"use client";

import { useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  CashFlowLedger,
  type ChartView,
  ChartToolbar,
  NetWorthChart,
  RunSummary,
  SuccessRate,
  WarningList,
} from "@/components/results";
import type { ScaleKind } from "@/components/charts";
import { Button, Hr } from "@/components/ui";
import type { Percentile, ResultsData } from "@/lib/types";
import type { Run } from "@/lib/api/types";
import { EmptyState } from "./EmptyState";

const NEXT_PERCENTILE: Record<Percentile, Percentile> = {
  p50: "p95",
  p95: "p5",
  p5: "p50",
};

function chartCopy(
  view: ChartView,
  percentile: Percentile,
  ages: string,
  dollars: string,
) {
  if (view === "stack") {
    return {
      title: "Net worth by account",
      subtitle: `${percentile.toUpperCase()} run, stacked to the total · ${ages} · ${dollars}`,
    };
  }
  if (view === "bar") {
    return {
      title: "Net worth by year",
      subtitle: `${percentile.toUpperCase()} run, each year split by account · ${ages} · ${dollars}`,
    };
  }
  return {
    title: "Net worth — P5 / P50 / P95 band",
    subtitle: `shaded band spans the 5th to 95th percentile run · ${dollars}`,
  };
}

/** Results tab: the run's success rate, its net-worth paths and its warnings. */
export function ResultsScreen({
  results,
  run,
  active,
  loading,
  error,
  percentile,
  onPercentileChange,
  onRun,
  onCancel,
}: {
  results: ResultsData | undefined;
  run: Run | undefined;
  /** A run is queued or executing right now. */
  active: boolean;
  loading: boolean;
  error: string | undefined;
  /**
   * Which path is on screen. Owned by the run rather than by this screen: the
   * composition, the cash flows and the ledger all come from the server one
   * path at a time, so changing it is a fetch.
   */
  percentile: Percentile;
  onPercentileChange: (percentile: Percentile) => void;
  onRun: () => void;
  onCancel: () => void;
}) {
  const [view, setView] = useState<ChartView>("fan");
  const [scaleKind, setScaleKind] = useState<ScaleKind>("linear");

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
  // Every figure on this screen is real, so the chart says so once rather than
  // every panel repeating it.
  // The axis is called out in the subtitle: a log chart read as a linear one
  // flatters every plan, so the frame has to say which one it is drawing.
  const dollars =
    scaleKind === "log"
      ? `${results.baseYear} dollars · log scale`
      : `${results.baseYear} dollars`;
  const copy = chartCopy(view, percentile, span, dollars);

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
            scaleKind={scaleKind}
            onScaleKindChange={setScaleKind}
            percentile={percentile}
            onCyclePercentile={() => onPercentileChange(NEXT_PERCENTILE[percentile])}
          />

          <NetWorthChart
            bands={bands}
            accountSeries={results.accountSeries}
            view={view}
            percentile={percentile}
            scaleKind={scaleKind}
          />

          <div style={{ marginTop: 24 }}>
            <CashFlowLedger
              rows={results.cashFlows}
              percentile={percentile}
              runId={run?.id}
              baseYear={results.baseYear}
            />
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
          <RunSummary
            stats={stats}
            bands={bands}
            baseYear={results.baseYear}
            totalInflation={results.totalInflation}
          />
          <Hr flush />
          <WarningList warnings={results.warnings} />
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
