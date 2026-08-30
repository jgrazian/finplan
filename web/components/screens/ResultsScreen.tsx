"use client";

import { useMemo, useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  CashFlowTable,
  type ChartView,
  ChartToolbar,
  NetWorthChart,
  RunSummary,
  SuccessRate,
  WarningList,
  WhatIfPanel,
  type WhatIfOverrides,
} from "@/components/results";
import { Hr } from "@/components/ui";
import { buildAccountSeries, buildBands, MOCK_RESULTS } from "@/lib/mock/results";
import type { Percentile } from "@/lib/types";

const NEXT_PERCENTILE: Record<Percentile, Percentile> = {
  p50: "p95",
  p95: "p5",
  p5: "p50",
};

function chartCopy(view: ChartView, percentile: Percentile, ages: number[]) {
  if (view === "stack") {
    return {
      title: "Net worth by account",
      subtitle: `${percentile.toUpperCase()} representative run · ages ${ages[0]}–${ages[ages.length - 1]}`,
    };
  }
  if (view === "bar") {
    return {
      title: "Net worth by year",
      subtitle: `${percentile.toUpperCase()} representative run · ages ${ages[0]}–${ages[ages.length - 1]}`,
    };
  }
  return {
    title: "Net worth — P5 / P50 / P95 band",
    subtitle: "shaded band spans the 5th to 95th percentile run",
  };
}

/** Artboard 1a — one chart frame, four treatments, live percentile switch. */
export function ResultsScreen() {
  const [view, setView] = useState<ChartView>("fan");
  const [percentile, setPercentile] = useState<Percentile>("p50");
  const [overrides, setOverrides] = useState<WhatIfOverrides>({
    retirementAge: 62,
    annualSpendK: 145,
  });
  const [applied, setApplied] = useState<WhatIfOverrides | null>(null);

  const results = MOCK_RESULTS;

  // Re-runs only when overrides are applied, so dragging a slider does not
  // imply the plan has been re-simulated.
  const { bands, accountSeries } = useMemo(() => {
    if (!applied) {
      return { bands: results.bands, accountSeries: results.accountSeries };
    }
    const next = buildBands({
      retirementAge: applied.retirementAge,
      annualSpend: applied.annualSpendK * 1000,
    });
    return { bands: next, accountSeries: buildAccountSeries(next.p50) };
  }, [applied, results]);

  const copy = chartCopy(view, percentile, bands.ages);

  return (
    <SplitPane
      railWidth={296}
      main={
        <div style={{ padding: "22px 24px" }}>
          <SuccessRate
            successRate={results.stats.successRate}
            iterations={results.stats.numIterations}
            converged={results.stats.converged}
            finalAge={results.finalAge}
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
            accountSeries={accountSeries}
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
          <RunSummary stats={results.stats} bands={bands} />
          <Hr flush />
          <WhatIfPanel
            value={overrides}
            onChange={setOverrides}
            onRerun={() => setApplied(overrides)}
          />
        </div>
      }
    />
  );
}
