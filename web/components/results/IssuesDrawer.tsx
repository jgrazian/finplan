"use client";

import { useEffect, type ReactNode } from "react";
import { Tag } from "@/components/ui";
import type { PreflightIssue } from "@/lib/api/types";
import type { Percentile } from "@/lib/types";
import type { IssueSummary, PathCheck } from "@/lib/view/issues";

const MUTED = "color-mix(in srgb, var(--color-text) 70%, transparent)";

/**
 * Everything the strip summarises, in a drawer over the right of the results
 * (design 14a): what stopped the last run, what blocks the next one, what is
 * worth a second look, and how many iterations ran short of cash.
 */
export function IssuesDrawer({
  summary,
  percentile,
  onClose,
  onRun,
  onReviewIssue,
  onReviewEvent,
  onPercentileChange,
}: {
  summary: IssueSummary;
  percentile: Percentile;
  onClose: () => void;
  onRun: () => void;
  onReviewIssue: (issue: PreflightIssue) => void;
  /** Open an event, by row id, in Plan. */
  onReviewEvent: (eventId: number) => void;
  onPercentileChange: (percentile: Percentile) => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const { runError, blocking, review, shortfall, eventFailures, checks } = summary;
  const onPath = eventFailures.length + checks.length;
  const path = summary.pathLabel ?? "path shown";
  const heading = [
    blocking.length > 0 && `${blocking.length} blocking`,
    review.length > 0 && `${review.length} to review`,
    (runError || shortfall) && `${(runError ? 1 : 0) + (shortfall ? 1 : 0)} from the last run`,
    onPath > 0 && `${onPath} on the ${path}`,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <aside
      id="results-issues"
      aria-label="Issues"
      className="issues-drawer"
      style={{
        position: "absolute",
        top: 0,
        right: 0,
        zIndex: 30,
        width: "min(420px, 100%)",
        maxHeight: "100%",
        overflowY: "auto",
        background: "var(--color-bg)",
        borderLeft: "1px solid var(--color-divider)",
        borderBottom: "1px solid var(--color-divider)",
        boxShadow: "var(--shadow-lg)",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 10,
          padding: "14px 18px",
          borderBottom: "1px solid var(--color-divider)",
        }}
      >
        <h4 style={{ margin: 0, fontSize: 20 }}>Issues</h4>
        <span className="text-muted" style={{ fontSize: 12 }}>
          {heading || "Nothing to report"}
        </span>
        <button
          className="btn btn-ghost btn-icon"
          type="button"
          aria-label="Close issues"
          style={{ marginLeft: "auto", width: 28, height: 28 }}
          onClick={onClose}
        >
          ×
        </button>
      </div>

      {runError && (
        <div
          style={{
            padding: "12px 18px",
            background: "var(--color-accent-900)",
            color: "var(--color-bg)",
            display: "flex",
            flexDirection: "column",
            gap: 6,
          }}
        >
          <span
            style={{
              fontFamily: "var(--font-heading)",
              fontWeight: 600,
              letterSpacing: ".06em",
              textTransform: "uppercase",
              fontSize: 12,
            }}
          >
            Run error
          </span>
          <span style={{ fontSize: 13, textWrap: "pretty" }}>{runError.message}</span>
          {runError.detail && (
            <code style={{ color: "inherit", fontSize: 12, whiteSpace: "pre-wrap" }}>
              {runError.detail}
            </code>
          )}
          <div>
            <button className="sbtn" type="button" onClick={onRun}>
              Run again
            </button>
          </div>
        </div>
      )}

      {blocking.length > 0 && (
        <Group title="Blocks the run">
          {blocking.map((issue, i) => (
            <IssueRow key={`${issue.code}:${i}`} issue={issue} tone="accent" onReview={onReviewIssue} />
          ))}
        </Group>
      )}

      {review.length > 0 && (
        <Group title="Worth checking">
          {review.map((issue, i) => (
            <IssueRow key={`${issue.code}:${i}`} issue={issue} tone="outline" onReview={onReviewIssue} />
          ))}
        </Group>
      )}

      {shortfall && (
        <Group title="From the last run">
          <div style={{ padding: "10px 18px 14px", display: "flex", flexDirection: "column", gap: 8 }}>
            <div style={{ display: "flex", gap: 8, alignItems: "baseline" }}>
              <Tag tone="outline">Shortfall</Tag>
              <b style={{ fontSize: 13.5 }}>
                {shortfall.failed.toLocaleString("en-US")}{" "}
                {shortfall.failed === 1 ? "iteration" : "iterations"} failed the cash funding check
              </b>
            </div>
            <table className="table" style={{ fontSize: 12 }}>
              <tbody>
                <tr>
                  <td>Iterations short</td>
                  <td style={{ textAlign: "right" }}>
                    {shortfall.failed.toLocaleString("en-US")} of{" "}
                    {shortfall.total.toLocaleString("en-US")}
                  </td>
                </tr>
                <tr>
                  <td>First shortfall, shown path</td>
                  <td style={{ textAlign: "right" }}>{shortfall.firstInPath ?? "None"}</td>
                </tr>
                <tr>
                  <td>Funding warnings, shown path</td>
                  <td style={{ textAlign: "right" }}>{shortfall.pathWarnings}</td>
                </tr>
              </tbody>
            </table>
            <p style={{ margin: 0, fontSize: 12, color: MUTED, textWrap: "pretty" }}>
              An iteration fails when a cash account falls below zero after the year&apos;s
              activity settles, or when a planned event cannot be processed. Investments
              elsewhere do not fund that account on their own, so a positive ending net worth
              can still fail. Add or adjust a transfer or withdrawal rule if that is how you
              intend to pay for it.
            </p>
            <p style={{ margin: 0, fontSize: 12, color: MUTED, textWrap: "pretty" }}>
              Paths that fail usually sit low in the envelope. The P10 path is the likeliest
              of the stored paths to show where cash runs out.
            </p>
            {percentile !== "p10" && (
              <div>
                <button className="sbtn" type="button" onClick={() => onPercentileChange("p10")}>
                  Show P10 path
                </button>
              </div>
            )}
          </div>
        </Group>
      )}

      {eventFailures.length > 0 && (
        <Group title={`Events that failed · ${path}`}>
          {eventFailures.map((failure) => (
            <div
              key={failure.key}
              style={{
                padding: "10px 18px 12px",
                borderBottom: "1px solid var(--color-divider)",
                display: "flex",
                flexDirection: "column",
                gap: 6,
              }}
            >
              <div style={{ display: "flex", gap: 8, alignItems: "baseline" }}>
                <Tag tone="accent">Event</Tag>
                <b style={{ fontSize: 13.5 }}>{failure.event}</b>
                {failure.date && (
                  <span className="text-muted" style={{ fontSize: 12, marginLeft: "auto" }}>
                    {failure.date}
                  </span>
                )}
              </div>
              <span style={{ fontSize: 12.5, color: MUTED, textWrap: "pretty" }}>
                {failure.message}
              </span>
              {failure.eventId != null && (
                <div>
                  <LinkButton onClick={() => onReviewEvent(failure.eventId!)}>
                    Edit event in Plan →
                  </LinkButton>
                </div>
              )}
            </div>
          ))}
        </Group>
      )}

      {checks.length > 0 && (
        <Group title={`Worth a look · ${path}`}>
          {checks.map((check) => (
            <div
              key={check.code}
              style={{
                padding: "10px 18px 12px",
                borderBottom: "1px solid var(--color-divider)",
                display: "flex",
                flexDirection: "column",
                gap: 6,
              }}
            >
              <div style={{ display: "flex", gap: 8, alignItems: "baseline" }}>
                <Tag tone="outline">{CHECK_TAG[check.code]}</Tag>
                <b style={{ fontSize: 13.5 }}>{check.title}</b>
              </div>
              <span style={{ fontSize: 12.5, color: MUTED, textWrap: "pretty" }}>
                {check.detail}
              </span>
            </div>
          ))}
        </Group>
      )}

      {summary.count === 0 && (
        <p style={{ margin: 0, padding: "14px 18px", fontSize: 13, color: MUTED }}>
          The plan passes its checks and every iteration stayed funded.
        </p>
      )}
    </aside>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section aria-label={title}>
      <div style={{ padding: "12px 18px 4px" }}>
        <span
          style={{
            font: "600 10px ui-monospace, Menlo, monospace",
            letterSpacing: ".06em",
            textTransform: "uppercase",
            color: "color-mix(in srgb, var(--color-text) 45%, transparent)",
          }}
        >
          {title}
        </span>
      </div>
      {children}
    </section>
  );
}

const CHECK_TAG: Record<PathCheck["code"], string> = {
  "withdrawal-rate": "Withdrawals",
  "idle-cash": "Cash",
};

function LinkButton({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        background: "none",
        border: 0,
        padding: 0,
        font: "inherit",
        fontSize: 12.5,
        color: "var(--color-accent-700)",
        textDecoration: "underline",
        cursor: "pointer",
      }}
    >
      {children}
    </button>
  );
}

const SECTION_LABEL: Record<string, string> = {
  plan: "Plan",
  portfolio: "Portfolio",
};

function IssueRow({
  issue,
  tone,
  onReview,
}: {
  issue: PreflightIssue;
  tone: "accent" | "outline";
  onReview: (issue: PreflightIssue) => void;
}) {
  const where = SECTION_LABEL[issue.section] ?? issue.section;
  return (
    <div
      style={{
        padding: "10px 18px 12px",
        borderBottom: "1px solid var(--color-divider)",
        display: "flex",
        flexDirection: "column",
        gap: 6,
      }}
    >
      <div style={{ display: "flex", gap: 8, alignItems: "baseline" }}>
        <Tag tone={tone}>{where}</Tag>
        <span style={{ fontSize: 13, textWrap: "pretty" }}>{issue.message}</span>
      </div>
      <div>
        <LinkButton onClick={() => onReview(issue)}>Review in {where} →</LinkButton>
      </div>
    </div>
  );
}
