"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { DraftStatus } from "@/lib/api/generated/DraftStatus";
import { ApiError } from "@/lib/api/http";
import type { Scenario, Suggestion } from "@/lib/api/types";
import { cardAnchor } from "@/lib/view/chat";
import { DRAFT_ACTION_LABEL, draftBoard, type DraftCard } from "@/lib/view/draft";
import {
  type Card,
  type CardAction,
  type Destination,
  type Names,
  stepProblemsIn,
  stepProblemText,
} from "@/lib/view/review";
import { type CardOutcome, SuggestionCard } from "./SuggestionCard";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/**
 * A draft's Review (design 2c): the notes the drafting agent wrote, in
 * Portfolio / Plan / To confirm. "Add to draft" writes a note's changes into
 * the draft; nothing is a plan until Create & run.
 */
export function DraftReview({
  scenario,
  names,
  offline,
  onPlanChanged,
  onCreated,
  onDiscarded,
  onNavigate,
}: {
  scenario: Scenario;
  names: Names;
  offline: boolean;
  /** The draft was written to; reload what depends on it. */
  onPlanChanged: () => void;
  /** Create & run made the draft a plan. */
  onCreated: (created: Scenario) => void;
  /** The draft was deleted. */
  onDiscarded: () => void;
  onNavigate: (to: Destination) => void;
}) {
  const [notes, setNotes] = useState<Suggestion[]>();
  const [status, setStatus] = useState<DraftStatus>();
  const [loadError, setLoadError] = useState<string>();
  const [nonce, setNonce] = useState(0);
  const [busy, setBusy] = useState<number>();
  const [outcomes, setOutcomes] = useState<Record<number, CardOutcome>>({});
  const [selections, setSelections] = useState<Record<number, string>>({});
  const [working, setWorking] = useState<"create" | "discard">();
  const [error, setError] = useState<string>();
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  const id = scenario.id;

  useEffect(() => {
    let live = true;
    Promise.all([api.suggestions.list(id), api.drafts.get(id)]).then(
      ([list, next]) => {
        if (!live) return;
        setNotes(list);
        setStatus(next);
        setLoadError(undefined);
      },
      (err: unknown) => live && setLoadError(err instanceof Error ? err.message : String(err)),
    );
    return () => {
      live = false;
    };
  }, [id, nonce]);

  const board = useMemo(
    () => draftBoard(notes ?? [], { names, questions: status?.questions ?? [], selections }),
    [notes, names, status?.questions, selections],
  );

  const act = async (card: Card, action: CardAction, step?: string) => {
    if (card.path == null) return;
    const path = card.path;
    setBusy(card.id);
    setOutcomes((all) => ({ ...all, [card.id]: {} }));
    try {
      if (action === "confirm" || action === "dismiss") {
        await api.suggestions.dismiss(card.id, { as: action === "confirm" ? "confirmed" : "dismissed" });
      } else {
        await api.suggestions.apply(card.id, {
          path,
          through_step: action === "apply-step" ? (step ?? null) : null,
          to: "plan",
          name: null,
        });
        onPlanChanged();
      }
      reload();
    } catch (err) {
      const problems = err instanceof ApiError ? stepProblemsIn(err.body, path) : [];
      setOutcomes((all) => ({
        ...all,
        [card.id]:
          problems.length > 0
            ? { problems: problems.map((p) => stepProblemText(p, card.steps)), stale: err instanceof ApiError && err.status === 409 }
            : { error: err instanceof Error ? err.message : String(err) },
      }));
      if (err instanceof ApiError && err.status === 409) reload();
    } finally {
      setBusy(undefined);
    }
  };

  const create = async () => {
    setWorking("create");
    setError(undefined);
    try {
      const created = await api.drafts.createAndRun(id);
      onCreated(created.scenario);
    } catch (err) {
      setError(
        err instanceof ApiError && err.status === 409
          ? "The draft is still being written. Try again when it has finished."
          : err instanceof Error
            ? err.message
            : String(err),
      );
      setWorking(undefined);
    }
  };

  const discard = async () => {
    setWorking("discard");
    setError(undefined);
    try {
      await api.drafts.remove(id);
      onDiscarded();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setWorking(undefined);
    }
  };

  if (notes === undefined && !loadError) {
    return <p style={{ padding: "24px 20px", margin: 0, fontSize: 13, color: MUTED }}>Loading the draft…</p>;
  }
  if (notes === undefined) {
    return (
      <div style={{ padding: "40px 24px", maxWidth: 560 }}>
        <h4 style={{ margin: "0 0 6px" }}>Cannot load this draft</h4>
        <p style={{ margin: 0, fontSize: 13, color: MUTED }}>{loadError}</p>
      </div>
    );
  }

  const drafting = status?.state === "drafting";
  const questions = status?.questions.length ?? 0;

  const renderCard = ({ card, badge, waiting }: DraftCard) => (
    <div key={card.id} id={cardAnchor(card.id)} tabIndex={-1} style={{ outline: "none" }}>
      <SuggestionCard
        card={card}
        busy={offline || busy === card.id}
        offline={offline}
        outcome={outcomes[card.id]}
        labels={DRAFT_ACTION_LABEL}
        badge={badge}
        waiting={waiting}
        onSelect={(key) => setSelections((all) => ({ ...all, [card.id]: key }))}
        onAction={(action, step) => void act(card, action, step)}
        onNavigate={onNavigate}
        onReviewAgain={reload}
        onOpenCopy={() => undefined}
        onChatSettled={reload}
      />
    </div>
  );

  return (
    <section aria-label="Draft review" style={{ display: "flex", flexDirection: "column" }}>
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
        <span style={{ fontSize: 12.5, color: MUTED }}>{board.headline || "No notes"}</span>
        <div style={{ marginLeft: "auto", display: "flex", flexWrap: "wrap", gap: 8 }}>
          <Button disabled={offline || working != null} onClick={() => void discard()}>
            {working === "discard" ? "Deleting…" : "Discard draft"}
          </Button>
          <Button variant="primary" disabled={offline || drafting || working != null} onClick={() => void create()}>
            {working === "create" ? "Creating…" : "Create & run"}
          </Button>
        </div>
      </div>

      {(drafting || questions > 0 || error || loadError) && (
        <div
          role="status"
          style={{
            padding: "10px 20px",
            background: "var(--color-accent-100)",
            borderLeft: "4px solid var(--color-accent-700)",
            color: "var(--color-accent-900)",
            fontSize: 13,
            display: "flex",
            flexDirection: "column",
            gap: 4,
          }}
        >
          {drafting && <span>The draft is still being written. Create & run is available once it has finished.</span>}
          {questions > 0 && !drafting && (
            <span>Some notes are waiting on questions you have not answered. They are left out unless you answer them.</span>
          )}
          {error && <span>{error}</span>}
          {loadError && <span>{loadError}</span>}
        </div>
      )}

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(min(280px, 100%), 1fr))",
          gap: 1,
          background: "var(--color-divider)",
        }}
      >
        {board.columns.map((column) => (
          <div
            key={column.column}
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
              <p style={{ margin: 0, fontSize: 12.5, color: MUTED }}>Nothing here.</p>
            )}
            {column.cards.map(renderCard)}
          </div>
        ))}
      </div>

      <p style={{ margin: 0, padding: "10px 20px", fontSize: 11.5, color: MUTED, borderTop: "1px solid var(--color-divider)" }}>
        Notes were written from what you described and attached. Nothing becomes a plan until you Create & run. Not
        financial advice.
      </p>
    </section>
  );
}
