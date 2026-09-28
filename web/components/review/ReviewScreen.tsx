"use client";

import type React from "react";
import { useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui";
import { api } from "@/lib/api/client";
import { ApiError } from "@/lib/api/http";
import type { Run } from "@/lib/api/types";
import type { ReviewState } from "@/lib/hooks/useReview";
import {
  aiLine,
  board,
  type Card,
  type CardAction,
  type Destination,
  type Names,
  previewLine,
  problemText,
  reviewBanner,
  type StepRow,
  stepProblemText,
  stepProblemsIn,
} from "@/lib/view/review";
import { cardAnchor } from "@/lib/view/chat";
import { type CardOutcome, SuggestionCard } from "./SuggestionCard";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/**
 * The Review tab (design 14a): notes about one run, in three columns, each
 * applying to the plan as an edit the server has already diffed and, where
 * it could, simulated.
 */
export function ReviewScreen({
  state,
  runs,
  planChanged,
  running,
  names,
  offline,
  onPlanChanged,
  onRerun,
  onOpenCopy,
  onNavigate,
}: {
  state: ReviewState;
  /** The scenario's runs, newest first, as the run history holds them. */
  runs: readonly Run[];
  /** The plan was edited after the latest run. */
  planChanged: boolean;
  /** A run of this plan is under way. */
  running: boolean;
  names: Names;
  offline: boolean;
  /** The plan was written to; reload what depends on it. */
  onPlanChanged: () => void;
  /** Start a run of this plan without leaving the tab, as the Run button does. */
  onRerun: () => void;
  /** Open a scenario a stress note was applied to. It is not run. */
  onOpenCopy: (scenarioId: number) => void;
  onNavigate: (to: Destination) => void;
}) {
  const { review, error, reload, reviewRun, reviewing } = state;
  const [busy, setBusy] = useState<number>();
  // What happened when a card's path was last acted on, by "id:path": a
  // preview or a refusal belongs to the path it was made for.
  const [outcomes, setOutcomes] = useState<Record<string, CardOutcome>>({});
  // The path picked on each card that offers several, by suggestion id.
  const [selections, setSelections] = useState<Record<number, string>>({});
  const [notice, setNotice] = useState<string>();
  // Notes applied from this board since it was last reviewed: a stale refusal
  // after one of them is most likely the two editing the same field.
  const appliedHere = useRef(new Set<number>());

  const latest = runs.find((r) => r.status === "succeeded");
  const reviewedRun = review ? runs.find((r) => r.id === review.run_id) : undefined;
  const view = useMemo(
    () =>
      review
        ? board(review, {
            runCreatedAt: reviewedRun?.created_at,
            names,
            latest: latest && { id: latest.id, created_at: latest.created_at },
            selections,
          })
        : undefined,
    [review, reviewedRun?.created_at, names, latest, selections],
  );
  const banner =
    review && view
      ? reviewBanner({
          reviewRunId: review.run_id,
          latestRunId: latest?.id,
          planChanged,
          running,
          applied: view.applied,
          pending: view.pending,
          latestAt: latest?.created_at,
        })
      : undefined;
  const ai = review ? aiLine(review) : undefined;

  const reviewAgain = () => {
    setOutcomes({});
    setSelections({});
    setNotice(undefined);
    appliedHere.current.clear();
    void reviewRun(latest?.id ?? null);
  };

  const rerun = () => {
    setNotice(undefined);
    onRerun();
  };

  const outcomeKey = (id: number, path?: string) => `${id}:${path ?? ""}`;
  const record = (id: number, path: string | undefined, outcome: CardOutcome) =>
    setOutcomes((all) => ({ ...all, [outcomeKey(id, path)]: outcome }));

  /**
   * Act on a card's selected path: `apply-step` applies its steps through
   * `step`; `apply` and `apply-path` all remaining ones; a copy and a preview
   * take the whole path. `steps` names the path's steps for problem wording.
   */
  const act = async (
    id: number,
    action: CardAction,
    path: string | undefined,
    { step, steps = [] }: { step?: string; steps?: readonly StepRow[] } = {},
  ) => {
    if (path == null) return;
    setBusy(id);
    record(id, path, {});
    try {
      switch (action) {
        case "apply":
        case "apply-path":
        case "apply-step": {
          // No run: more steps and notes can be applied first, then one
          // re-run shows them together. The banner offers that run.
          const through = action === "apply-step" ? (step ?? null) : null;
          await api.suggestions.apply(id, { path, through_step: through, to: "plan", name: null });
          appliedHere.current.add(id);
          setNotice(undefined);
          onPlanChanged();
          reload();
          break;
        }
        case "apply-copy": {
          const applied = await api.suggestions.apply(id, { path, through_step: null, to: "copy", name: null });
          record(id, path, { copyId: applied.scenario_id });
          reload();
          break;
        }
        case "preview": {
          const preview = await api.suggestions.preview(id, { path, through_step: null });
          record(id, path,
            preview.problems.length > 0
              ? { problems: preview.problems.map(problemText), stale: preview.problems.some((p) => p.kind === "stale") }
              : { preview: previewLine(preview, Math.max(steps.length, 1)) },
          );
          break;
        }
        case "confirm":
        case "dismiss": {
          await api.suggestions.dismiss(id, { as: action === "confirm" ? "confirmed" : "dismissed" });
          reload();
          break;
        }
      }
    } catch (err) {
      // A refused batch lists its problems: 409 when the plan moved since
      // the note was written, 422 when the edit no longer fits it.
      const problems = err instanceof ApiError ? stepProblemsIn(err.body, path) : [];
      if (problems.length > 0) {
        const stale = err instanceof ApiError && err.status === 409;
        const others = [...appliedHere.current].some((other) => other !== id);
        const applying = action === "apply" || action === "apply-path" || action === "apply-step";
        record(id, path, {
          problems: problems.map((p) => stepProblemText(p, steps)),
          stale,
          conflict: stale && applying && others && problems.some((p) => p.problem.kind === "stale"),
        });
      } else {
        record(id, path, { error: err instanceof Error ? err.message : String(err) });
        // A 409 with nothing listed is a note acted on elsewhere: show where it stands now.
        if (err instanceof ApiError && err.status === 409) reload();
      }
    } finally {
      setBusy(undefined);
    }
  };

  /**
   * A card, anchored so a chat reply can scroll to it, and under it the notes
   * chat replies wrote about it, nested as "From the chat".
   */
  const renderCard = (card: Card): React.ReactNode => (
    <div key={card.id} id={cardAnchor(card.id)} tabIndex={-1} style={{ display: "flex", flexDirection: "column", gap: 10, outline: "none" }}>
      <SuggestionCard
        card={card}
        busy={offline || busy === card.id}
        offline={offline}
        outcome={outcomes[outcomeKey(card.id, card.path)]}
        onSelect={(key) => setSelections((all) => ({ ...all, [card.id]: key }))}
        onAction={(action, step) => void act(card.id, action, card.path, { step, steps: card.steps })}
        onNavigate={onNavigate}
        onReviewAgain={reviewAgain}
        onOpenCopy={onOpenCopy}
        onChatSettled={reload}
      />
      {card.children.length > 0 && (
        <section
          aria-label={`From the chat about: ${card.title}`}
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 10,
            marginLeft: 14,
            paddingLeft: 12,
            borderLeft: "2px solid var(--color-divider)",
          }}
        >
          <div
            style={{
              fontSize: 10.5,
              fontWeight: 600,
              letterSpacing: ".06em",
              textTransform: "uppercase",
              color: MUTED,
            }}
          >
            From the chat
          </div>
          {card.children.map((child) => renderCard(child))}
        </section>
      )}
    </div>
  );

  if (review === undefined && !error) {
    return <p style={{ padding: "24px 20px", margin: 0, fontSize: 13, color: MUTED }}>Loading the review…</p>;
  }

  if (!review || !view) {
    return (
      <div style={{ padding: "40px 24px", maxWidth: 560, display: "flex", flexDirection: "column", gap: 12 }}>
        <h4 style={{ margin: 0 }}>{error ? "Cannot load the review" : "No review yet"}</h4>
        <p style={{ margin: 0, fontSize: 13, lineHeight: 1.5, color: MUTED }}>
          {error
            ? error.message
            : "A review reads the plan and its latest run, and writes notes about each part: what looks off, what to check, and edits you can apply, then re-run together."}
        </p>
        <div>
          <Button
            variant="primary"
            disabled={offline || reviewing || latest == null}
            onClick={reviewAgain}
          >
            {reviewing ? "Reviewing…" : "Review this run"}
          </Button>
        </div>
        {latest == null && (
          <p style={{ margin: 0, fontSize: 12, color: MUTED }}>Run the plan first; a review is written about a finished run.</p>
        )}
      </div>
    );
  }

  return (
    <section aria-label="Review" style={{ display: "flex", flexDirection: "column" }}>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          gap: "8px 14px",
          padding: "10px 20px",
          borderBottom: "1px solid var(--color-divider)",
        }}
      >
        <span style={{ fontSize: 12.5, color: MUTED }}>
          {view.headline}
          {view.resolved > 0 && ` · ${view.resolved} set aside`}
        </span>
        {ai && (
          <span
            role="status"
            aria-live="polite"
            style={{ fontSize: 12.5, color: MUTED, fontStyle: ai.running ? "italic" : undefined }}
          >
            {ai.text}
          </span>
        )}
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          {/* One primary action at a time: the banner's, when it has one. */}
          <Button variant={banner?.action ? "secondary" : "primary"} disabled={offline || reviewing} onClick={reviewAgain}>
            {reviewing ? "Reviewing…" : "Review again"}
          </Button>
        </div>
      </div>

      {(banner || notice || error) && (
        <div
          role="status"
          style={{
            display: "flex",
            flexWrap: "wrap",
            alignItems: "center",
            gap: "6px 14px",
            padding: "10px 20px",
            background: "var(--color-accent-100)",
            borderLeft: "4px solid var(--color-accent-700)",
            color: "var(--color-accent-900)",
            fontSize: 13,
          }}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 4, flex: "1 1 240px" }}>
            {banner && <span>{banner.text}</span>}
            {notice && <span>{notice}</span>}
            {error && <span>{error.message}</span>}
          </div>
          {banner?.action === "rerun" && (
            <Button variant="primary" disabled={offline || running} onClick={rerun}>
              Re-run
            </Button>
          )}
          {banner?.action === "review" && (
            <Button variant="primary" disabled={offline || reviewing} onClick={reviewAgain}>
              {reviewing ? "Reviewing…" : "Review again"}
            </Button>
          )}
        </div>
      )}

      <div
        style={{
          display: "grid",
          // Three columns on a desktop; they wrap to one on a phone.
          gridTemplateColumns: "repeat(auto-fit, minmax(min(280px, 100%), 1fr))",
          gap: 1,
          background: "var(--color-divider)",
        }}
      >
        {view.columns.map((column) => (
          <div
            key={column.section}
            style={{
              padding: "16px 18px",
              display: "flex",
              flexDirection: "column",
              gap: 14,
              background: "var(--color-bg)",
              minWidth: 0,
            }}
          >
            <h6 style={{ margin: 0 }}>
              {column.heading}{" "}
              <span className="text-muted" style={{ letterSpacing: 0 }}>
                {column.cards.length === 1 ? "1 note" : `${column.cards.length} notes`}
              </span>
            </h6>
            {column.cards.length === 0 && (
              <p style={{ margin: 0, fontSize: 12.5, color: MUTED }}>Nothing to raise here.</p>
            )}
            {column.cards.map((card) => renderCard(card))}
          </div>
        ))}
      </div>

      <p style={{ margin: 0, padding: "10px 20px", fontSize: 11.5, color: MUTED, borderTop: "1px solid var(--color-divider)" }}>
        Notes are generated from the plan and the reviewed run. Figures marked “est.” are not simulated until
        you preview them or re-run after applying. Not financial advice.
      </p>
    </section>
  );
}
