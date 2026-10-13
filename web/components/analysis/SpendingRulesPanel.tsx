"use client";

import { useEffect, useMemo, useState } from "react";
import { EmptyState } from "@/components/screens/EmptyState";
import { Button } from "@/components/ui";
import type { Scenario, SpendingComparison } from "@/lib/api/types";
import { ApiError } from "@/lib/api/http";
import { fmtCompact, fmtPercent } from "@/lib/format";
import { useAsync } from "@/lib/hooks/useAsync";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { usePlanApi } from "@/lib/nav";
import { ruleChart, ruleLines, withSpending } from "@/lib/view/spending";
import { SpendingRulesChart } from "./SpendingRulesChart";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/**
 * Analysis > Spending (spec 22): the plan's spending under each rule, a
 * small Monte Carlo per rule on the same markets. Laid out like Drawdown's
 * strategy comparison; Apply writes the chosen rule into the plan's
 * spending event.
 */
export function SpendingRulesPanel({
  scenario,
  onPlanChanged,
}: {
  scenario: Scenario;
  onPlanChanged: () => void;
}) {
  const api = usePlanApi();
  const runs = useAsync(() => api.runs.list(scenario.id), [api, scenario.id]);
  const run = runs.data?.find((r) => r.status === "succeeded");

  const [compared, setCompared] = useState<{ run: number; data?: SpendingComparison; error?: unknown }>();
  useEffect(() => {
    if (!run) return;
    const abort = new AbortController();
    api.runs
      .spendingCompare(run.id, {}, abort.signal)
      .then((data) => setCompared({ run: run.id, data }))
      .catch((error: unknown) => {
        if (!abort.signal.aborted) setCompared({ run: run.id, error });
      });
    return () => abort.abort();
  }, [api, run]);
  const current = compared?.run === run?.id ? compared : undefined;
  const chart = useMemo(() => (current?.data ? ruleChart(current.data) : undefined), [current]);
  // The rule whose band the chart shades; As planned to begin with.
  const [selected, setSelected] = useState(0);

  const [notice, setNotice] = useState<string>();
  const submit = useSubmit();

  if (runs.loading && !runs.data) return <EmptyState title="Loading…" detail="Finding the plan’s latest run." />;
  if (runs.error) return <EmptyState title="Cannot read this plan’s runs" detail={runs.error.message} />;
  if (!run) {
    return (
      <EmptyState
        title="Run the plan first"
        detail="Spending rules are compared against a finished run. Run the plan from the Results tab, then come back."
      />
    );
  }
  if (current?.error) {
    const error = current.error;
    const unprocessable = error instanceof ApiError && error.status === 422;
    const stale = error instanceof ApiError && error.status === 409;
    return (
      <EmptyState
        title={
          unprocessable
            ? "No retirement spending to compare"
            : stale
              ? "Run the plan again to compare spending rules."
              : "The comparison could not finish"
        }
        detail={
          unprocessable
            ? "Give the plan a spending rule, or an expense event named for retirement (monthly, quarterly or yearly), on the Plan tab."
            : error instanceof Error
              ? error.message
              : String(error)
        }
      />
    );
  }
  const comparison = current?.data;
  if (!comparison) {
    return <EmptyState title="Comparing spending rules…" detail="Running each rule over the same simulated markets." />;
  }

  const apply = (index: number) => {
    const row = comparison.rows[index];
    if (!row.effect) return;
    const effect = row.effect;
    setNotice(undefined);
    submit.run(
      async () => {
        const events = await api.events.list(scenario.id);
        const event = events.find((e) => e.id === comparison.event_id);
        if (!event) throw new Error(`${comparison.event_name} is no longer in the plan.`);
        await api.events.replace(scenario.id, event.id, withSpending(event, effect));
      },
      () => {
        setNotice(`${comparison.event_name} now spends by ${row.label}. Run the plan to see it in Results.`);
        onPlanChanged();
      },
    );
  };

  const lines = ruleLines(comparison);
  const best = (on: boolean) =>
    on
      ? {
        fontWeight: 700,
        color: "var(--color-accent-800)",
        background: "color-mix(in srgb, var(--color-accent) 12%, transparent)",
      }
      : {};
  const cell = { textAlign: "right" as const, padding: "4px 10px" };

  return (
    <div
      style={{
        padding: "22px 24px",
        display: "flex",
        flexDirection: "column",
        gap: 14,
        width: "100%",
        maxWidth: 980,
        margin: "0 auto",
        boxSizing: "border-box",
      }}
    >
      <div style={{ fontSize: 13, lineHeight: 1.5 }}>
        <strong>{comparison.event_name}</strong> under each rule, starting at the plan’s first-year draw of{" "}
        {fmtCompact(comparison.first_draw)} ({fmtPercent(comparison.initial_rate, 1)} of the portfolio when it
        starts). Figures are in today’s dollars.
      </div>
      {chart && <SpendingRulesChart chart={chart} selected={selected} onSelect={setSelected} />}
      <section aria-label="Spending rule comparison" style={{ fontSize: 12.5 }}>
        <div style={{ color: MUTED, marginBottom: 6 }}>
          {comparison.iterations} markets each, the same markets for every rule
        </div>
        <div style={{ overflowX: "auto" }}>
          <table style={{ borderCollapse: "collapse", minWidth: "100%" }}>
            <thead>
              <tr style={{ color: MUTED, textAlign: "right" }}>
                <th style={{ textAlign: "left", fontWeight: 500, padding: "2px 10px 4px 0" }}>Rule</th>
                <th style={{ fontWeight: 500, padding: "2px 10px 4px" }}>Cash never short</th>
                <th style={{ fontWeight: 500, padding: "2px 10px 4px" }}>Median lifetime spending</th>
                <th style={{ fontWeight: 500, padding: "2px 10px 4px" }}>1 in 10 markets, the draw dips to</th>
                <th style={{ fontWeight: 500, padding: "2px 10px 4px" }}>Cuts deeper than 10%</th>
                <th style={{ fontWeight: 500, padding: "2px 10px 4px" }}>Median ending balance</th>
                <th aria-label="Apply" />
              </tr>
            </thead>
            <tbody>
              {lines.map((line, i) => (
                <tr
                  key={line.key}
                  style={{
                    borderTop: "1px solid var(--color-divider)",
                    background: i === selected ? "color-mix(in srgb, var(--color-text) 4%, transparent)" : undefined,
                  }}
                >
                  <td style={{ padding: "4px 10px 4px 0", whiteSpace: "nowrap" }}>
                    <button
                      type="button"
                      aria-pressed={i === selected}
                      title="Show this rule's range on the chart"
                      onClick={() => setSelected(i)}
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        gap: 6,
                        padding: 0,
                        border: 0,
                        background: "none",
                        color: "inherit",
                        font: "inherit",
                        fontWeight: i === selected ? 600 : undefined,
                        cursor: "pointer",
                      }}
                    >
                      {chart?.series[i] && (
                        <svg width={14} height={8} aria-hidden style={{ flex: "none" }}>
                          <line
                            x1={0}
                            x2={14}
                            y1={4}
                            y2={4}
                            stroke={chart.series[i].color}
                            strokeWidth={2}
                            strokeDasharray={chart.series[i].dashed ? "4 3" : undefined}
                          />
                        </svg>
                      )}
                      {line.label}
                    </button>
                  </td>
                  <td style={{ ...cell, ...best(line.bestFunding) }}>
                    {line.row.funding_success_rate == null ? "—" : fmtPercent(line.row.funding_success_rate, 1)}
                  </td>
                  <td style={{ ...cell, ...best(line.bestLifetime) }}>
                    {fmtCompact(line.row.median_lifetime_spending)}
                  </td>
                  <td style={{ ...cell, ...best(line.bestLowest) }}>{fmtCompact(line.row.p10_lowest_draw)}/yr</td>
                  <td style={{ ...cell, ...best(line.fewestCuts) }}>{fmtPercent(line.row.cut_rate, 0)}</td>
                  <td style={cell}>
                    {fmtCompact(line.row.median_final_net_worth_real ?? line.row.median_final_net_worth)}
                  </td>
                  <td style={{ padding: "2px 0 2px 10px", textAlign: "right" }}>
                    {line.row.effect && (
                      <Button
                        variant="ghost"
                        style={{ fontSize: 12 }}
                        disabled={submit.busy}
                        onClick={() => apply(i)}
                      >
                        Apply
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
      {notice && <p role="status" style={{ margin: 0, fontSize: 12.5 }}>{notice}</p>}
      {submit.error && <p role="alert" style={{ margin: 0, fontSize: 12.5 }}>{submit.error}</p>}
      <p style={{ margin: 0, fontSize: 12, color: MUTED, lineHeight: 1.5 }}>
        A rule that adjusts rarely runs out, so it is judged on how much it spends and how deep its cuts go,
        beside whether cash is ever short. Percent of portfolio and Capped change start at the plan’s rate;
        VPW spends the portfolio down by its final age. Apply writes the rule into {comparison.event_name}; its
        floor, ceiling and other terms are on the Plan tab.
      </p>
    </div>
  );
}
