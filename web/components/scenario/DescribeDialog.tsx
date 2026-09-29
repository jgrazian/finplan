"use client";

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { Button, CurrencyInput, DateInput, Dialog, Input, SegmentedControl, Tag } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { AiDrafts } from "@/lib/api/generated/AiDrafts";
import type { DocumentManifest } from "@/lib/api/generated/DocumentManifest";
import type { DraftQuestion } from "@/lib/api/generated/DraftQuestion";
import type { DraftStatus } from "@/lib/api/generated/DraftStatus";
import type { Entitlements } from "@/lib/api/generated/Entitlements";
import { ApiError } from "@/lib/api/http";
import type { Scenario } from "@/lib/api/types";
import {
  answersOf,
  CONSENT_LINES,
  documentNote,
  DRAFT_POLL_MS,
  draftPanel,
  fileSize,
  limitsLine,
  quotaBlock,
  quotaLine,
  RETENTION_OPTIONS,
  type Retention,
} from "@/lib/view/draft";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";
const BOX = { border: "1px solid var(--color-divider)", padding: "10px 12px" } as const;

/** How many polls in a row may fail before the panel says it has lost touch. */
const POLL_FAILURES_SHOWN = 3;

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/**
 * New scenario, Describe & upload (design 2a): a description and documents on
 * the left, the draft filling in on the right. Nothing here is a plan until
 * Create & run; closing the dialog (or going back to Guided) deletes the
 * draft, which is not resumable.
 */
export function DescribeDialog({
  access,
  drafts,
  modeSwitch,
  onClose,
  onCreated,
  onReviewDraft,
}: {
  access: Entitlements;
  drafts: AiDrafts;
  modeSwitch: ReactNode;
  onClose: () => void;
  onCreated: (scenario: Scenario) => void;
  onReviewDraft: (draft: Scenario) => void;
}) {
  const [description, setDescription] = useState("");
  const [retention, setRetention] = useState<Retention>("delete");
  const [status, setStatus] = useState<DraftStatus>();
  const [documents, setDocuments] = useState<DocumentManifest[]>([]);
  const [started, setStarted] = useState(false);
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [working, setWorking] = useState<"attach" | "start" | "answer" | "create" | "review">();
  const [error, setError] = useState<string>();
  const [pollFailures, setPollFailures] = useState(0);
  const [gone, setGone] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);

  // The draft exists from the first file or the first "Draft it", not from
  // opening the tab: creating one spends one of the month's drafts.
  const draftId = useRef<number | undefined>(undefined);
  const creating = useRef<Promise<number> | undefined>(undefined);
  const handedOff = useRef(false);

  const ensureDraft = useCallback((keep: boolean): Promise<number> => {
    if (draftId.current != null) return Promise.resolve(draftId.current);
    creating.current ??= api.drafts
      .create({ retain_documents: keep })
      .then((created) => {
        draftId.current = created.id;
        setStatus(created);
        return created.id;
      })
      .finally(() => {
        creating.current = undefined;
      });
    return creating.current;
  }, []);

  /** Delete the draft unless it was handed on (to Review or to Create & run). */
  const discard = useCallback(() => {
    if (handedOff.current) return;
    const pending = creating.current;
    void (async () => {
      try {
        await pending;
      } catch {
        return;
      }
      const id = draftId.current;
      if (id != null && !handedOff.current) await api.drafts.remove(id);
    })().catch(() => undefined);
  }, []);

  // Closing, and leaving for Guided, both unmount this: one place deletes.
  useEffect(() => discard, [discard]);

  // A stable close, so the dialog's focus effect does not run on every poll.
  const onCloseRef = useRef(onClose);
  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);
  const close = useCallback(() => onCloseRef.current(), []);

  const state = status?.state;

  // Read the draft until it stops changing.
  useEffect(() => {
    const id = draftId.current;
    if (!started || id == null || state !== "drafting" || gone) return;
    let live = true;
    const timer = setInterval(() => {
      api.drafts.get(id).then(
        (next) => {
          if (!live) return;
          setStatus(next);
          setPollFailures(0);
        },
        (err: unknown) => {
          if (!live) return;
          if (err instanceof ApiError && err.status === 404) setGone(true);
          else setPollFailures((n) => n + 1);
        },
      );
    }, DRAFT_POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [started, state, gone]);

  const attach = async (files: File[]) => {
    if (files.length === 0) return;
    setWorking("attach");
    setError(undefined);
    try {
      const id = await ensureDraft(retention === "keep");
      const added = await api.drafts.documents.upload(id, files);
      setDocuments((held) => [...held, ...added]);
    } catch (err) {
      setError(message(err));
    } finally {
      setWorking(undefined);
      if (fileInput.current) fileInput.current.value = "";
    }
  };

  const detach = async (doc: DocumentManifest) => {
    const id = draftId.current;
    if (id == null) return;
    setError(undefined);
    try {
      await api.drafts.documents.remove(id, doc.id);
      setDocuments((held) => held.filter((d) => d.id !== doc.id));
    } catch (err) {
      setError(message(err));
    }
  };

  const chooseRetention = async (next: Retention) => {
    setRetention(next);
    const id = draftId.current;
    if (id == null) return;
    try {
      setStatus(await api.drafts.update(id, { retain_documents: next === "keep" }));
    } catch (err) {
      setError(message(err));
    }
  };

  const begin = async () => {
    if (description.trim() === "" && documents.length === 0) {
      setError("Describe yourself or attach a document to draft from.");
      return;
    }
    setWorking("start");
    setError(undefined);
    try {
      const id = await ensureDraft(retention === "keep");
      // The first write may have come before the radio moved; say it again.
      if (status?.retain_documents !== (retention === "keep")) {
        await api.drafts.update(id, { retain_documents: retention === "keep" });
      }
      setStatus(await api.drafts.start(id, { description: description.trim() }));
      setStarted(true);
      setPollFailures(0);
      setGone(false);
    } catch (err) {
      setError(message(err));
    } finally {
      setWorking(undefined);
    }
  };

  const questions: DraftQuestion[] = status?.state === "awaiting_answers" ? status.questions : [];
  const ready = answersOf(questions, answers);

  const sendAnswers = async () => {
    const id = draftId.current;
    if (id == null || ready == null) return;
    setWorking("answer");
    setError(undefined);
    try {
      setStatus(await api.drafts.answer(id, { answers: ready }));
      setAnswers({});
    } catch (err) {
      setError(message(err));
    } finally {
      setWorking(undefined);
    }
  };

  const review = () => {
    if (status == null) return;
    handedOff.current = true;
    setWorking("review");
    onReviewDraft(status.scenario);
  };

  const create = async () => {
    const id = draftId.current;
    if (id == null) return;
    setWorking("create");
    setError(undefined);
    try {
      const created = await api.drafts.createAndRun(id);
      handedOff.current = true;
      onCreated(created.scenario);
    } catch (err) {
      setError(
        err instanceof ApiError && err.status === 409
          ? "The draft is still being written. Try again when it has finished."
          : message(err),
      );
      setWorking(undefined);
    }
  };

  // What the quota shows: the draft in hand has already been counted.
  const spent = status != null ? 1 : 0;
  const left = { remaining: Math.max(0, drafts.remaining - spent), per_month: drafts.per_month };
  const blocked = status == null ? quotaBlock(drafts) : undefined;
  const settled = started && state != null && state !== "drafting";
  const canFinish = settled && !gone && working == null;
  const busy = working != null;
  const panel = status && started ? draftPanel(status) : undefined;

  const footer = (
    <div style={{ display: "flex", alignItems: "center", flexWrap: "wrap", gap: 8, marginTop: 4 }}>
      <Button type="button" onClick={close}>
        Cancel
      </Button>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginLeft: "auto" }}>
        {(!started || state === "failed") && (
          <Button
            type="submit"
            variant="primary"
            disabled={busy || blocked != null || (state === "failed" && gone)}
          >
            {working === "start" ? "Starting…" : state === "failed" ? "Try again" : "Draft it"}
          </Button>
        )}
        {started && (
          <>
            <Button type="button" disabled={!canFinish} onClick={review}>
              Review draft
            </Button>
            <Button type="button" variant="primary" disabled={!canFinish} onClick={() => void create()}>
              {working === "create" ? "Creating…" : "Create & run"}
            </Button>
          </>
        )}
      </div>
    </div>
  );

  return (
    <Dialog
      title="Describe your situation"
      width={980}
      onClose={close}
      onSubmit={() => void (started && state !== "failed" ? sendAnswers() : begin())}
      busy={busy}
      error={error}
      footer={footer}
    >
      {modeSwitch}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(min(320px, 100%), 1fr))",
          gap: 18,
          alignItems: "start",
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
          <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12 }}>
            About you
            <textarea
              className="input"
              rows={5}
              maxLength={4000}
              disabled={started && state !== "failed"}
              value={description}
              placeholder="Your age, income, spending, goals and any big purchases ahead."
              onChange={(e) => setDescription(e.target.value)}
            />
          </label>

          <section aria-label="Documents" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div role="note" style={{ ...BOX, fontSize: 12, color: MUTED, display: "flex", flexDirection: "column", gap: 4 }}>
              <b style={{ color: "var(--color-text)" }}>Before you attach anything</b>
              {CONSENT_LINES.map((line) => (
                <span key={line}>{line}</span>
              ))}
            </div>
            <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
              <input
                ref={fileInput}
                type="file"
                multiple
                hidden
                aria-label="Attach files"
                onChange={(e) => void attach(Array.from(e.target.files ?? []))}
              />
              <Button
                type="button"
                disabled={
                  working != null ||
                  blocked != null ||
                  (started && state === "drafting") ||
                  documents.length >= drafts.max_files
                }
                onClick={() => fileInput.current?.click()}
              >
                {working === "attach" ? "Uploading…" : "Attach files"}
              </Button>
              <span style={{ fontSize: 12, color: MUTED }}>{limitsLine(drafts)}</span>
            </div>
            {documents.length > 0 && (
              <ul aria-label="Attached files" style={{ margin: 0, padding: 0, listStyle: "none", display: "flex", flexDirection: "column", gap: 4 }}>
                {documents.map((doc) => (
                  <li
                    key={doc.id}
                    style={{ display: "flex", alignItems: "baseline", gap: 8, fontSize: 12.5, minWidth: 0 }}
                  >
                    <span style={{ minWidth: 0, overflowWrap: "anywhere", flex: "1 1 auto" }}>
                      {doc.filename}
                      <span style={{ color: MUTED }}>
                        {" "}
                        · {fileSize(doc.bytes)} · {documentNote(doc)}
                      </span>
                    </span>
                    <button
                      type="button"
                      className="btn btn-ghost"
                      style={{ fontSize: 12 }}
                      disabled={working != null || (started && state === "drafting")}
                      aria-label={`Remove ${doc.filename}`}
                      onClick={() => void detach(doc)}
                    >
                      Remove
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <fieldset style={{ border: 0, margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 6 }}>
            <legend style={{ padding: 0, marginBottom: 6, fontSize: 12 }}>Your documents</legend>
            {RETENTION_OPTIONS.map((option) => (
              <label key={option.value} className="radio">
                <input
                  type="radio"
                  name="draft-retention"
                  value={option.value}
                  checked={retention === option.value}
                  disabled={state === "drafting"}
                  onChange={() => void chooseRetention(option.value)}
                />
                <span className="dot" />
                {option.label}
              </label>
            ))}
          </fieldset>

          {questions.length > 0 && (
            <section aria-label="Questions" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              {questions.map((question, index) => (
                <QuestionField
                  key={question.key}
                  number={index + 1}
                  question={question}
                  value={answers[question.key]}
                  onChange={(value) => setAnswers((held) => ({ ...held, [question.key]: value }))}
                />
              ))}
              <div>
                <Button type="submit" variant="primary" disabled={busy || ready == null}>
                  {working === "answer" ? "Sending…" : "Send answers"}
                </Button>
              </div>
            </section>
          )}

          <p style={{ margin: 0, fontSize: 12, color: MUTED }}>
            {quotaLine(access, left)}
            {blocked && <> · {blocked}</>}
          </p>
        </div>

        <DraftPanel
          panel={panel}
          gone={gone}
          lostTouch={pollFailures >= POLL_FAILURES_SHOWN}
          stop={status?.stop ?? undefined}
        />
      </div>
    </Dialog>
  );
}

/** One of the agent's questions: segmented for a choice, otherwise a value. */
function QuestionField({
  number,
  question,
  value,
  onChange,
}: {
  number: number;
  question: DraftQuestion;
  value: string | undefined;
  onChange: (value: string) => void;
}) {
  const id = `draft-question-${question.key}`;
  return (
    <div role="group" aria-labelledby={id} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <div id={id} style={{ fontSize: 13, fontWeight: 600 }}>
        {number}. {question.prompt}
      </div>
      {question.answer_type === "choice" ? (
        <SegmentedControl
          name={id}
          ariaLabel={question.prompt}
          options={question.options}
          value={value ?? null}
          onChange={onChange}
        />
      ) : question.answer_type === "money" ? (
        <CurrencyInput
          nullable
          group
          decimals={0}
          value={value == null || value === "" ? null : Number(value)}
          aria-label={question.prompt}
          onValueChange={(amount) => onChange(amount == null ? "" : String(amount))}
        />
      ) : question.answer_type === "date" ? (
        <DateInput value={value ?? ""} ariaLabel={question.prompt} onValueChange={onChange} />
      ) : (
        <Input aria-label={question.prompt} value={value ?? ""} onChange={(e) => onChange(e.target.value)} />
      )}
    </div>
  );
}

/** The draft as it fills in (design 2a, right-hand side). */
function DraftPanel({
  panel,
  gone,
  lostTouch,
  stop,
}: {
  panel: ReturnType<typeof draftPanel> | undefined;
  gone: boolean;
  lostTouch: boolean;
  stop?: string;
}) {
  return (
    <aside
      aria-label="Draft"
      style={{ ...BOX, display: "flex", flexDirection: "column", gap: 8, minWidth: 0, minHeight: 160 }}
    >
      {panel == null ? (
        <p style={{ margin: 0, fontSize: 13, color: MUTED }}>
          Your draft appears here once you start: the accounts, events and assumptions it finds, and anything it needs
          to ask you.
        </p>
      ) : (
        <div role="status" aria-live="polite" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
            <h6 style={{ margin: 0 }}>{panel.heading}</h6>
            {panel.estimate && (
              <span style={{ marginLeft: "auto" }}>
                <Tag tone="outline">{panel.estimate}</Tag>
              </span>
            )}
          </div>
          <div style={{ fontSize: 13 }}>{panel.summary}</div>
          {panel.progress && (
            <div style={{ fontSize: 12, color: MUTED, fontStyle: "italic" }}>{panel.progress}</div>
          )}
          {panel.waiting.length > 0 && (
            <ul style={{ margin: 0, padding: 0, listStyle: "none", display: "flex", flexDirection: "column", gap: 4 }}>
              {panel.waiting.map((note) => (
                <li key={note.id} style={{ fontSize: 12.5 }}>
                  {note.title} <span style={{ color: MUTED }}>· {note.text}</span>
                </li>
              ))}
            </ul>
          )}
          {panel.notes && <div style={{ fontSize: 12, color: MUTED }}>{panel.notes}</div>}
          {panel.error && (
            <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--color-accent-800)" }}>
              {panel.error}
            </p>
          )}
          {stop === "turn_limit" || stop === "max_tokens" ? (
            <p style={{ margin: 0, fontSize: 12, color: MUTED }}>
              It stopped before covering everything. Review what it wrote and add the rest.
            </p>
          ) : null}
        </div>
      )}
      {gone && (
        <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--color-accent-800)" }}>
          This draft has expired or was deleted. Close this and start again.
        </p>
      )}
      {lostTouch && !gone && (
        <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--color-accent-800)" }}>
          The server is not answering. Still trying.
        </p>
      )}
    </aside>
  );
}
