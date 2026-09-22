"use client";

import { useState } from "react";
import { Dropdown } from "@/components/ui";
import { Button, Tag } from "@/components/ui";
import { fmtPercent } from "@/lib/format";
import type { AnalysisParameter, SensitivityResults } from "@/lib/api/types";
import { paramId, paramValue, sensitivityView } from "@/lib/view/analysis";

/** The row grid, shared by the header and every row so the columns line up. */
const COLUMNS = "minmax(130px, 1fr) 130px minmax(120px, 1.4fr) 58px 74px";

/**
 * What changes the outcome, ranked — Sweep's entry point.
 *
 * Two runs a parameter, which is cheap enough to answer "what should I even
 * sweep" before committing to a grid. The bar spans the success rate at the two
 * ends of the band, so a parameter that barely moves is a stub and the one
 * worth an axis is a bar you can see from across the room.
 */
export function SensitivityPanel({
  results,
  parameters,
  swept,
  onToggle,
  onSweep,
  canSweep,
  busy,
}: {
  results: SensitivityResults;
  parameters: AnalysisParameter[];
  /** Parameter ids the next sweep will step over. */
  swept: Set<string>;
  onToggle: (parameterId: string) => void;
  onSweep: () => void;
  canSweep: boolean;
  busy?: boolean;
}) {
  const [metric, setMetric] = useState<"funding" | "success">("funding");
  const { rows, low, high } = sensitivityView(results, metric);
  const planRate = metric === "funding" ? results.plan.funding_success_rate : results.plan.success_rate;
  // Every bar shares one axis, so the plan's mark sits at the same place on each.
  const planAt = (high === low ? 0.5 : ((planRate ?? low) - low) / (high - low)) * 200;
  const byId = new Map(parameters.map((p) => [p.id, p]));
  const band = Math.round(results.fraction * 100);

  return (
    <div style={{ padding: "18px 20px 22px" }}>
      <Dropdown ariaLabel="Sensitivity outcome" value={metric} onChange={setMetric} options={[{ value: "funding", label: "Cash funding check" }, { value: "success", label: "Positive ending net worth" }]} />
      {rows.length < results.rows.length && <p>Not measured — rerun to rank all parameters by this outcome.</p>}
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
        <h4 style={{ margin: 0 }}>What changes the outcome</h4>
        <span
          style={{
            fontSize: 11.5,
            color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
          }}
        >
          each parameter ±{band}% · the plan runs at {planRate == null ? "Not measured — rerun" : fmtPercent(planRate)} ·{" "}
          {rows.length} × 2 × {results.iterations.toLocaleString("en-US")} simulations
        </span>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: COLUMNS,
          gap: "0 14px",
          alignItems: "center",
          marginTop: 12,
          paddingBottom: 6,
          borderBottom: "1px solid var(--color-divider)",
          fontSize: 10,
          letterSpacing: ".1em",
          textTransform: "uppercase",
          color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
        }}
      >
        <span>parameter</span>
        <span>range</span>
        <span>
          {metric === "funding" ? "Cash funding" : "Positive ending net worth"} {fmtPercent(low, 0)} → {fmtPercent(high, 0)}
        </span>
        <span>span</span>
        <span>sweep</span>
      </div>

      {rows.map((row) => {
        const parameter = byId.get(row.parameterId);
        return (
          <div
            key={row.parameterId}
            style={{
              display: "grid",
              gridTemplateColumns: COLUMNS,
              gap: "0 14px",
              alignItems: "center",
              padding: "7px 0",
              borderBottom: "1px solid color-mix(in srgb, var(--color-text) 8%, transparent)",
            }}
          >
            <span style={{ fontFamily: "ui-monospace, Menlo, monospace", fontSize: 12 }}>
              {parameter ? paramId(parameter) : row.label}
            </span>
            <span
              style={{
                fontSize: 11.5,
                color: "color-mix(in srgb, var(--color-text) 60%, transparent)",
              }}
            >
              {paramValue(row.kind, row.lowValue)} – {paramValue(row.kind, row.highValue)}
            </span>
            <svg
              viewBox="0 0 200 14"
              preserveAspectRatio="none"
              style={{ width: "100%", height: 14, display: "block" }}
              role="img"
              aria-label={`${fmtPercent(row.lowSuccess)} at the low end, ${fmtPercent(row.highSuccess)} at the high end`}
            >
              <rect x={0} y={0} width={200} height={14} fill="var(--color-text)" fillOpacity={0.05} />
              <rect
                x={row.barStart * 200}
                y={0}
                width={Math.max(1.5, row.barWidth * 200)}
                height={14}
                fill="var(--color-accent)"
                fillOpacity={0.75}
              />
              {/* Where the plan itself sits, so a band is read as a move from
                  somewhere rather than as an absolute. */}
              <line
                x1={planAt}
                x2={planAt}
                y1={0}
                y2={14}
                stroke="var(--color-text)"
                strokeDasharray="2 2"
              />
            </svg>
            <span
              style={{ fontFamily: "var(--font-heading)", fontWeight: 600, fontSize: 14 }}
              title={`${fmtPercent(row.lowSuccess)} to ${fmtPercent(row.highSuccess)}`}
            >
              {row.span.toFixed(1)}
            </span>
            {/* A sweep carries as many variables as you like, so this is a
                set to be in or out of rather than two axis slots to compete
                for — which of them a graph draws is decided afterwards. */}
            <span style={{ display: "flex", gap: 4 }}>
              {swept.has(row.parameterId) ? (
                <Button
                  variant="ghost"
                  style={{ padding: 0, minHeight: 0, border: 0 }}
                  title="Leave it out of the sweep"
                  onClick={() => onToggle(row.parameterId)}
                >
                  <Tag tone="accent">SWEPT</Tag>
                </Button>
              ) : (
                <Button
                  variant="ghost"
                  style={{ padding: "2px 7px", minHeight: 0 }}
                  onClick={() => onToggle(row.parameterId)}
                >
                  ADD
                </Button>
              )}
            </span>
          </div>
        );
      })}

      <div style={{ display: "flex", gap: 12, alignItems: "center", marginTop: 16 }}>
        <span style={{ flex: 1, fontSize: 12.5 }}>
          {swept.size > 0 ? (
            <>
              Sweep{" "}
              {[...swept].map((id, i) => (
                <span key={id}>
                  {i > 0 && " × "}
                  <Mono>{label(byId, id)}</Mono>
                </span>
              ))}
            </>
          ) : (
            "Pick a variable to sweep — the top of the list is where a grid will show you the most."
          )}
        </span>
        <Button variant="primary" onClick={onSweep} disabled={busy || !canSweep}>
          Run sweep
        </Button>
      </div>

      <p
        style={{
          fontSize: 11.5,
          margin: "12px 0 0",
          maxWidth: 640,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
          textWrap: "pretty",
        }}
      >
        The ranking is two runs a parameter, so it answers what to sweep before
        the grid spends thousands of simulations doing it. A parameter that
        has a small measured effect may still matter; sampling uncertainty and interactions are not assessed by this ranking.
      </p>
    </div>
  );
}

function label(byId: Map<string, AnalysisParameter>, id: string | undefined): string {
  const parameter = id == null ? undefined : byId.get(id);
  return parameter ? paramId(parameter) : "—";
}

function Mono({ children }: { children: React.ReactNode }) {
  return (
    <span style={{ fontFamily: "ui-monospace, Menlo, monospace", fontSize: 12 }}>{children}</span>
  );
}
