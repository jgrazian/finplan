"use client";

import { useCallback, useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { ChatBubble, ChatText } from "@/components/chat/ChatBubble";
import { Blueprint, Button, CurrencyInput, DateInput, Input, SegmentedControl, Tag } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { AiDrafts } from "@/lib/api/generated/AiDrafts";
import type { DocumentManifest } from "@/lib/api/generated/DocumentManifest";
import type { DraftQuestion } from "@/lib/api/generated/DraftQuestion";
import type { DraftStatus } from "@/lib/api/generated/DraftStatus";
import type { Entitlements } from "@/lib/api/generated/Entitlements";
import { ApiError } from "@/lib/api/http";
import type { Scenario } from "@/lib/api/types";
import {
  answerLabel,
  answersOf,
  CONSENT_LINES,
  documentNote,
  DRAFT_POLL_MS,
  draftContents,
  type DraftContents,
  draftPanel,
  fileSize,
  limitsLine,
  outgoingMessage,
  quotaBlock,
  quotaLine,
  RETENTION_OPTIONS,
  type Retention,
} from "@/lib/view/draft";
import { money } from "@/lib/view/format";
import { SetupBar } from "./SetupBar";

const ERROR = { margin: 0, fontSize: 12.5, color: "var(--color-accent-800)" } as const;

/** How many polls in a row may fail before the panel says it has lost touch. */
const POLL_FAILURES_SHOWN = 3;

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/** "PDF", "CSV": the file's kind, as its chip in a message shows it. */
const extension = (filename: string) => {
  const dot = filename.lastIndexOf(".");
  return dot > 0 ? filename.slice(dot + 1).toUpperCase().slice(0, 4) : "FILE";
};

const documentsWord = (count: number) => `${count} ${count === 1 ? "document" : "documents"}`;

/** "5 documents and your description", leaving out what was not given. */
function sourcesOf(described: boolean, documents: number): string | undefined {
  const parts = [
    documents > 0 ? documentsWord(documents) : undefined,
    described ? "your description" : undefined,
  ].filter((part): part is string => part != null);
  return parts.length > 0 ? parts.join(" and ") : undefined;
}

/** One message in the conversation, as it was sent or as the agent ended. */
type Turn =
  | {
      key: number;
      from: "you";
      text: string;
      files: DocumentManifest[];
      answers: Array<{ prompt: string; answer: string }>;
    }
  | { key: number; from: "finplan"; text: string };

/**
 * New scenario, Describe & upload (design 2a): a conversation on the left,
 * where the person describes themselves, attaches files, answers the agent's
 * questions and tells it more once a draft is written; and the draft filling
 * in on the right, account by account and event by event. Nothing here is a
 * plan until Create & run; leaving the page (or going back to Guided) deletes
 * the draft, which is not resumable.
 */
export function DescribeSetup({
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
  const [reply, setReply] = useState("");
  const [retention, setRetention] = useState<Retention>("delete");
  const [status, setStatus] = useState<DraftStatus>();
  const [documents, setDocuments] = useState<DocumentManifest[]>([]);
  // Files before this index went with an earlier message.
  const [sentFiles, setSentFiles] = useState(0);
  const [thread, setThread] = useState<Turn[]>([]);
  const [contents, setContents] = useState<DraftContents>();
  const [started, setStarted] = useState(false);
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [working, setWorking] = useState<"attach" | "start" | "send" | "create" | "review">();
  const [error, setError] = useState<string>();
  const [unadded, setUnadded] = useState(false);
  const [pollFailures, setPollFailures] = useState(0);
  const [gone, setGone] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const composer = useRef<HTMLTextAreaElement>(null);
  const turnKey = useRef(0);

  // The draft exists from the first file or the first "Draft it", not from
  // opening the page: creating one spends one of the month's drafts.
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

  // Cancel, leaving for a tab, and switching to Guided or Blank all unmount
  // this: one place deletes.
  useEffect(() => discard, [discard]);

  // The page opens on the description, ready to type.
  useEffect(() => {
    composer.current?.focus();
  }, []);

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

  // The rows behind the rail's lists, read again whenever what the draft
  // holds moves (a note added counts: it may change a balance, not a count).
  const counts = status?.counts;
  const holds = started && counts
    ? `${counts.accounts}/${counts.assets}/${counts.events}/${counts.notes_added}`
    : undefined;
  useEffect(() => {
    const id = draftId.current;
    if (holds == null || id == null) return;
    let live = true;
    Promise.all([api.accounts.list(id), api.assets.list(id), api.events.list(id)]).then(
      ([accounts, assets, events]) => live && setContents(draftContents(accounts, assets, events)),
      // The counts still say what is there; the lists catch up on the next change.
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [holds]);

  // Each time the agent stops, its closing line answers the last message.
  const closing = state === "ready" ? (status?.progress ?? "The draft is ready.") : undefined;
  useEffect(() => {
    if (!started || closing == null) return;
    setThread((turns) =>
      turns.at(-1)?.from !== "you"
        ? turns
        : [...turns, { key: ++turnKey.current, from: "finplan", text: closing }],
    );
  }, [started, closing]);

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
      // A retry after a failure starts over, so the thread does too.
      setThread([
        { key: ++turnKey.current, from: "you", text: description.trim(), files: documents, answers: [] },
      ]);
      setSentFiles(documents.length);
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
  const fresh = documents.slice(sentFiles);
  const followUpsLeft = status?.follow_ups_left ?? 0;

  /** The composer's Send: answers and a message while it waits, a follow-up once it is ready. */
  const send = async () => {
    const id = draftId.current;
    if (id == null) return;
    const outgoing = outgoingMessage(reply, fresh);
    if (state === "awaiting_answers" && ready == null) {
      setError("Answer the questions above to send.");
      return;
    }
    if (state === "ready" && outgoing === "") {
      setError("Write something to send.");
      return;
    }
    setWorking("send");
    setError(undefined);
    try {
      const answered = questions.map((q) => ({ prompt: q.prompt, answer: answerLabel(q, answers[q.key]) }));
      const next =
        state === "awaiting_answers" && ready != null
          ? await api.drafts.answer(id, { answers: ready, ...(outgoing ? { message: outgoing } : {}) })
          : await api.drafts.message(id, { message: outgoing });
      setThread((turns) => [
        ...turns,
        {
          key: ++turnKey.current,
          from: "you",
          text: reply.trim(),
          files: fresh,
          answers: state === "awaiting_answers" ? answered : [],
        },
      ]);
      setStatus(next);
      setReply("");
      setAnswers({});
      setSentFiles(documents.length);
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

  // Create & run adds the notes not yet added first; if one cannot be added
  // the draft stays a draft, the note is named, and it can be left out.
  const create = async (addOpen: boolean) => {
    const id = draftId.current;
    if (id == null) return;
    setWorking("create");
    setError(undefined);
    setUnadded(false);
    try {
      const created = await api.drafts.createAndRun(id, { add_open: addOpen });
      handedOff.current = true;
      onCreated(created.scenario);
    } catch (err) {
      setError(
        err instanceof ApiError && err.status === 409
          ? "The draft is still being written. Try again when it has finished."
          : message(err),
      );
      if (err instanceof ApiError && err.status === 422) {
        setUnadded(true);
        // What it could add is in the draft now: read the status again.
        api.drafts.get(id).then(setStatus, () => undefined);
      }
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
  // The first message has gone and the agent has not failed on it: the
  // composer now answers and follows up rather than describes.
  const sent = started && state !== "failed";
  const drafting = started && state === "drafting";
  const sources = sourcesOf(description.trim() !== "", documents.length);
  const canSend =
    !busy &&
    !gone &&
    (state === "awaiting_answers" ? ready != null : state === "ready" && followUpsLeft > 0);

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    void (sent ? send() : begin());
  };

  const placeholder = !sent
    ? "Your age, income, spending, goals and any big purchases ahead…"
    : drafting
      ? "Drafting… you can write again when it stops."
      : state === "awaiting_answers"
        ? "Anything else? It goes with your answers."
        : followUpsLeft > 0
          ? "Tell me more, or correct something…"
          : "This draft takes no more messages. Change the rest on Review.";

  return (
    <div className="ns-page">
      <SetupBar
        title="New scenario"
        modeSwitch={modeSwitch}
        end={
          <span className="ns-mut">
            {quotaLine(access, left)}
            {blocked && <> · {blocked}</>}
          </span>
        }
      />

      <div className="ns-body ns-body-describe">
        <form className="ns-col" aria-label="Describe your situation" onSubmit={onSubmit}>
          <div className="ns-scroll">
            <div className="ns-thread">
              {!sent && (
                <ChatBubble from="finplan">
                  <span style={{ textWrap: "pretty" }}>
                    Tell me about yourself: your age, income, spending, goals and any big purchases ahead.
                    Attach bank, brokerage and 401(k) statements, pay stubs or a tax return and I&rsquo;ll
                    read your balances from them. I&rsquo;ll ask up to three questions they don&rsquo;t
                    answer, and build the draft on the right.
                  </span>
                  <div
                    role="note"
                    className="ns-mut"
                    style={{
                      display: "flex",
                      flexDirection: "column",
                      gap: 4,
                      fontSize: 12,
                      paddingTop: 8,
                      borderTop: "1px solid var(--color-divider)",
                    }}
                  >
                    <b style={{ color: "var(--color-text)" }}>Before you attach anything</b>
                    {CONSENT_LINES.map((line) => (
                      <span key={line}>{line}</span>
                    ))}
                  </div>
                </ChatBubble>
              )}

              {sent &&
                thread.map((turn) =>
                  turn.from === "you" ? (
                    <ChatBubble key={turn.key} from="you">
                      {turn.answers.length > 0 && (
                        <dl style={{ margin: 0, display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
                          {turn.answers.map((a) => (
                            <div key={a.prompt}>
                              <dt className="ns-mut" style={{ display: "inline" }}>
                                {a.prompt}{" "}
                              </dt>
                              <dd style={{ display: "inline", margin: 0, fontWeight: 600 }}>{a.answer}</dd>
                            </div>
                          ))}
                        </dl>
                      )}
                      {turn.text && <ChatText>{turn.text}</ChatText>}
                      <FileChips files={turn.files} />
                    </ChatBubble>
                  ) : (
                    <ChatBubble key={turn.key} from="finplan">
                      <ChatText>{turn.text}</ChatText>
                    </ChatBubble>
                  ),
                )}

              {questions.length > 0 && (
                <ChatBubble as="section" ariaLabel="Questions" from="finplan">
                  <span style={{ textWrap: "pretty" }}>
                    I read {sources ?? "what you sent"}.{" "}
                    {questions.length === 1 ? "One thing" : `${questions.length} things`}{" "}
                    I couldn&rsquo;t tell from {documents.length > 0 ? "them" : "it"}:
                  </span>
                  <div style={{ display: "flex", flexDirection: "column", gap: 12, padding: "4px 0" }}>
                    {questions.map((question, index) => (
                      <QuestionField
                        key={question.key}
                        number={index + 1}
                        question={question}
                        value={answers[question.key]}
                        onChange={(value) => setAnswers((held) => ({ ...held, [question.key]: value }))}
                      />
                    ))}
                  </div>
                  <span className="ns-mut" style={{ fontSize: 12.5, textWrap: "pretty" }}>
                    Anything else I&rsquo;m unsure of goes into the draft as a note to confirm.
                  </span>
                </ChatBubble>
              )}

              {drafting && panel && (
                <span className="ns-mut" style={{ fontSize: 12.5, fontStyle: "italic" }}>
                  {panel.progress ?? "Drafting…"} · {panel.summary}
                </span>
              )}
            </div>
          </div>

          <div className="ns-composer">
            <FileChips
              files={sent ? fresh : documents}
              disabled={busy || drafting}
              onRemove={(doc) => void detach(doc)}
            />
            <textarea
              ref={composer}
              className="input"
              rows={sent ? 2 : 3}
              maxLength={4000}
              aria-label={sent ? "Message" : "About you"}
              value={sent ? reply : description}
              disabled={
                working === "start" || working === "send" || drafting || (state === "ready" && followUpsLeft === 0)
              }
              placeholder={placeholder}
              style={{ resize: "vertical", fontSize: 13 }}
              onChange={(e) => (sent ? setReply(e.target.value) : setDescription(e.target.value))}
            />
            {error && (
              <p role="alert" style={ERROR}>
                {error}
              </p>
            )}
            <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 10 }}>
              <input
                ref={fileInput}
                type="file"
                multiple
                hidden
                aria-label="Attach files"
                onChange={(e) => void attach(Array.from(e.target.files ?? []))}
              />
              <Button
                disabled={
                  busy ||
                  drafting ||
                  blocked != null ||
                  documents.length >= drafts.max_files ||
                  (state === "ready" && followUpsLeft === 0)
                }
                onClick={() => fileInput.current?.click()}
              >
                {working === "attach" ? "Uploading…" : "Attach files"}
              </Button>
              <span className="ns-mut" style={{ fontSize: 11.5 }}>
                {sent && state === "ready"
                  ? `${followUpsLeft} ${followUpsLeft === 1 ? "message" : "messages"} left on this draft`
                  : `PDF, CSV, OFX, images · ${limitsLine(drafts)}`}
              </span>
              {sent ? (
                <Button type="submit" variant="primary" style={{ marginLeft: "auto" }} disabled={!canSend}>
                  {working === "send" ? "Sending…" : state === "awaiting_answers" ? "Send answers" : "Send"}
                </Button>
              ) : (
                <Button
                  type="submit"
                  variant="primary"
                  style={{ marginLeft: "auto" }}
                  disabled={busy || blocked != null || (state === "failed" && gone)}
                >
                  {working === "start" ? "Starting…" : state === "failed" ? "Try again" : "Draft it"}
                </Button>
              )}
            </div>
          </div>
        </form>

        <div className="ns-col">
          <div className="ns-rail-head">
            <h3 style={{ margin: 0, fontSize: 22 }}>Draft</h3>
            {sources && (
              <span className="ns-mut" style={{ fontSize: 12, textAlign: "right" }}>
                from {sources}
              </span>
            )}
          </div>
          <div className="ns-scroll">
            <div className="ns-rail">
              <DraftPanel
                panel={panel}
                contents={contents}
                gone={gone}
                lostTouch={pollFailures >= POLL_FAILURES_SHOWN}
                stop={status?.stop ?? undefined}
              />

              <fieldset
                style={{
                  border: 0,
                  margin: 0,
                  padding: 0,
                  display: "flex",
                  flexDirection: "column",
                  gap: 6,
                }}
              >
                {/* The rule sits above the legend, not through it as a fieldset's border would. */}
                <legend
                  className="ns-lbl"
                  style={{
                    width: "100%",
                    padding: "14px 0 0",
                    marginBottom: 6,
                    borderTop: "1px solid var(--color-divider)",
                  }}
                >
                  Your documents
                </legend>
                {RETENTION_OPTIONS.map((option) => (
                  <label key={option.value} className="radio" style={{ fontSize: 13 }}>
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
            </div>
          </div>
          <div className="ns-foot ns-foot-rail">
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <div className="ns-foot-end">
              <Button disabled={!canFinish} onClick={review}>
                Review draft
              </Button>
              {unadded && (
                <Button disabled={!canFinish} onClick={() => void create(false)}>
                  Create without them
                </Button>
              )}
              <Button variant="primary" disabled={!canFinish} onClick={() => void create(true)}>
                {working === "create" ? "Creating…" : "Create & run"}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

/** Files as chips: in a sent message, or waiting in the composer to be removed. */
function FileChips({
  files,
  disabled,
  onRemove,
}: {
  files: readonly DocumentManifest[];
  disabled?: boolean;
  /** Present in the composer, where a file can still be taken off. */
  onRemove?: (doc: DocumentManifest) => void;
}) {
  if (files.length === 0) return null;
  return (
    <ul
      aria-label={onRemove ? "Attached files" : "Files sent"}
      style={{ margin: 0, padding: 0, listStyle: "none", display: "flex", flexWrap: "wrap", gap: 6 }}
    >
      {files.map((doc) => (
        <li key={doc.id} className="ns-fchip" title={`${fileSize(doc.bytes)} · ${documentNote(doc)}`}>
          <b>{extension(doc.filename)}</b>
          <span style={{ overflowWrap: "anywhere" }}>{doc.filename}</span>
          {/* Once sent, only a file that was not read cleanly says so. */}
          {(onRemove || doc.status !== "parsed") && <span className="ns-mut">· {documentNote(doc)}</span>}
          {onRemove && (
            <button
              type="button"
              className="btn btn-ghost"
              style={{ fontSize: 12, minHeight: 0, padding: "0 4px" }}
              disabled={disabled}
              aria-label={`Remove ${doc.filename}`}
              onClick={() => onRemove(doc)}
            >
              ×
            </button>
          )}
        </li>
      ))}
    </ul>
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
      <span id={id} style={{ fontSize: 13 }}>
        {number}. {question.prompt}
      </span>
      <div style={{ alignSelf: "flex-start", minWidth: question.answer_type === "choice" ? undefined : 180 }}>
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
    </div>
  );
}

/**
 * The draft as it fills in (design 2a, right-hand rail): its status, then the
 * accounts and events it holds, with the notes that wait on an answer shown
 * as ghost rows among them.
 */
function DraftPanel({
  panel,
  contents,
  gone,
  lostTouch,
  stop,
}: {
  panel: ReturnType<typeof draftPanel> | undefined;
  contents: DraftContents | undefined;
  gone: boolean;
  lostTouch: boolean;
  stop?: string;
}) {
  return (
    <section aria-label="Draft status" style={{ display: "flex", flexDirection: "column", gap: 18 }}>
      {panel == null ? (
        <p className="ns-mut" style={{ margin: 0, fontSize: 13 }}>
          Your draft appears here once you start: the accounts, events and assumptions it finds, and anything it needs
          to ask you.
        </p>
      ) : (
        <div role="status" aria-live="polite" style={{ display: "flex", flexDirection: "column", gap: 18 }}>
          <Blueprint style={{ padding: "12px 14px", display: "flex", flexDirection: "column", gap: 6 }}>
            <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
              <span className="ns-lbl">{panel.heading}</span>
              {panel.estimate && (
                <span style={{ marginLeft: "auto" }}>
                  <Tag tone="outline">{panel.estimate}</Tag>
                </span>
              )}
            </div>
            <div style={{ fontSize: 14 }}>{panel.summary}</div>
          </Blueprint>

          {contents && contents.accounts.length > 0 && (
            <div>
              <div className="ns-lbl" style={{ marginBottom: 4 }}>
                Accounts · {money(contents.total)}
              </div>
              {contents.accounts.map((account) => (
                <div key={account.id} className="ns-crow">
                  <span style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>{account.name}</span>
                  <Tag tone={account.tone}>{account.tag}</Tag>
                  <span style={{ minWidth: 90, textAlign: "right" }}>{money(account.balance)}</span>
                </div>
              ))}
            </div>
          )}

          {((contents && contents.events.length > 0) || panel.waiting.length > 0) && (
            <div>
              <div className="ns-lbl" style={{ marginBottom: 4 }}>
                Events
              </div>
              {contents?.events.map((event) => (
                <div key={event.id} className="ns-crow ns-mono" style={{ fontSize: 12 }}>
                  <span style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>{event.name}</span>
                  <span className="ns-mut" style={{ textAlign: "right" }}>
                    {event.when}
                  </span>
                </div>
              ))}
              {panel.waiting.map((note) => (
                <div key={note.id} className="ns-crow ns-mut" style={{ fontSize: 12, fontStyle: "italic" }}>
                  <span style={{ flex: 1, minWidth: 0 }}>{note.title}</span>
                  <span style={{ textAlign: "right" }}>{note.text}</span>
                </div>
              ))}
            </div>
          )}

          {panel.notes && (
            <div style={{ display: "flex", alignItems: "baseline", gap: 8, fontSize: 12.5 }}>
              <Tag tone="outline">Notes</Tag>
              <span className="ns-mut">{panel.notes}</span>
            </div>
          )}
          {panel.error && (
            <p role="alert" style={ERROR}>
              {panel.error}
            </p>
          )}
          {stop === "turn_limit" || stop === "max_tokens" ? (
            <p className="ns-mut" style={{ margin: 0, fontSize: 12 }}>
              It stopped before covering everything. Send a message such as &ldquo;Carry on&rdquo; to let it
              finish, or review what it wrote and add the rest.
            </p>
          ) : null}
        </div>
      )}
      {gone && (
        <p role="alert" style={ERROR}>
          This draft has expired or was deleted. Cancel and start again.
        </p>
      )}
      {lostTouch && !gone && (
        <p role="alert" style={ERROR}>
          The server is not answering. Still trying.
        </p>
      )}
    </section>
  );
}
