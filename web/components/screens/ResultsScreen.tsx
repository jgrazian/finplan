"use client";

import { useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  AccountBreakdown,
  CashFlowLedger,
  type ChartView,
  ChartToolbar,
  NetWorthChart,
  RunSummary,
  SuccessRate,
  WarningList,
  useYearFocus,
} from "@/components/results";
import type { ScaleKind } from "@/components/charts";
import { resolveScaleKind } from "@/components/charts/stack";
import { Button, Hr } from "@/components/ui";
import type { Percentile, ResultsData } from "@/lib/types";
import type { Run } from "@/lib/api/types";
import { accountBreakdown } from "@/lib/view/results";
import { EmptyState } from "./EmptyState";

function chartCopy(
  view: ChartView,
  pathLabel: string,
  ages: string,
  dollars: string,
  hasEnvelope: boolean,
) {
  if (view === "stack") {
    return {
      title: "Net worth by account",
      subtitle: `${pathLabel}, balances above zero and debt below · ${ages} · ${dollars}`,
    };
  }
  if (view === "bar") {
    return {
      title: "Net worth by year",
      subtitle: `${pathLabel}, each year split by account · ${ages} · ${dollars}`,
    };
  }
  if (!hasEnvelope) {
    return { title: "Net worth — representative path", subtitle: `${pathLabel} · ${dollars}` };
  }
  return {
    title: "Net worth — envelope and path",
    subtitle: `Pointwise P5–P95 shading, P50 dashed · solid: ${pathLabel} · ${dollars}`,
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
   * Requested nominal-terminal rank. The loaded ResultsData owns the actual
   * path ID/label until a replacement arrives; changing this starts a fetch.
   */
  percentile: Percentile;
  onPercentileChange: (percentile: Percentile) => void;
  onRun: () => void;
  onCancel: () => void;
}) {
  const [view, setView] = useState<ChartView>("fan");
  const [scaleKind, setScaleKind] = useState<ScaleKind>("linear");
  // Hooks run before the early returns below, so the focus is taken against
  // whatever horizon is loaded — zero while there is none.
  const focus = useYearFocus(results?.bands.years.length ?? 0);

  if (error && !results) {
    return <EmptyState title="The run did not finish" detail={error} />;
  }
  if (active) {
    return <RunProgress run={run} onCancel={onCancel} />;
  }
  if (loading && !results) {
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
  // Label real base dates (or nominal legacy units) and the axis explicitly.
  const chartScale = resolveScaleKind(scaleKind, view);
  const dollars = `${results.dollarLabel}${chartScale.kind === "log" ? " · log scale" : ""}`;
  const copy = chartCopy(view, results.pathLabel, span, dollars, results.hasEnvelope);
  const breakdown = accountBreakdown(results.accountSeries, focus.index);

  return (
    <SplitPane
      railWidth={296}
      main={
        <div style={{ padding: "22px 24px" }}>
          <SuccessRate
            successRate={stats.successRate}
            fundingSuccessRate={stats.fundingSuccessRate}
            iterations={stats.numIterations}
          />

          {!results.hasEnvelope && (
            <p role="status">Real envelope unavailable for this result. Rerun to measure all-path real quantiles; only the selected path is shown.</p>
          )}
          {loading && <p role="status">Loading selected path… Showing {results.pathLabel} until its replacement arrives.</p>}
          {error && <p role="alert">Could not load results: {error}. Showing the last loaded path.</p>}
          <ChartToolbar
            title={copy.title}
            subtitle={copy.subtitle}
            view={view}
            onViewChange={setView}
            scaleKind={chartScale.kind}
            logDisabledReason={chartScale.reason}
            onScaleKindChange={setScaleKind}
            percentile={percentile}
            onPercentileChange={onPercentileChange}
          />

          {chartScale.reason && (
            <p style={{ fontSize: 12, marginBottom: 10 }}>{chartScale.reason}</p>
          )}
          <NetWorthChart
            bands={bands}
            accountSeries={results.accountSeries}
            view={view}
            pathValues={results.pathValues}
            pathLabel={results.pathLabel}
            scaleKind={chartScale.kind}
            focus={focus}
          />

          <div style={{ marginTop: 24 }}>
            <CashFlowLedger
              rows={results.cashFlows}
              key={`${results.runId}:${results.pathId}`}
              series={results.pathId}
              pathLabel={results.pathLabel}
              runId={results.runId}
              dollarLabel={results.dollarLabel}
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
          <p style={{ margin: 0, fontSize: 12 }}>{results.pathLabel} · {results.dollarLabel}</p>
          <AccountBreakdown
            breakdown={breakdown}
            year={bands.years[focus.index]}
            age={ageAt(bands, focus.index)}
            hint={focus.hint}
            pinned={focus.pinnedIndex != null}
            onUnpin={focus.unpin}
          />
          <Hr flush />
          <RunSummary
            stats={stats}
            baseDate={results.baseDate}
            pathLabel={results.pathLabel}
            dollarLabel={results.dollarLabel}
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

/**
 * The plan's age at a year, or nothing when the scenario has no birth date to
 * count from — the axis then holds calendar years, and "age 2041" is a lie the
 * panel would otherwise print.
 */
function ageAt(bands: ResultsData["bands"], index: number): number | undefined {
  const age = bands.ages[index];
  return age == null || age === bands.years[index] ? undefined : age;
}
