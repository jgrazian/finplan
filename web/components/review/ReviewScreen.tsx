"use client";

import type React from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button, SegmentedControl } from "@/components/ui";
import { api } from "@/lib/api/client";
import { ApiError } from "@/lib/api/http";
import type { AiPlanChat, SuggestionKind } from "@/lib/api/suggestions";
import type { Run, Scenario } from "@/lib/api/types";
import { useNav } from "@/lib/nav";
import type { ReviewState } from "@/lib/hooks/useReview";
import {
  aiLine,
  board,
  type Card,
  type CardAction,
  type Destination,
  type Names,
  type NoteView,
  noteList,
  pickedNote,
  previewLine,
  problemText,
  reviewBanner,
  type StepRow,
  stepProblemText,
  stepProblemsIn,
  steppedNote,
} from "@/lib/view/review";
import { cardAnchor } from "@/lib/view/chat";
import { DraftReview } from "./DraftReview";
import { NoteDetail } from "./NoteDetail";
import { NoteList } from "./NoteList";
import { PlanChat } from "./PlanChat";
import { type CardOutcome, SuggestionCard } from "./SuggestionCard";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/**
 * The Review tab: notes about one run, listed by section beside the one in
 * hand, each applying to the plan as an edit the server has already diffed
 * and, where it could, simulated.
 */
export function ReviewScreen({ draft, ...props }: ReviewScreenProps) {
  return draft ? (
    <DraftReview
      scenario={draft.scenario}
      names={props.names}
      offline={props.offline}
      onPlanChanged={props.onPlanChanged}
      onCreated={draft.onCreated}
      onDiscarded={draft.onDiscarded}
      onNavigate={props.onNavigate}
    />
  ) : (
    <PlanReview {...props} />
  );
}

interface ReviewProps {
  scenarioId: number;
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
  /** Plan chat's month; null or absent when the server has no model. */
  planChat?: AiPlanChat | null;
  /** A plan chat message was spent: read the allowance again. */
  onChatSpent?: () => void;
}

type ReviewSection = "notes" | "chat";

const REVIEW_SECTIONS = [
  { value: "notes" as const, label: "Notes" },
  { value: "chat" as const, label: "Chat" },
];

/**
 * Where the reader was on each scenario's notes: the note in hand and the
 * list's filters. A tab change clears the query (`setTab` starts a tab
 * afresh), so coming back to Review would otherwise land on the first note
 * and lose the one being read or chatted about. Kept for the page's life.
 */
const placeByScenario = new Map<number, { note?: number; show: NoteView; kinds: SuggestionKind[] }>();

/** Every card on the board, nested ones included, by suggestion id. */
function cardsById(columns: readonly { cards: readonly Card[] }[]): Map<number, Card> {
  const out = new Map<number, Card>();
  const add = (card: Card) => {
    out.set(card.id, card);
    card.children.forEach(add);
  };
  columns.forEach((column) => column.cards.forEach(add));
  return out;
}

/** What a draft's Review (design 2c) needs beyond a plan's. */
interface DraftProps {
  scenario: Scenario;
  onCreated: (created: Scenario) => void;
  onDiscarded: () => void;
}

interface ReviewScreenProps extends ReviewProps {
  /** Set when the open scenario is a draft (`status: "draft"`): it has no run to review. */
  draft?: DraftProps;
}

function PlanReview({
  scenarioId,
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
  planChat,
  onChatSpent,
}: ReviewProps) {
  const { review, error, reload, reviewRun, reviewing } = state;
  // The sub-tab is in the query, like Portfolio's. Chat is offered only while
  // the server has a model to answer.
  const nav = useNav();
  const chatAvailable = planChat != null && review?.ai != null;
  const section: ReviewSection = chatAvailable && nav.section === "chat" ? "chat" : "notes";
  const [busy, setBusy] = useState<number>();
  // What happened when a card's path was last acted on, by "id:path": a
  // preview or a refusal belongs to the path it was made for.
  const [outcomes, setOutcomes] = useState<Record<string, CardOutcome>>({});
  // The path picked on each card that offers several, by suggestion id.
  const [selections, setSelections] = useState<Record<number, string>>({});
  const [notice, setNotice] = useState<string>();
  // Which notes the list shows; open ones by default, the ones left to act on.
  const [show, setShow] = useState<NoteView>(() => placeByScenario.get(scenarioId)?.show ?? "open");
  const [kinds, setKinds] = useState<SuggestionKind[]>(() => placeByScenario.get(scenarioId)?.kinds ?? []);
  // The note just acted on stays listed until the user moves on, so handling
  // one in the Open view does not snatch it from under the reader.
  const [kept, setKept] = useState<number>();
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
    // Setting a note aside acts on the note itself: a read note has no path.
    if (action === "dismiss") return resolve(id, action, path);
    if (path == null) return;
    setBusy(id);
    record(id, path, {});
    setKept(id);
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
          // The preview is kept as the path's check: read it back for the figures.
          if (preview.problems.length === 0) reload();
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

  /** Set a note aside, or take that back. */
  const resolve = async (id: number, op: "dismiss" | "reopen", path: string | undefined) => {
    setBusy(id);
    record(id, path, {});
    setKept(id);
    try {
      if (op === "reopen") await api.suggestions.reopen(id);
      else await api.suggestions.dismiss(id, { as: "dismissed" });
      reload();
    } catch (err) {
      record(id, path, { error: err instanceof Error ? err.message : String(err) });
      if (err instanceof ApiError && err.status === 409) reload();
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

  const cards = useMemo(() => (view ? cardsById(view.columns) : new Map<number, Card>()), [view]);

  // The note in hand is the row picked, in the query like every tab's; a pick
  // a newer review no longer has falls back to the first open note.
  const list = useMemo(
    () => (view ? noteList(view, { show, kinds, keep: kept }) : undefined),
    [view, show, kinds, kept],
  );
  // The query's pick wins (a link, a refresh); with none, the note last read here.
  const picked = nav.selection != null ? Number(nav.selection) : placeByScenario.get(scenarioId)?.note;
  const current = list ? pickedNote(list.visible, picked) : undefined;
  useEffect(() => {
    placeByScenario.set(scenarioId, { note: current?.id, show, kinds });
  }, [scenarioId, current?.id, show, kinds]);
  const { setSelection } = nav;
  const pick = useCallback(
    (id: number) => {
      setKept((k) => (k === id ? k : undefined));
      setSelection(String(id));
    },
    [setSelection],
  );
  const step = useCallback(
    (direction: 1 | -1) => {
      const next = list && steppedNote(list.visible, current?.id, direction);
      if (next) pick(next.id);
    },
    [list, current?.id, pick],
  );

  // j and k step through the notes, as the Next note keycap says.
  useEffect(() => {
    if (section !== "notes") return;
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && (/^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName) || el.isContentEditable)) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "j") step(1);
      else if (e.key === "k") step(-1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [section, step]);
  // A note a chat reply wrote, as its card; one set aside since, in words.
  const renderNote = (id: number) => {
    const card = cards.get(id);
    if (card) return renderCard(card);
    return (
      <p style={{ margin: 0, fontSize: 12.5, color: MUTED }}>
        This note is no longer on the review: it was set aside, or a newer review replaced it.
      </p>
    );
  };

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

  const position = current && list ? `${list.all.indexOf(current) + 1} of ${list.all.length}` : "";

  return (
    <section aria-label="Review" style={{ display: "flex", flexDirection: "column" }}>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          gap: "8px 16px",
          padding: "10px 20px",
          borderBottom: "1px solid var(--color-divider)",
        }}
      >
        {chatAvailable && (
          <SegmentedControl
            ariaLabel="Review section"
            options={REVIEW_SECTIONS}
            value={section}
            onChange={nav.setSection}
          />
        )}
        <span style={{ fontSize: 13, color: MUTED }}>
          {section === "chat"
            ? "Ask about the plan or for a change; changes come back as notes for you to apply."
            : view.headline}
        </span>
        {ai && (
          <span
            role="status"
            aria-live="polite"
            style={{ fontSize: 13, color: MUTED, fontStyle: ai.running ? "italic" : undefined }}
          >
            {ai.text}
          </span>
        )}
        <div style={{ marginLeft: "auto", display: "flex", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
          {(banner || notice || error) && (
            <div
              role="status"
              style={{
                display: "flex",
                flexWrap: "wrap",
                alignItems: "center",
                gap: "6px 12px",
                padding: banner?.action ? "4px 4px 4px 12px" : "6px 12px",
                background: "var(--color-accent-100)",
                border: "1px solid color-mix(in srgb, var(--color-accent) 25%, transparent)",
                borderRadius: 9,
                color: "var(--color-accent-700)",
                fontSize: 13,
              }}
            >
              <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                {banner && <span>{banner.text}</span>}
                {notice && <span>{notice}</span>}
                {error && <span>{error.message}</span>}
              </span>
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
          {/* One primary action at a time: the banner's, when it has one. */}
          {banner?.action !== "review" && (
            <Button variant={banner?.action ? "ghost" : "secondary"} disabled={offline || reviewing} onClick={reviewAgain}>
              {reviewing ? "Reviewing…" : "Review again"}
            </Button>
          )}
        </div>
      </div>

      {section === "chat" && planChat ? (
        <PlanChat
          scenarioId={scenarioId}
          allowance={planChat}
          offline={offline}
          renderNote={renderNote}
          onSettled={reload}
          onSpent={() => onChatSpent?.()}
        />
      ) : (
        list && (
          <div className="review-notes">
            <NoteList
              list={list}
              show={show}
              onShow={(next) => {
                setKept(undefined);
                setShow(next);
              }}
              onToggleKind={(kind) =>
                setKinds((all) => (all.includes(kind) ? all.filter((k) => k !== kind) : [...all, kind]))
              }
              onClearKinds={() => setKinds([])}
              selected={current?.id}
              onPick={pick}
            />
            <main className="review-notes-main">
              {current ? (
                <NoteDetail
                  key={current.id}
                  card={current}
                  position={position}
                  busy={offline || busy === current.id}
                  offline={offline}
                  outcome={outcomes[outcomeKey(current.id, current.path)]}
                  onSelect={(key) => setSelections((all) => ({ ...all, [current.id]: key }))}
                  onAction={(action, stepKey) =>
                    void act(current.id, action, current.path, { step: stepKey, steps: current.steps })
                  }
                  onUndo={() => void resolve(current.id, "reopen", current.path)}
                  onNext={() => step(1)}
                  onNavigate={onNavigate}
                  onReviewAgain={reviewAgain}
                  onOpenCopy={onOpenCopy}
                  onChatSettled={reload}
                />
              ) : (
                <p style={{ margin: 0, fontSize: 14, color: MUTED }}>
                  Nothing to raise: this review has no notes about the run.
                </p>
              )}
            </main>
          </div>
        )
      )}

      <p style={{ margin: 0, padding: "10px 20px", fontSize: 11.5, color: MUTED, borderTop: "1px solid var(--color-divider)" }}>
        Notes are generated from the plan and the reviewed run. Figures marked “est.” are not simulated until
        you preview them or re-run after applying. Not financial advice.
      </p>
    </section>
  );
}
