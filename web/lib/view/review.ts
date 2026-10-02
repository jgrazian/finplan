import type { ChangeProblem } from "../api/generated/ChangeProblem.ts";
import type { ChangeTarget } from "../api/generated/ChangeTarget.ts";
import type { Preview } from "../api/generated/Preview.ts";
import type { PreviewStats } from "../api/generated/PreviewStats.ts";
import type {
  Evidence,
  Review,
  Suggestion,
  SuggestionKind,
  SuggestionPath,
  SuggestionSection,
  SuggestionStatus,
  SuggestionStep,
} from "../api/suggestions.ts";
import { fmtCompact, fmtInt, fmtPercent } from "../format.ts";
import { reviewActivity } from "./activity.ts";
import { type ChatOffer, chatOffer } from "./chat.ts";
import { clockTime } from "./issues.ts";

/**
 * The Review tab's board (design 14a).
 *
 * A review is a set of notes about one run, each written against one part of
 * the plan and most offering up to four paths — courses of action, each up to
 * four ordered steps the user can apply whole or one at a time. The board
 * groups the notes by section — Portfolio, Scenario & events, Results — and
 * the Review tab lists them beside the one in hand (`noteList`); each is a card: a kicker naming the kind of note, the claim, the
 * reasoning, and for the path in hand its steps with the server's own diff of
 * each, and what the whole path did when simulated against the run (or the
 * author's estimate, until it is). A note with several paths shows them as a
 * picker; the actions act on the one selected, and once a step of one is
 * applied the others close.
 */

export const SECTIONS: ReadonlyArray<{ id: SuggestionSection; heading: string }> = [
  { id: "portfolio", heading: "Portfolio" },
  { id: "plan", heading: "Scenario & events" },
  { id: "results", heading: "Results" },
];

export const KIND_LABEL: Record<SuggestionKind, string> = {
  add: "Add",
  fix: "Fix",
  check: "Check",
  stress: "Stress",
  read: "Read",
};

/** Add (a draft's notes) before fix before check before stress before read: what needs acting on first. */
const KIND_ORDER: Record<SuggestionKind, number> = { add: 0, fix: 1, check: 2, stress: 3, read: 4 };

/**
 * The short topic after the kind in a card's kicker, per rule. A rule missing
 * here — a new one, or an AI note with no rule — falls back to its name.
 */
const RULE_TOPIC: Record<string, string> = {
  cost_basis_equals_value: "cost basis",
  idle_bank_cash: "idle cash",
  unused_contribution_limits: "contributions",
  sweep_sells_while_cash: "sales you don't need",
  liability_payment_inflation_adjusted: "loan payment",
  unmapped_or_mismatched_assets: "return assumption",
  shortfall_account_concentration: "where it fails",
  success_vs_funding_gap: "success vs funding",
};

const SECTION_TOPIC: Record<SuggestionSection, string> = {
  portfolio: "portfolio",
  plan: "plan",
  results: "results",
};

export type CardAction =
  /**
   * Apply the note's one step to this plan. Nothing runs: several notes can be
   * applied first, then the plan re-run once to see their combined effect.
   */
  | "apply"
  /** Apply every remaining step of the selected path, in order, at once. */
  | "apply-path"
  /** Apply the selected path's steps through one of them (the step's button). */
  | "apply-step"
  /** Apply the whole selected path to a copy of this plan, left unrun too. */
  | "apply-copy"
  /**
   * A draft's check note only: the fact it asks about is right. A plan's
   * notes offer Dismiss alone, which the model is told about.
   */
  | "confirm"
  /** Set the note aside; it stays quiet next time, and the model is told. */
  | "dismiss"
  /** Simulate the selected path's steps against the run before deciding. */
  | "preview";

/** Where an evidence link goes. Ids are row ids, as the API speaks them. */
export type Destination =
  | { tab: "results"; year?: number }
  | { tab: "portfolio"; accountId: number }
  | { tab: "plan"; eventId: number };

export interface EvidenceLink {
  label: string;
  to?: Destination;
}

export interface DiffRow {
  label: string;
  /** What the plan has now; absent for an addition. */
  from?: string;
  /** What it would have; absent for a removal. */
  to?: string;
  /** Read off which sides are present: a new row, a removed one, or an edit. */
  change: "add" | "remove" | "edit";
}

/** One path in a note's picker. */
export interface PathRow {
  key: string;
  label: string;
  recommended: boolean;
  /** Its combined result in a phrase: "funding 96.1% · simulated", "est. funding ~93.0%". */
  headline?: string;
  /** "2 steps", for a path of more than one. */
  steps?: string;
  selected: boolean;
  /** Another path has been started, so this one can no longer be picked. */
  locked: boolean;
}

/** One ordered step of the selected path. */
export interface StepRow {
  key: string;
  /** 1-based, as the list numbers it. */
  number: number;
  title: string;
  reasoning?: string;
  diff: DiffRow[];
  applied: boolean;
  /** Where an applied step stands: "Applied · not yet run". */
  status?: string;
  /** The next step to apply: its button is live. */
  apply: boolean;
  /** A later step waits on this one: "after 1". */
  after?: number;
}

export interface CheckLine {
  text: string;
  /** Simulated against the run, rather than the author's estimate. */
  simulated: boolean;
  /** Why the simulated difference is not all the change's own. */
  caveat?: string;
  /**
   * What the line was measured against, once other notes have been applied
   * since: the reviewed run, not the plan as it now stands.
   */
  basis?: string;
}

/** One figure in a note's metric strip: what the plan has, and what the note would make it. */
export interface Metric {
  label: string;
  /** The reviewed run's figure; absent when the note quotes one figure alone. */
  from?: string;
  to: string;
  /** The author's estimate rather than a simulation. */
  estimate?: boolean;
}

export interface Card {
  id: number;
  kind: SuggestionKind;
  section: SuggestionSection;
  status: SuggestionStatus;
  kicker: string;
  title: string;
  body: string;
  /** The note's lead, which the detail shows open under the title. */
  summary: string;
  /** The working behind it, behind "Reasoning"; absent when the lead is all there is. */
  more?: string;
  /** The selected path's result in a line, for the note list: "funding 95.8% → 96.1%". */
  delta?: string;
  /** The selected path's result figure by figure, else the figures the note cites. */
  metrics: Metric[];
  /**
   * The paths to pick between, when the note offers more than one. Once a
   * step is applied the others are shown locked.
   */
  paths: PathRow[];
  /** The path the actions act on: the one started, else the one selected. */
  path?: string;
  /** Its label, where the card names it (several paths, or several steps). */
  pathLabel?: string;
  /** Why that path, when the note offers several and it says. */
  pathReasoning?: string;
  /** The selected path's steps, when it has more than one. */
  steps: StepRow[];
  /** The one step's diff, when the selected path has a single step. */
  diff: DiffRow[];
  /** The path's check, over all its steps. */
  check?: CheckLine;
  evidence: EvidenceLink[];
  actions: CardAction[];
  /** "AI" for model-written notes, so they read as such. */
  source?: string;
  /**
   * Where the note stands once acted on: "Applied · not yet run", "Added as
   * a scenario", "Started: Retire later · 1 of 2 steps applied".
   */
  applied?: string;
  /** When the note was applied, dismissed or confirmed; absent while open. */
  resolvedAt?: string;
  /** Why the other paths are closed, once one is started. */
  locked?: string;
  /** "Chat about this", when AI review is on; louder on a note with no change. */
  chat?: ChatOffer;
  /** The note a chat reply wrote this one under, when it was. */
  parentId?: number;
  /** Notes chat replies wrote under this one, oldest first, nested "From the chat". */
  children: Card[];
}

/**
 * The path a note's actions act on: the started one once any step is
 * applied; else the one the user picked, if the note still has it; else the
 * author's recommendation; else the first. None for a note with no paths.
 */
export function selectedPath(
  suggestion: Pick<Suggestion, "paths" | "applied_path">,
  chosen?: string,
): SuggestionPath | undefined {
  const { paths } = suggestion;
  const find = (key: string | null | undefined) => (key == null ? undefined : paths.find((p) => p.key === key));
  return find(suggestion.applied_path) ?? find(chosen) ?? paths.find((p) => p.recommended) ?? paths[0];
}

export interface Column {
  section: SuggestionSection;
  heading: string;
  cards: Card[];
}

export interface Board {
  /** "Last reviewed Oct 2 at 12:04 · 9 notes · 2 applied". */
  headline: string;
  /** Open notes first, fixes first; applied ones stay, after them. */
  columns: Column[];
  open: number;
  /** Steps applied to this plan (not to a copy). */
  applied: number;
  /** Of those, the ones no finished run is known to include. */
  pending: number;
  /** Notes set aside, which the board does not show. */
  resolved: number;
  /** Those notes as cards, for a list that keeps them, dimmed, with an undo. */
  setAside: Card[];
}

/** The newest finished run, as the run history holds it. */
export interface RunStamp {
  id: number;
  created_at: string;
}

/** Row names for evidence labels. A name absent from the live plan is left out. */
export interface Names {
  account: (id: number) => string | undefined;
  event: (id: number) => string | undefined;
}

const NO_NAMES: Names = { account: () => undefined, event: () => undefined };

/** A server timestamp ("2026-09-27 12:04:00", UTC) in epoch ms. */
function stampMs(stamp: string | null | undefined): number | undefined {
  if (!stamp) return undefined;
  const zoned = /[zZ]$|[+-]\d\d:?\d\d$/.test(stamp);
  const ms = new Date(zoned ? stamp : `${stamp.replace(" ", "T")}Z`).getTime();
  return Number.isNaN(ms) ? undefined : ms;
}

/**
 * When a review was written, for the Review header: "Last reviewed Oct 2 at
 * 13:40", in local time; the run's id when the stamp cannot be read.
 */
export function reviewedLine(review: Pick<Review, "reviewed_at" | "run_id">): string {
  const at = dateTime(review.reviewed_at);
  return at ? `Last reviewed ${at}` : `Last reviewed run #${review.run_id}`;
}

/** A server stamp as a local day, "Oct 2"; undefined when it cannot be read. */
export function shortDate(stamp: string | null | undefined): string | undefined {
  const ms = stampMs(stamp);
  return ms == null ? undefined : new Date(ms).toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

/** A server stamp as a local day and time, "Oct 2 at 13:40". */
export function dateTime(stamp: string | null | undefined): string | undefined {
  const day = shortDate(stamp);
  return day && `${day} at ${clockTime(stamp ?? undefined)}`;
}

/** The button a card action shows. No apply starts a run. */
export const ACTION_LABEL: Record<CardAction, string> = {
  apply: "Apply",
  "apply-path": "Apply path",
  "apply-step": "Apply",
  "apply-copy": "Add as scenario",
  confirm: "It's correct",
  dismiss: "Dismiss",
  preview: "Preview",
};

type Stepped = Pick<Suggestion, "kind" | "status" | "resolved_at" | "run_id" | "paths" | "applied_path">;

/** The steps of the started path already applied; none before one is. */
function appliedSteps(suggestion: Pick<Suggestion, "paths" | "applied_path">): SuggestionStep[] {
  const path = suggestion.paths.find((p) => p.key === suggestion.applied_path);
  return path ? path.steps.filter((s) => s.applied) : [];
}

/**
 * Stress notes are applied to a copy; every other kind to the plan itself.
 * The board counts only the latter as changes waiting for a re-run.
 */
function plannedSteps(suggestion: Stepped): SuggestionStep[] {
  return suggestion.kind === "stress" ? [] : appliedSteps(suggestion);
}

/**
 * Whether an applied step is still waiting for a run: "yes", "no", or
 * "unknown". A step's own `applied_at` decides against the latest run; older
 * notes without it fall back to the note's `resolved_at` once the whole path
 * is in, and part-way through are not claimed either way.
 */
function stepAwaits(suggestion: Stepped, step: SuggestionStep, latest?: RunStamp): "yes" | "no" | "unknown" {
  const applied = stampMs(
    step.applied_at ?? (suggestion.status === "applied" ? suggestion.resolved_at : null),
  );
  const ran = stampMs(latest?.created_at);
  if (applied != null) return ran == null || ran < applied ? "yes" : "no";
  if (latest == null || latest.id === suggestion.run_id) return "yes";
  return "unknown";
}

function runPhrase(latest?: RunStamp): string | undefined {
  const at = clockTime(latest?.created_at);
  return at ? `in the ${at} run` : undefined;
}

function stepStatus(suggestion: Stepped, step: SuggestionStep, latest?: RunStamp): string | undefined {
  if (!step.applied) return undefined;
  if (suggestion.kind === "stress") return "Added to the scenario";
  switch (stepAwaits(suggestion, step, latest)) {
    case "yes":
      return "Applied · not yet run";
    case "no": {
      const run = runPhrase(latest);
      return run ? `Applied · ${run}` : "Applied";
    }
    case "unknown":
      return "Applied";
  }
}

/**
 * Where a note stands once acted on, under its title. A note offering several
 * paths names the one taken: "Applied: Retire later · not yet run". A path
 * part-way applied says how far: "Started: Retire later · 1 of 2 steps applied".
 */
export function appliedLabel(suggestion: Stepped, latest?: RunStamp): string | undefined {
  const path = suggestion.paths.find((p) => p.key === suggestion.applied_path);
  const which = suggestion.paths.length > 1 ? path?.label : undefined;
  if (suggestion.status === "applied") {
    if (suggestion.kind === "stress") return which ? `Added as a scenario: ${which}` : "Added as a scenario";
    const applied = which ? `Applied: ${which}` : "Applied";
    const steps = path?.steps ?? [];
    const waits = steps.map((s) => stepAwaits(suggestion, s, latest));
    if (waits.includes("yes")) return `${applied} · not yet run`;
    if (waits.length > 0 && waits.every((w) => w === "no")) {
      const run = runPhrase(latest);
      return run ? `${applied} · ${run}` : applied;
    }
    return applied;
  }
  if (suggestion.status === "open" && path) {
    const done = path.steps.filter((s) => s.applied).length;
    if (done === 0) return undefined;
    const progress = `${fmtInt(done)} of ${plural(path.steps.length, "step")} applied`;
    return which ? `Started: ${which} · ${progress}` : `Started · ${progress}`;
  }
  return undefined;
}

export function kicker(suggestion: Pick<Suggestion, "kind" | "rule" | "section">): string {
  const topic =
    (suggestion.rule != null ? RULE_TOPIC[suggestion.rule] : undefined) ??
    suggestion.rule?.replace(/_/g, " ") ??
    SECTION_TOPIC[suggestion.section];
  return `${KIND_LABEL[suggestion.kind]} · ${topic}`;
}

/**
 * What a card offers, by kind, for the path in hand. Only open notes offer
 * anything, and only a path with changes left to make can be applied or
 * previewed. Once a step is applied the note is committed to its path: it
 * can be carried on, not confirmed, dismissed or copied.
 */
export function actionsFor(
  suggestion: Pick<Suggestion, "kind" | "status" | "applied_path">,
  path?: Pick<SuggestionPath, "steps" | "check">,
  { confirm = false }: { confirm?: boolean } = {},
): CardAction[] {
  if (suggestion.status !== "open") return [];
  const started = suggestion.applied_path != null;
  const remaining = (path?.steps ?? []).filter((s) => !s.applied);
  const editable = remaining.some((s) => s.changes.length > 0);
  const apply: CardAction = (path?.steps.length ?? 0) > 1 ? "apply-path" : "apply";
  const actions: CardAction[] = [];
  switch (suggestion.kind) {
    case "add":
    case "fix":
      if (editable) actions.push(apply);
      break;
    case "check":
      if (editable) actions.push(apply);
      if (confirm && !started) actions.push("confirm");
      break;
    case "stress":
      if (editable && !started) actions.push("apply-copy");
      break;
    case "read":
      break;
  }
  if (editable && !started && path?.check == null) actions.push("preview");
  if (!started) actions.push("dismiss");
  return actions;
}

/**
 * The metric a check line quotes: funding success when both sides measured
 * it, since the notes are mostly about running short of cash, and success
 * otherwise.
 */
function metric(base: PreviewStats, edited: PreviewStats): [string, number, number] {
  if (base.funding_success_rate != null && edited.funding_success_rate != null) {
    return ["funding", base.funding_success_rate, edited.funding_success_rate];
  }
  return ["success", base.success_rate, edited.success_rate];
}

const NOT_PAIRED =
  "Not paired: the change alters the simulated markets, so part of the difference is sampling.";

/** "funding 90.6% → 91.8% · simulated, 2,000 iterations", "… · all steps" for a path of several. */
export function simulatedLine(
  base: PreviewStats,
  edited: PreviewStats,
  iterations: number,
  paired: boolean,
  steps = 1,
): CheckLine {
  const [label, from, to] = metric(base, edited);
  return {
    text: `${label} ${fmtPercent(from)} → ${fmtPercent(to)} · simulated, ${fmtInt(iterations)} iterations${steps > 1 ? " · all steps" : ""}`,
    simulated: true,
    caveat: paired ? undefined : NOT_PAIRED,
  };
}

/** A path's check line: its combined simulation, else the author's estimate. */
export function checkLine(path: Pick<SuggestionPath, "check" | "estimate"> & { steps?: unknown[] }): CheckLine | undefined {
  const { check, estimate } = path;
  const steps = path.steps?.length ?? 1;
  const all = steps > 1 ? " · all steps" : "";
  if (check) return simulatedLine(check.base, check.edited, check.iterations, check.paired, steps);
  if (estimate?.funding_success_rate != null) {
    return { text: `est. funding ~${fmtPercent(estimate.funding_success_rate)}${all} · preview to simulate`, simulated: false };
  }
  if (estimate?.success_rate != null) {
    return { text: `est. success ~${fmtPercent(estimate.success_rate)}${all} · preview to simulate`, simulated: false };
  }
  return undefined;
}

/**
 * A path's combined result in a phrase, for its row in the picker: the
 * simulated outcome if it has been simulated, else the author's estimate.
 */
export function pathHeadline(path: Pick<SuggestionPath, "check" | "estimate">): string | undefined {
  const { check, estimate } = path;
  if (check) {
    const [label, , to] = metric(check.base, check.edited);
    return `${label} ${fmtPercent(to)} · simulated`;
  }
  if (estimate?.funding_success_rate != null) return `est. funding ~${fmtPercent(estimate.funding_success_rate)}`;
  if (estimate?.success_rate != null) return `est. success ~${fmtPercent(estimate.success_rate)}`;
  return undefined;
}

/** A preview's result, in the same words a card's own check uses. */
export function previewLine(preview: Preview, steps = 1): CheckLine | undefined {
  if (preview.base == null || preview.edited == null) return undefined;
  return simulatedLine(preview.base, preview.edited, preview.iterations, preview.paired, steps);
}

function humanize(name: string): string {
  const text = name.replace(/_/g, " ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** A stat or diagnostic value, read off its name: rates as %, money compact. */
function statValue(name: string, value: number): string {
  if (/rate|share|ratio/.test(name) && Math.abs(value) <= 1) return fmtPercent(value);
  if (/year/.test(name)) return String(Math.round(value));
  if (Math.abs(value) >= 1_000) return fmtCompact(value);
  return Number.isInteger(value) ? fmtInt(value) : value.toFixed(2);
}

export function evidenceLink(evidence: Evidence, names: Names = NO_NAMES): EvidenceLink {
  switch (evidence.ref) {
    case "ledger": {
      const event = evidence.event_id != null ? names.event(evidence.event_id) : undefined;
      const account = evidence.account_id != null ? names.account(evidence.account_id) : undefined;
      const about = event ?? account;
      return {
        label: `Ledger ${evidence.year}${about ? ` · ${about}` : ""}`,
        to: { tab: "results", year: evidence.year },
      };
    }
    case "account_series": {
      const account = names.account(evidence.account_id) ?? "Account";
      const year = evidence.date.slice(0, 4);
      return {
        label: `${account} · ${year}: ${fmtCompact(evidence.value)}`,
        to: { tab: "portfolio", accountId: evidence.account_id },
      };
    }
    case "stat":
      return { label: `${humanize(evidence.name)}: ${statValue(evidence.name, evidence.value)}` };
    case "diagnostic":
      return {
        label: `${humanize(evidence.field)}: ${statValue(evidence.field, evidence.value)}`,
        to: { tab: "results" },
      };
    case "document":
      return { label: `Document p. ${evidence.page}: “${clip(evidence.excerpt)}”` };
    case "answer":
      return { label: `Your answer: ${humanize(evidence.question_key)}` };
    case "description":
      return { label: `You wrote: “${clip(evidence.excerpt)}”` };
    case "computed":
      return { label: `Calculated: ${humanize(evidence.tool)}` };
  }
}

/** A quoted excerpt, cut to fit a chip. */
function clip(excerpt: string, max = 60): string {
  const text = excerpt.replace(/\s+/g, " ").trim();
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

export function diffRow(line: { label: string; from: string | null; to: string | null }): DiffRow {
  const from = line.from ?? undefined;
  const to = line.to ?? undefined;
  return { label: line.label, from, to, change: from == null ? "add" : to == null ? "remove" : "edit" };
}

/**
 * The selected path's steps as the list shows them: applied ones marked, the
 * next one applyable (for notes applied to the plan), later ones waiting on
 * the one before.
 */
function stepRows(suggestion: Stepped, path: SuggestionPath, latest?: RunStamp): StepRow[] {
  const stepwise = suggestion.status === "open" && (suggestion.kind === "fix" || suggestion.kind === "check");
  const next = path.steps.findIndex((s) => !s.applied);
  return path.steps.map((step, index) => ({
    key: step.key,
    number: index + 1,
    title: step.title,
    reasoning: step.reasoning ?? undefined,
    diff: step.diff.map(diffRow),
    applied: step.applied,
    status: stepStatus(suggestion, step, latest),
    apply: stepwise && index === next && step.changes.length > 0,
    after: stepwise && !step.applied && next >= 0 && index > next ? index : undefined,
  }));
}

export function toCard(
  suggestion: Suggestion,
  names: Names = NO_NAMES,
  {
    latest,
    basis,
    selected,
    chat = false,
    confirm = false,
  }: {
    latest?: RunStamp;
    basis?: string;
    selected?: string;
    chat?: boolean;
    /** Offer "It's correct" on check notes: a draft's, whose facts the person confirms. */
    confirm?: boolean;
  } = {},
): Card {
  const path = selectedPath(suggestion, selected);
  const check = path ? checkLine(path) : undefined;
  const started = suggestion.applied_path != null;
  const several = suggestion.paths.length > 1;
  const stepped = (path?.steps.length ?? 0) > 1;
  const picking = suggestion.status === "open" && several;
  const { summary, more } = leadOf(suggestion);
  const cited = citedMetrics(suggestion.evidence);
  return {
    id: suggestion.id,
    kind: suggestion.kind,
    section: suggestion.section,
    status: suggestion.status,
    kicker: kicker(suggestion),
    title: suggestion.title,
    body: suggestion.reasoning,
    summary,
    more,
    delta: (path && deltaLine(path)) ?? (cited[0] && `${cited[0].label} ${cited[0].to}`),
    metrics: (path && pathMetrics(path)) ?? cited,
    paths: picking
      ? suggestion.paths.map((p) => ({
          key: p.key,
          label: p.label,
          recommended: p.recommended,
          headline: pathHeadline(p),
          steps: p.steps.length > 1 ? plural(p.steps.length, "step") : undefined,
          selected: p.key === path?.key,
          locked: started && p.key !== suggestion.applied_path,
        }))
      : [],
    path: path?.key,
    pathLabel: path && (several || stepped) ? path.label : undefined,
    pathReasoning: several ? (path?.reasoning ?? undefined) : undefined,
    steps: path && stepped ? stepRows(suggestion, path, latest) : [],
    diff: path && !stepped ? (path.steps[0]?.diff ?? []).map(diffRow) : [],
    check: check && basis && suggestion.status === "open" ? { ...check, basis } : check,
    evidence: suggestion.evidence.map((e) => evidenceLink(e, names)),
    actions: actionsFor(suggestion, path, { confirm }),
    source: suggestion.source === "ai" ? "AI" : undefined,
    applied: appliedLabel(suggestion, latest),
    resolvedAt: suggestion.status === "open" ? undefined : (suggestion.resolved_at ?? undefined),
    locked:
      picking && started ? "The other paths are closed: steps from this one are already in the plan." : undefined,
    chat: chatOffer({ enabled: chat, paths: suggestion.paths.length }),
    parentId: parentOf(suggestion) ?? undefined,
    children: [],
  };
}

/**
 * The note a chat reply wrote this one under. Reviews stored before chat
 * existed may carry no `parent_id` at all.
 */
export function parentOf(suggestion: Pick<Suggestion, "parent_id">): number | null {
  return suggestion.parent_id ?? null;
}

/** Split the shown notes into those the board lists and those nested under a parent. */
function nest(shown: readonly Suggestion[]): { top: Suggestion[]; children: Map<number, Suggestion[]> } {
  const ids = new Set(shown.map((s) => s.id));
  const top: Suggestion[] = [];
  const children = new Map<number, Suggestion[]>();
  for (const s of shown) {
    const parent = parentOf(s);
    if (parent == null || parent === s.id || !ids.has(parent)) {
      top.push(s);
    } else {
      children.set(parent, [...(children.get(parent) ?? []), s]);
    }
  }
  // Oldest first: they read as the conversation went.
  for (const list of children.values()) list.sort((a, b) => a.id - b.id);
  // A cycle of parents would hide every note in it; list those at the top.
  const reachable = new Set<number>();
  const walk = (id: number) => {
    if (reachable.has(id)) return;
    reachable.add(id);
    for (const c of children.get(id) ?? []) walk(c.id);
  };
  for (const s of top) walk(s.id);
  for (const s of shown) if (!reachable.has(s.id)) {
    top.push(s);
    walk(s.id);
  }
  return { top, children };
}

function plural(count: number, one: string, many = `${one}s`): string {
  return `${fmtInt(count)} ${count === 1 ? one : many}`;
}

function byKind(suggestions: Suggestion[]): Suggestion[] {
  // Stable within a kind: the server's own order is the author's ranking.
  return suggestions
    .map((s, index) => ({ s, index }))
    .sort((a, b) => KIND_ORDER[a.s.kind] - KIND_ORDER[b.s.kind] || a.index - b.index)
    .map(({ s }) => s);
}

export function board(
  review: Review,
  {
    runCreatedAt,
    names = NO_NAMES,
    latest,
    selections = {},
  }: {
    runCreatedAt?: string;
    names?: Names;
    latest?: RunStamp;
    /** The path picked on each card, by suggestion id; unpicked cards default. */
    selections?: Readonly<Record<number, string>>;
  } = {},
): Board {
  const open = byKind(review.suggestions.filter((s) => s.status === "open"));
  // Applied notes stay on the board, marked, so a batch of them reads as one.
  const applied = byKind(review.suggestions.filter((s) => s.status === "applied"));
  const shown = [...open, ...applied];
  // Steps count, not notes: a path applied step by step is several changes.
  // Steps applied since this review: notes of its run, and paths still under
  // way. Notes applied in earlier reviews stay listed (as handled) but their
  // changes are in the plan the reviewed run already read.
  const planned = shown
    .filter((s) => s.run_id === review.run_id || s.status === "open")
    .flatMap((s) => plannedSteps(s).map((step) => ({ s, step })));
  const pending = planned.filter(({ s, step }) => stepAwaits(s, step, latest) === "yes").length;
  const at = clockTime(runCreatedAt) ?? clockTime(review.reviewed_at);
  const run = at ? `the ${at} run` : `run #${review.run_id}`;
  // Checks on the open notes were simulated against the reviewed run alone.
  const basis = planned.length > 0 ? `simulated against ${run}, before the applied changes` : undefined;
  const openNotes = open.filter((s) => s.applied_path == null).length;
  const chat = review.ai != null;
  // Notes a chat reply wrote sit under the note they answer, not beside it;
  // one whose parent is not on the board (set aside) stands on its own.
  const nested = nest(shown);
  const card = (s: Suggestion, seen: ReadonlySet<number> = new Set()): Card => {
    const within = new Set(seen).add(s.id);
    return {
      ...toCard(s, names, { latest, basis, selected: selections[s.id], chat }),
      children: (nested.children.get(s.id) ?? []).filter((c) => !within.has(c.id)).map((c) => card(c, within)),
    };
  };
  return {
    headline: `${reviewedLine(review)} · ${plural(openNotes, "note")}${planned.length > 0 ? ` · ${fmtInt(planned.length)} applied` : ""}`,
    columns: SECTIONS.map(({ id, heading }) => ({
      section: id,
      heading,
      cards: nested.top.filter((s) => s.section === id).map((s) => card(s)),
    })),
    open: open.length,
    applied: planned.length,
    pending,
    resolved: review.suggestions.length - shown.length,
    setAside: byKind(review.suggestions.filter((s) => s.status === "dismissed" || s.status === "confirmed")).map(
      (s) => toCard(s, names, { latest, selected: selections[s.id], chat }),
    ),
  };
}

/** Where the model-written half of the review stands, as the header says it. */
export interface AiLine {
  text: string;
  /** Still writing: the board fills in as it finishes. */
  running: boolean;
}

/**
 * The header line for the review's model-written pass: nothing when the
 * server has no review model, or when the pass finished without comment.
 */
export function aiLine(review: Pick<Review, "ai">): AiLine | undefined {
  const ai = review.ai;
  if (ai == null) return undefined;
  switch (ai.status) {
    case "running":
      return { text: reviewActivity(ai.activity), running: true };
    case "failed":
      return {
        text: `The AI review did not finish${ai.error ? `: ${ai.error}` : ""}. The rule notes below are unaffected; review again to retry.`,
        running: false,
      };
    case "done":
      return ai.error
        ? { text: `The AI review stopped early: ${ai.error}. Its notes so far are shown.`, running: false }
        : undefined;
  }
}

/** What the banner under the header says, and the one action it offers. */
export interface ReviewBanner {
  text: string;
  /** "rerun" starts a run of the plan; "review" writes a fresh review. */
  action?: "rerun" | "review";
}

/**
 * The banner under the Review header: applied changes waiting for a re-run,
 * a run of them under way, or a newer run the review has not read yet.
 * Nothing when the review still describes the plan and its latest run.
 */
export function reviewBanner({
  reviewRunId,
  latestRunId,
  planChanged,
  running,
  applied,
  pending,
  latestAt,
}: {
  reviewRunId: number;
  latestRunId?: number;
  /** The plan was edited after the latest run. */
  planChanged: boolean;
  /** A run of this plan is under way. */
  running: boolean;
  /** Notes applied to the plan since the reviewed run. */
  applied: number;
  /** Of those, the ones no finished run includes yet. */
  pending: number;
  /** When the latest finished run was made, for "the 12:31 run". */
  latestAt?: string;
}): ReviewBanner | undefined {
  if (running) {
    return {
      text: pending > 0 ? `Running the plan with ${plural(pending, "applied change")}…` : "Running the plan…",
    };
  }
  if (planChanged) {
    return pending > 0
      ? { text: `${plural(pending, "change")} applied · Re-run to see the combined effect`, action: "rerun" }
      : {
          text: "The plan changed since the reviewed run, so some notes may no longer apply. Re-run, then review again.",
          action: "rerun",
        };
  }
  if (latestRunId != null && latestRunId !== reviewRunId) {
    const at = clockTime(latestAt);
    const run = at ? `The ${at} run` : "A newer run";
    return applied > 0
      ? { text: `${run} includes ${plural(applied, "applied change")}. Review again to check it.`, action: "review" }
      : { text: `${run} has finished since this review. Review again to check it.`, action: "review" };
  }
  return undefined;
}

/** The heading over a refused apply's problems. */
export function refusalHeading({ stale, conflict }: { stale?: boolean; conflict?: boolean }): string {
  if (stale && conflict) return "An earlier change touched the same field.";
  if (stale) return "The plan changed since this note was written.";
  return "These changes no longer fit the plan.";
}

/** One sentence per reason a note's changes could not be applied. */
export function problemText(problem: ChangeProblem): string {
  switch (problem.kind) {
    case "stale":
      return `${problem.path || "This item"} changed since the note was written.`;
    case "bad_path":
      return `${problem.path || "The edit"} no longer fits the plan: ${problem.reason}.`;
    case "invalid_body":
      return `The edited ${targetNoun(problem.target)} would not be valid: ${problem.message}.`;
    case "unknown_target":
      return `The ${targetNoun(problem.target)} this note edits no longer exists.`;
    case "unsupported_op":
      return `This change cannot be applied here: ${problem.reason}.`;
    case "duplicate_key":
      return `Two new items share the name "${problem.key}".`;
    case "unknown_reference":
      return `A change refers to "${problem.key}", which nothing in this note creates.`;
    case "wrong_reference_kind":
      return `"${problem.key}" is ${article(problem.found)}, but ${problem.field} needs ${problem.expected ? article(problem.expected) : "something else"}.`;
    case "reference_cycle":
      return `New items refer to each other in a loop: ${problem.keys.join(" → ")}.`;
  }
}

function article(kind: string): string {
  return `${/^[aeiou]/.test(kind) ? "an" : "a"} ${kind}`;
}

/** Keyed loosely: a target kind this build does not know yet reads as its own name. */
function targetNoun(target: ChangeTarget | Record<string, unknown>): string {
  // The plan's own settings are a bare string, not a one-key object.
  if (typeof target === "string") return target === "scenario" ? "plan setting" : String(target).replace(/_/g, " ");
  const kind = Object.keys(target)[0] ?? "";
  if (kind === "event" || kind === "new_event") return "event";
  if (kind === "asset" || kind === "new_asset") return "asset";
  if (kind === "account" || kind === "new_account") return "account";
  if (kind === "parameter" || kind === "new_parameter") return "parameter";
  return kind.replace(/^new_/, "").replace(/_/g, " ") || "item";
}

/** A refused change, and the step it belongs to when the server says. */
export interface StepProblem {
  problem: ChangeProblem;
  /** The step's key. */
  step?: string;
}

type Group = { path?: unknown; step?: unknown; problems?: unknown };

function listOf(value: unknown): ChangeProblem[] {
  return Array.isArray(value) ? (value as ChangeProblem[]) : [];
}

/**
 * The problems a refused apply or preview carries, each with its step. The
 * server lists every problem (`problems`) and the same ones grouped by the
 * path and step they concern (`by_step`); where exactly is read loosely, so a
 * reshuffle of the envelope degrades to the error's message rather than to
 * nothing.
 *
 * With `path`, only that path's problems. Without the grouping, every problem
 * is shown, since they cannot be told apart.
 */
export function stepProblemsIn(body: unknown, path?: string): StepProblem[] {
  if (body == null || typeof body !== "object") return [];
  const record = body as { problems?: unknown; by_step?: unknown; error?: { problems?: unknown; by_step?: unknown } };
  const groups = record.by_step ?? record.error?.by_step;
  if (Array.isArray(groups) && groups.length > 0) {
    return (groups as Group[])
      .filter((g) => path == null || g?.path === path)
      .flatMap((g) =>
        listOf(g?.problems).map((problem) => ({ problem, step: typeof g?.step === "string" ? g.step : undefined })),
      );
  }
  return listOf(record.problems ?? record.error?.problems).map((problem) => ({ problem }));
}

/** The problems alone; see `stepProblemsIn`. */
export function problemsIn(body: unknown, path?: string): ChangeProblem[] {
  return stepProblemsIn(body, path).map((p) => p.problem);
}

/** A problem as a sentence, named by its step when the path has several: "Step 2: …". */
export function stepProblemText({ problem, step }: StepProblem, steps: ReadonlyArray<{ key: string }> = []): string {
  const index = step == null ? -1 : steps.findIndex((s) => s.key === step);
  const text = problemText(problem);
  return steps.length > 1 && index >= 0 ? `Step ${index + 1}: ${text}` : text;
}

/** How many open notes edit each row, for the "1 review note →" hooks. */
export interface NoteCounts {
  events: Map<number, number>;
  assets: Map<number, number>;
  accounts: Map<number, number>;
}

export function noteCounts(suggestions: readonly Suggestion[]): NoteCounts {
  const counts: NoteCounts = { events: new Map(), assets: new Map(), accounts: new Map() };
  for (const suggestion of suggestions) {
    if (suggestion.status !== "open") continue;
    // A note editing one row twice — in two steps, or two paths — is still
    // one note about it.
    const seen = new Set<string>();
    for (const { target } of suggestion.paths.flatMap((p) => p.steps.flatMap((s) => s.changes))) {
      // `"scenario"` is the plan's own settings, a row of none of these maps.
      if (typeof target === "string") continue;
      const [map, id] =
        "event" in target
          ? [counts.events, target.event]
          : "asset" in target
            ? [counts.assets, target.asset]
            : "account" in target
              ? [counts.accounts, target.account]
              : [undefined, undefined];
      if (map == null || id == null) continue;
      const key = `${Object.keys(target)[0]}:${id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      map.set(id, (map.get(id) ?? 0) + 1);
    }
  }
  return counts;
}

export function noteLink(count: number | undefined): string | undefined {
  if (!count) return undefined;
  return `${plural(count, "review note")} →`;
}

// ── The note list and detail (design: Review notes) ────────────────────────

/** Reasoning up to this long reads whole; a longer one leads with its first sentence. */
const SUMMARY_MAX = 220;

/** A lead shorter than this is a fragment, not a summary. */
const SUMMARY_MIN = 40;

/** Words whose full stop does not end a sentence. */
const ABBREVIATION = /(?:^|[\s(])(?:Inc|Corp|Co|Ltd|Mr|Mrs|Ms|Dr|St|vs|etc|approx|e\.g|i\.e|U\.S)$/;

/**
 * A note's reasoning cut for a note with no summary of its own: its first
 * sentence open, the rest behind "Reasoning". A sentence ends at `.`, `!` or `?` before a space
 * and a capital, quote or bracket — so "$1.5M" and "6.00%" stay whole — and
 * not after an abbreviation ("Inc.") or inside brackets.
 */
export function splitReasoning(text: string): { summary: string; more?: string } {
  const trimmed = text.trim();
  if (trimmed.length <= SUMMARY_MAX) return { summary: trimmed };
  for (const end of trimmed.matchAll(/[.!?](?=\s+[A-Z“"‘'(])/g)) {
    const summary = trimmed.slice(0, end.index + 1);
    if (summary.length < SUMMARY_MIN || ABBREVIATION.test(summary.slice(0, -1))) continue;
    if (summary.split("(").length !== summary.split(")").length) continue;
    const more = trimmed.slice(end.index + 1).trim();
    return more ? { summary, more } : { summary };
  }
  return { summary: trimmed };
}

/**
 * A note's lead and what sits behind "Reasoning": the author's own summary
 * over the whole reasoning, or for a note stored before authors wrote one,
 * its reasoning cut at the first sentence.
 */
export function leadOf(suggestion: Pick<Suggestion, "summary" | "reasoning">): { summary: string; more?: string } {
  const summary = suggestion.summary?.trim();
  if (!summary) return splitReasoning(suggestion.reasoning);
  const more = suggestion.reasoning.trim();
  return more && more !== summary ? { summary, more } : { summary };
}

/** A path's result in a line for the list: simulated "a → b", else the estimate. */
export function deltaLine(path: Pick<SuggestionPath, "check" | "estimate">): string | undefined {
  if (path.check) {
    const [label, from, to] = metric(path.check.base, path.check.edited);
    return `${label} ${fmtPercent(from)} → ${fmtPercent(to)}`;
  }
  return pathHeadline(path);
}

/**
 * A path's result figure by figure: the rate it quotes, success beside it,
 * the 5th-percentile real final net worth and the median first shortfall
 * where both sides measured them. An estimate gives its rates alone.
 */
export function pathMetrics(path: Pick<SuggestionPath, "check" | "estimate">): Metric[] | undefined {
  const { check, estimate } = path;
  if (check) {
    const { base, edited } = check;
    const out: Metric[] = [];
    if (base.funding_success_rate != null && edited.funding_success_rate != null) {
      out.push({ label: "Funding", from: fmtPercent(base.funding_success_rate), to: fmtPercent(edited.funding_success_rate) });
    }
    out.push({ label: "Success", from: fmtPercent(base.success_rate), to: fmtPercent(edited.success_rate) });
    if (base.real_final && edited.real_final) {
      out.push({ label: "P5 real final", from: fmtCompact(base.real_final.p5), to: fmtCompact(edited.real_final.p5) });
    }
    const was = base.funding?.median_first_shortfall_year;
    const now = edited.funding?.median_first_shortfall_year;
    if (was != null || now != null) {
      out.push({
        label: "First shortfall",
        from: was != null ? String(was) : "none",
        to: now != null ? String(now) : "none",
      });
    }
    return out;
  }
  const out: Metric[] = [];
  if (estimate?.funding_success_rate != null) {
    out.push({ label: "Funding", to: `~${fmtPercent(estimate.funding_success_rate)}`, estimate: true });
  }
  if (estimate?.success_rate != null) {
    out.push({ label: "Success", to: `~${fmtPercent(estimate.success_rate)}`, estimate: true });
  }
  return out.length > 0 ? out : undefined;
}

/** The figures a note cites as evidence, for a note with no path to measure. */
export function citedMetrics(evidence: readonly Evidence[], max = 4): Metric[] {
  const out: Metric[] = [];
  for (const e of evidence) {
    if (e.ref === "stat") out.push({ label: humanize(e.name), to: statValue(e.name, e.value) });
    else if (e.ref === "diagnostic") out.push({ label: humanize(e.field), to: statValue(e.field, e.value) });
    if (out.length === max) break;
  }
  return out;
}

/**
 * The pill a handled note carries in the list: done (applied, confirmed, or
 * read), dismissed, or a path part-way applied.
 */
export interface RowStatus {
  label: string;
  tone: "done" | "dismissed" | "started";
  /** The day it was handled, "Oct 2", when known. */
  date?: string;
}

export function listStatus(card: Pick<Card, "kind" | "status" | "applied" | "resolvedAt">): RowStatus | undefined {
  const status = baseStatus(card);
  const date = shortDate(card.resolvedAt);
  return status && date ? { ...status, date } : status;
}

function baseStatus(card: Pick<Card, "kind" | "status" | "applied">): RowStatus | undefined {
  switch (card.status) {
    case "applied":
      return { label: card.kind === "stress" ? "✓ Added" : "✓ Applied", tone: "done" };
    case "confirmed":
      return { label: "✓ Correct", tone: "done" };
    case "dismissed":
      // "Got it" on a read note sets it aside: it was read, not refused.
      return card.kind === "read" ? { label: "✓ Read", tone: "done" } : { label: "Dismissed", tone: "dismissed" };
    case "open":
      return card.applied ? { label: "Started", tone: "started" } : undefined;
  }
}

/** Where a closed note stands, in the detail's action row; undefined while open. */
export function closedText(card: Pick<Card, "kind" | "status" | "applied" | "resolvedAt">): string | undefined {
  const at = dateTime(card.resolvedAt);
  const on = at ? ` on ${at}` : "";
  switch (card.status) {
    case "applied": {
      if (!card.applied) return card.kind === "stress" ? `Added as a scenario${on}.` : `Applied to the plan${on}.`;
      // "Applied: Retire later · not yet run" → "Applied: Retire later on Oct 2 at 13:40 · not yet run".
      const [lead, ...rest] = card.applied.split(" · ");
      return [`${lead}${on}`, ...rest].join(" · ");
    }
    case "confirmed":
      return `Marked as correct${on}.`;
    case "dismissed":
      return card.kind === "read" ? `Marked as read${on}.` : `Dismissed${on}.`;
    case "open":
      return undefined;
  }
}

/** A closed note the user can take back: one set aside, not one in the plan. */
export function undoable(card: Pick<Card, "status">): boolean {
  return card.status === "dismissed" || card.status === "confirmed";
}

export interface DetailAction {
  action: CardAction;
  label: string;
}

/**
 * The detail's buttons: one primary — the note's apply, else for a note with
 * nothing to apply its Dismiss ("Got it" on a read note) — the rest as quiet
 * buttons, and Dismiss as a link beside an apply.
 */
export function detailActions(card: Pick<Card, "kind" | "actions">): {
  primary?: DetailAction;
  secondary: DetailAction[];
  dismiss: boolean;
} {
  const has = (a: CardAction) => card.actions.includes(a);
  const applying = card.actions.find((a) => a === "apply" || a === "apply-path" || a === "apply-copy");
  let primary: DetailAction | undefined;
  if (applying) primary = { action: applying, label: ACTION_LABEL[applying] };
  else if (has("dismiss")) primary = { action: "dismiss", label: card.kind === "read" ? "Got it" : ACTION_LABEL.dismiss };
  const secondary = card.actions
    .filter((a) => a !== primary?.action && a !== "dismiss")
    .map((action) => ({ action, label: ACTION_LABEL[action] }));
  return { primary, secondary, dismiss: has("dismiss") && primary?.action !== "dismiss" };
}

/** Which notes the list holds: those still to act on, those acted on, or every one. */
export type NoteView = "open" | "handled" | "all";

/** Acted on: applied, confirmed or dismissed. A path part-way applied is still open. */
export function handled(card: Pick<Card, "status">): boolean {
  return card.status !== "open";
}

/** One row of the note list; a note a chat reply wrote sits under its parent. */
export interface NoteRow {
  card: Card;
  depth: number;
  status?: RowStatus;
  /** Acted on: its dot greys out. */
  closed: boolean;
}

export interface NoteList {
  views: Array<{ id: NoteView; label: string; count: number }>;
  /** A chip per kind of note, counted within the view; off ones narrow nothing. */
  kinds: Array<{ id: SuggestionKind; label: string; count: number; on: boolean }>;
  /** The visible notes, by section, as the list draws them. */
  /**
   * The visible notes as the list draws them: under section headings, or in
   * the Handled view as one list with no heading, newest first.
   */
  groups: Array<{ key: string; heading?: string; rows: NoteRow[] }>;
  /** Why the list is empty, when it is. */
  empty?: string;
  /** Every note in list order, filters or not: "6 of 9" counts over it. */
  all: Card[];
  /** The visible ones, in order: what j and k step through. */
  visible: Card[];
}

const VIEWS: ReadonlyArray<{ id: NoteView; label: string }> = [
  { id: "open", label: "Open" },
  { id: "handled", label: "Handled" },
  { id: "all", label: "All" },
];

/** The kind chips, most pressing first; "add" only appears on a board that has one. */
const KIND_CHIPS: readonly SuggestionKind[] = ["add", "fix", "stress", "check", "read"];

/**
 * The note list beside the detail: the board's notes by section — open,
 * then applied, then those set aside — each followed by the notes chat
 * replies wrote under it, narrowed to a view (open, handled, all) and, when
 * any kind chip is on, to those kinds.
 *
 * `keep` stays in the list whatever the view: the note in hand, so acting on
 * it (or taking that back) does not snatch it away mid-read.
 */
export function noteList(
  view: Pick<Board, "columns" | "setAside">,
  {
    show = "open",
    kinds = [],
    keep,
  }: { show?: NoteView; kinds?: readonly SuggestionKind[]; keep?: number } = {},
): NoteList {
  const bySection = SECTIONS.map(({ id, heading }) => {
    const rows: NoteRow[] = [];
    const add = (card: Card, depth: number) => {
      rows.push({ card, depth, status: listStatus(card), closed: handled(card) });
      card.children.forEach((child) => add(child, depth + 1));
    };
    view.columns.find((c) => c.section === id)?.cards.forEach((card) => add(card, 0));
    view.setAside.filter((card) => card.section === id).forEach((card) => add(card, 0));
    return { section: id, heading, rows };
  });
  const rows = bySection.flatMap((g) => g.rows);
  const inView = (card: Card, v: NoteView) =>
    v === "all" || card.id === keep || (v === "handled") === handled(card);
  const pool = rows.filter((r) => inView(r.card, show));
  const shown = (r: NoteRow) => inView(r.card, show) && (kinds.length === 0 || kinds.includes(r.card.kind));
  const groups =
    show === "handled"
      ? byDate(rows.filter(shown))
      : bySection
          .map((g) => ({ key: g.section, heading: g.heading, rows: g.rows.filter(shown) }))
          .filter((g) => g.rows.length > 0);
  const visible = groups.flatMap((g) => g.rows.map((r) => r.card));
  return {
    views: VIEWS.map((v) => ({
      ...v,
      count: v.id === "all" ? rows.length : rows.filter((r) => (v.id === "handled") === handled(r.card)).length,
    })),
    kinds: KIND_CHIPS.filter((k) => k !== "add" || rows.some((r) => r.card.kind === "add")).map((k) => ({
      id: k,
      label: KIND_LABEL[k],
      count: pool.filter((r) => r.card.kind === k).length,
      on: kinds.includes(k),
    })),
    groups,
    empty:
      visible.length > 0
        ? undefined
        : show === "handled" && kinds.length === 0
          ? "Nothing dismissed or applied yet."
          : show === "open" && kinds.length === 0
            ? "Nothing left to act on."
            : "No notes match these filters.",
    all: rows.map((r) => r.card),
    visible,
  };
}

/**
 * Handled notes as one plain list, by when they were handled, newest first.
 * A note taken back while in view (open again, kept) heads it, as the one in
 * hand; one with no date closes it.
 */
function byDate(rows: readonly NoteRow[]): NoteList["groups"] {
  const when = (r: NoteRow) => stampMs(r.card.resolvedAt);
  const rank = (r: NoteRow) => (!r.closed ? 0 : when(r) == null ? 2 : 1);
  const sorted = [...rows].sort(
    (a, b) => rank(a) - rank(b) || (when(b) ?? 0) - (when(a) ?? 0) || b.card.id - a.card.id,
  );
  return sorted.length > 0 ? [{ key: "handled", rows: sorted.map((r) => ({ ...r, depth: 0 })) }] : [];
}

/** The note to show: the one picked, if it is still visible; else the first open one; else the first. */
export function pickedNote(visible: readonly Card[], picked?: number): Card | undefined {
  return visible.find((c) => c.id === picked) ?? visible.find((c) => c.status === "open") ?? visible[0];
}

/**
 * The note after `current` for "Next note": the next open one in the list,
 * wrapping round; the plain next one once none is open. `step` -1 goes back,
 * one note at a time.
 */
export function steppedNote(visible: readonly Card[], current: number | undefined, step: 1 | -1 = 1): Card | undefined {
  const at = visible.findIndex((c) => c.id === current);
  const n = visible.length;
  if (n === 0) return undefined;
  if (step === -1) return visible[(Math.max(at, 0) - 1 + n) % n];
  for (let i = 1; i < n; i++) {
    const card = visible[(at + i + n) % n];
    if (card.status === "open") return card;
  }
  return visible[(at + 1) % n];
}
