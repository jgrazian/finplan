/**
 * The Review tab for a plan on this device (spec 19, phase 4): the rule-based
 * notes, with no model and no server.
 *
 * `finplan_plan::review::local_review` reads a plan and one run's results and
 * writes the notes the rules find, with each course of action's diff. Here that
 * is kept as one `Review` per plan (`{...localReview, ai: null}`: no model pass),
 * in `meta`, so a reload finds the notes where they were left.
 *
 *  - **Dismissals** are kept by fingerprint (`suggestion_fingerprint`, which is
 *    what a note is about independent of the figures it quotes) and passed back
 *    as `silenced` on the next review, so a note set aside stays set aside when
 *    the run is read again.
 *  - **Applying** a path writes its steps to the plan (or to a copy) through
 *    `apply_note`, which resolves each step against the plan the one before it
 *    left and refuses, with the server's problems body, a step whose `expect`
 *    no longer matches (409 stale) or that no longer fits (422).
 *  - **Not here**: the model-written pass, plan chat and per-note chat (they
 *    need the plan on the server), and "Preview", which simulates a path
 *    against the run and would need a second local run of the edited plan.
 *    Those stay behind "Move to cloud to use AI".
 */
import type { AppliedSuggestion, ApplySuggestion, Review, Suggestion } from "../api/suggestions.ts";
import type { LocalReviewResult } from "../api/generated/LocalReviewResult.ts";
import type { Core } from "./core.ts";
import { badRequest, conflict, guard, notFound } from "./errors.ts";
import { planLockName, withLock } from "./locks.ts";
import type { RunRecord, StoreTx } from "./store.ts";

/** What the Review tab asks of a plan's home when that home has no server. */
export interface LocalReviewApi {
  get(scenarioId: number): Promise<Review | null>;
  /** Review `runId` (the latest succeeded run when null). */
  run(scenarioId: number, runId: number | null): Promise<Review>;
  apply(scenarioId: number, id: number, body: ApplySuggestion): Promise<AppliedSuggestion>;
  /** Set a note aside, or say it is correct. */
  dismiss(scenarioId: number, id: number, as: "dismissed" | "confirmed"): Promise<Suggestion>;
  reopen(scenarioId: number, id: number): Promise<Suggestion>;
}

interface StoredReview {
  run_id: number;
  reviewed_at: string;
  suggestions: Suggestion[];
  /** One per suggestion, same order: what a dismissal is kept under. */
  fingerprints: string[];
}

const reviewKey = (planId: number) => `review:${planId}`;
const dismissedKey = (planId: number) => `dismissed:${planId}`;

const asReview = (stored: StoredReview): Review => ({
  run_id: stored.run_id,
  reviewed_at: stored.reviewed_at,
  suggestions: stored.suggestions,
  ai: null,
});

export function reviewGroup(core: Core): LocalReviewApi {
  const loadReview = async (tx: StoreTx, planId: number): Promise<StoredReview | undefined> =>
    tx.getMeta<StoredReview>(reviewKey(planId));

  const noteOf = (stored: StoredReview, id: number): { note: Suggestion; index: number } => {
    const index = stored.suggestions.findIndex((s) => s.id === id);
    if (index < 0) throw notFound("suggestion");
    return { note: stored.suggestions[index], index };
  };

  const toggle = async (
    scenarioId: number,
    id: number,
    status: "dismissed" | "confirmed" | "open",
  ): Promise<Suggestion> =>
    core.store.transact("rw", async (tx) => {
      await core.requirePlan(tx, scenarioId);
      const stored = await loadReview(tx, scenarioId);
      if (!stored) throw notFound("review");
      const { note, index } = noteOf(stored, id);
      const fingerprint = stored.fingerprints[index];
      const silenced = new Set((await tx.getMeta<string[]>(dismissedKey(scenarioId))) ?? []);
      if (status === "open") {
        if (note.status !== "dismissed" && note.status !== "confirmed") {
          throw conflict("this note is already open");
        }
        silenced.delete(fingerprint);
      } else {
        if (note.status !== "open") throw conflict("this note has already been acted on");
        silenced.add(fingerprint);
      }
      const next: Suggestion = {
        ...note,
        status,
        resolved_at: status === "open" ? null : core.nowText(),
      };
      stored.suggestions[index] = next;
      await tx.putMeta(reviewKey(scenarioId), stored);
      await tx.putMeta(dismissedKey(scenarioId), [...silenced]);
      return next;
    });

  return {
    async get(scenarioId) {
      return core.store.transact("r", async (tx) => {
        await core.requirePlan(tx, scenarioId);
        const stored = await loadReview(tx, scenarioId);
        return stored ? asReview(stored) : null;
      });
    },

    async run(scenarioId, runId) {
      const input = await core.store.transact("r", async (tx) => {
        const plan = await core.requirePlan(tx, scenarioId);
        const runs = await tx.listRuns(scenarioId);
        const run: RunRecord | undefined =
          runId === null ? runs.find((r) => r.status === "succeeded") : runs.find((r) => r.id === runId);
        if (!run) {
          throw runId === null ? conflict("Run the plan first; a review is written about a finished run.") : notFound("run");
        }
        if (run.status !== "succeeded") throw conflict("this run has no results to review");
        const results = await tx.getResults(run.id);
        if (results === undefined) throw conflict("this run has no results to review");
        return {
          graph: run.snapshot ?? plan.graph,
          library: await core.library(tx),
          results,
          runId: run.id,
          silenced: (await tx.getMeta<string[]>(dismissedKey(scenarioId))) ?? [],
        };
      });
      const reviewedAt = core.nowText();
      const out = core.call<LocalReviewResult>((engine) =>
        engine.local_review(
          input.graph,
          input.library,
          input.results,
          input.runId,
          reviewedAt,
          JSON.stringify(input.silenced),
        ),
      );
      const stored: StoredReview = {
        run_id: out.review.run_id,
        reviewed_at: out.review.reviewed_at,
        suggestions: out.review.suggestions,
        fingerprints: out.fingerprints,
      };
      await core.store.transact("rw", async (tx) => {
        await core.requirePlan(tx, scenarioId);
        await tx.putMeta(reviewKey(scenarioId), stored);
      });
      return asReview(stored);
    },

    dismiss: (scenarioId, id, as) => toggle(scenarioId, id, as),
    reopen: (scenarioId, id) => toggle(scenarioId, id, "open"),

    async apply(scenarioId, id, body) {
      const toCopy = body.to === "copy";
      if (toCopy && body.through_step !== null) {
        throw badRequest("a copy takes the whole path, so through_step must be null");
      }
      const result = await withLock(planLockName(scenarioId), () =>
        core.store.transact("rw", async (tx) => {
          const plan = await core.requirePlan(tx, scenarioId);
          const stored = await loadReview(tx, scenarioId);
          if (!stored) throw notFound("review");
          const { note, index } = noteOf(stored, id);
          if (note.status !== "open") throw conflict("this note has already been acted on");
          if (note.applied_path !== null && note.applied_path !== body.path) {
            throw conflict("another path of this note is already being applied");
          }
          const path = note.paths.find((p) => p.key === body.path);
          if (!path) throw notFound("path");
          const steps = toCopy ? path.steps : path.steps.filter((s) => !s.applied);
          let chosen = steps;
          if (body.through_step !== null) {
            const through = steps.findIndex((s) => s.key === body.through_step);
            if (through < 0) throw notFound("step");
            chosen = steps.slice(0, through + 1);
          }
          if (chosen.length === 0) throw conflict("every step of this path is already applied");

          const library = await core.library(tx);
          const now = core.nowText();
          const copyId = toCopy ? await tx.nextId("plan") : Number.NaN;
          const copyName = toCopy ? await copyNameFor(tx, plan.name, note.title, body.name) : "";
          const graph = guard(() =>
            core.engine.apply_note(
              plan.graph,
              library,
              path.key,
              JSON.stringify(chosen.map((s) => ({ key: s.key, changes: s.changes }))),
              copyId,
              copyName,
              now,
            ),
          );
          if (toCopy) {
            await tx.putPlan({
              id: copyId,
              name: copyName,
              graph,
              created_at: now,
              updated_at: now,
              last_exported_at: null,
              edits_since_export: 0,
            });
            return { scenarioId: copyId, suggestion: note, newPlan: true };
          }
          await tx.putPlan({ ...plan, graph, updated_at: now, edits_since_export: plan.edits_since_export + 1 });
          const appliedKeys = new Set(chosen.map((s) => s.key));
          const paths = note.paths.map((p) =>
            p.key === path.key
              ? {
                  ...p,
                  steps: p.steps.map((s) => (appliedKeys.has(s.key) ? { ...s, applied: true, applied_at: now } : s)),
                }
              : p,
          );
          const done = paths.find((p) => p.key === path.key)?.steps.every((s) => s.applied) ?? false;
          const next: Suggestion = {
            ...note,
            paths,
            applied_path: path.key,
            status: done ? "applied" : "open",
            resolved_at: done ? now : null,
          };
          stored.suggestions[index] = next;
          await tx.putMeta(reviewKey(scenarioId), stored);
          return { scenarioId, suggestion: next, newPlan: false };
        }),
      );
      core.announce(result.newPlan ? null : scenarioId);
      return { scenario_id: result.scenarioId, suggestion: result.suggestion };
    },
  };

  async function copyNameFor(tx: StoreTx, plan: string, title: string, asked: string | null): Promise<string> {
    const taken = new Set((await tx.listPlans()).map((p) => p.name));
    const base = (asked?.trim() || `${plan} — ${title}`).slice(0, 120);
    if (asked?.trim()) {
      if (taken.has(base)) throw conflict("a scenario with that name already exists");
      return base;
    }
    let name = base;
    for (let n = 2; taken.has(name); n++) name = `${base} (${n})`;
    return name;
  }
}
