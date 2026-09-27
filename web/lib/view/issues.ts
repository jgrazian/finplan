import type { PreflightIssue } from "../api/generated/PreflightIssue.ts";
import type { PreflightReport } from "../api/generated/PreflightReport.ts";
import type { Run } from "../api/generated/Run.ts";
import { fmtCompact, fmtPercent } from "../format.ts";
import type {
  AccountFlavorKind,
  AccountSeries,
  SimulationWarning,
  YearlyCashFlow,
} from "../types.ts";

/**
 * The Results screen's issue summary (design 14a).
 *
 * Three sources, one strip: the latest run failing outright, the plan
 * changing under the results (with whatever the preflight says blocks the
 * next run), and the loaded run's iterations that failed the cash funding
 * check. The strip shows the most severe; the drawer lists all of them.
 *
 * Below those, what the path on screen shows: events that failed on it, and
 * two checks read off its cash flows — a withdrawal rate that runs high, and
 * cash that piles up uninvested.
 */
export type StripTone = "failed" | "changed" | "clean";

export interface IssueStrip {
  tone: StripTone;
  /** Upper-case lead, e.g. `Plan changed · 2 issues`. */
  label: string;
  message: string;
  /** Secondary fact after the divider, e.g. the shortfall count. */
  aside?: string;
}

export interface RunError {
  message: string;
  detail?: string;
}

export interface Shortfall {
  failed: number;
  total: number;
  /** Selected path's first cash shortfall, verbatim from its warning. */
  firstInPath?: string;
  /** Warnings on the selected path that fail the funding check. */
  pathWarnings: number;
}

/** An effect or trigger that failed on the path on screen. */
export interface EventFailure {
  key: string;
  /** Row id, to open the event in Plan. */
  eventId?: number;
  /** The event's name, or a fallback when it has none to give. */
  event: string;
  date?: string;
  /** Server-rendered text: accounts are named, not numbered. */
  message: string;
}

/** Something the path on screen shows that is worth a second look. */
export interface PathCheck {
  code: "withdrawal-rate" | "idle-cash";
  title: string;
  detail: string;
}

export interface IssueSummary {
  strip?: IssueStrip;
  runError?: RunError;
  blocking: PreflightIssue[];
  review: PreflightIssue[];
  shortfall?: Shortfall;
  /** Label of the path the last two lists describe, e.g. `P50 path`. */
  pathLabel?: string;
  eventFailures: EventFailure[];
  checks: PathCheck[];
  /** Everything the drawer lists; drives its heading and the badge. */
  count: number;
}

/** Select observations by their stored kind, never by interpreting warning prose. */
export function fundingDiagnostics(warnings: SimulationWarning[]) {
  return {
    shortfalls: warnings.filter((warning) => warning.kind === "CashShortfall"),
    processing: warnings.filter((warning) =>
      warning.kind === "EffectSkipped" || warning.kind === "EvaluationFailed"),
  };
}

const count = (n: number, one: string, many = `${one}s`) =>
  `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

/** Codes the preflight always raises; reminders, not problems with this plan. */
const REMINDERS = new Set(["assumptions", "horizon"]);

export function summarizeIssues({
  run,
  preflight,
  stale,
  lastRunAt,
  fundingSuccessRate,
  iterations,
  warnings,
  pathLabel,
  checks = [],
  names,
}: {
  /** The scenario's newest run, whatever its status. */
  run: Pick<Run, "status" | "error_message" | "completed_iterations" | "iterations" | "max_iterations" | "converge"> | undefined;
  preflight: PreflightReport | undefined;
  /** Loaded results predate the plan's current inputs. */
  stale: boolean;
  /** Clock time the loaded results were queued, e.g. `12:04`. */
  lastRunAt?: string;
  fundingSuccessRate?: number;
  iterations?: number;
  warnings: SimulationWarning[];
  pathLabel?: string;
  checks?: PathCheck[];
  /** Row id → name, for the events and accounts warnings refer to. */
  names?: {
    event: (id: number) => string | undefined;
    account: (id: number) => string | undefined;
  };
}): IssueSummary {
  const blocking = preflight?.issues.filter((issue) => issue.severity === "error") ?? [];
  const review =
    preflight?.issues.filter(
      (issue) => issue.severity !== "error" && !REMINDERS.has(issue.code),
    ) ?? [];

  const runError: RunError | undefined =
    run?.status === "failed"
      ? {
          message: failedMessage(run),
          detail: run.error_message ?? undefined,
        }
      : undefined;

  let shortfall: Shortfall | undefined;
  if (fundingSuccessRate != null && iterations != null && iterations > 0) {
    const failed = iterations - Math.round(iterations * fundingSuccessRate);
    if (failed > 0) {
      const { shortfalls, processing } = fundingDiagnostics(warnings);
      const first = shortfalls[0];
      const account = first?.accountId != null ? names?.account(first.accountId) : undefined;
      shortfall = {
        failed,
        total: iterations,
        firstInPath: first && (first.date && account ? `${first.date} · ${account}` : first.detail),
        pathWarnings: shortfalls.length + processing.length,
      };
    }
  }

  const eventFailures: EventFailure[] = fundingDiagnostics(warnings).processing.map((w) => ({
    key: w.id,
    eventId: w.eventId,
    event:
      (w.eventId != null ? names?.event(w.eventId) : undefined) ??
      (w.eventId != null ? "A deleted event" : "A scheduled payment"),
    date: w.date,
    message: w.message ?? w.detail,
  }));

  const shortAside = shortfall
    ? `${shortfall.failed.toLocaleString("en-US")} of ${shortfall.total.toLocaleString("en-US")} iterations failed the cash funding check`
    : undefined;
  const since = lastRunAt ? ` (${lastRunAt})` : "";

  let strip: IssueStrip | undefined;
  if (runError) {
    strip = { tone: "failed", label: "Run failed", message: runError.detail ?? runError.message };
  } else if (stale || blocking.length > 0) {
    const lead = stale ? "Plan changed" : "Plan";
    strip = {
      tone: "changed",
      label: blocking.length > 0 ? `${lead} · ${count(blocking.length, "issue")}` : lead,
      message:
        blocking.length > 0
          ? stale
            ? `Results below are from the last valid run${since}. Fix the issues to re-run.`
            : "Fix the issues before the next run."
          : `Results below are from the last run${since}. Re-run to update them.`,
      aside: shortAside,
    };
  } else if (shortfall) {
    strip = {
      tone: "clean",
      label: "Ran clean",
      message: shortAside!,
    };
  } else if (checks.length > 0) {
    strip = {
      tone: "clean",
      label: "Ran clean",
      message: `${count(checks.length, "thing")} worth checking on the ${pathLabel ?? "path shown"}`,
    };
  }

  return {
    strip,
    runError,
    blocking,
    review,
    shortfall,
    pathLabel,
    eventFailures,
    checks,
    count:
      (runError ? 1 : 0) +
      blocking.length +
      review.length +
      (shortfall ? 1 : 0) +
      eventFailures.length +
      checks.length,
  };
}

function failedMessage(
  run: Pick<Run, "completed_iterations" | "iterations" | "max_iterations" | "converge">,
): string {
  const total = (run.converge ? run.max_iterations : run.iterations) ?? run.iterations;
  return run.completed_iterations > 0
    ? `The engine stopped after ${run.completed_iterations.toLocaleString("en-US")} of ${total.toLocaleString("en-US")} iterations.`
    : "The engine stopped before the first iteration finished.";
}

/**
 * A server stamp as local clock time, `12:04`. SQLite's `datetime('now')`
 * writes UTC without a zone, so a zoneless stamp is read as UTC.
 */
export function clockTime(stamp: string | undefined): string | undefined {
  if (!stamp) return undefined;
  const zoned = /[zZ]$|[+-]\d\d:?\d\d$/.test(stamp);
  const date = new Date(zoned ? stamp : `${stamp.replace(" ", "T")}Z`);
  if (Number.isNaN(date.getTime())) return undefined;
  return date.toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false });
}

/**
 * Above this share of invested balances, a year's withdrawals are worth a
 * look: the common rule of thumb for a portfolio meant to last thirty years.
 */
export const WITHDRAWAL_WATCH = 0.04;
/** Cash beyond this many years of spending is more buffer than a plan needs. */
export const IDLE_CASH_YEARS = 2;
/** ...and it has to keep growing this many years running to count as idle. */
export const IDLE_CASH_RUN = 5;

/**
 * Checks read off one path's yearly figures. Both are about the plan's
 * shape rather than luck, so the median path is a fair place to look.
 */
export function pathChecks({
  years,
  cashFlows,
  accountSeries,
  flavorOf,
}: {
  /** The years `accountSeries` values are indexed by, each a year-end. */
  years: number[];
  cashFlows: Pick<YearlyCashFlow, "year" | "withdrawals" | "expenses">[];
  accountSeries: Pick<AccountSeries, "accountId" | "label" | "values">[];
  flavorOf: (accountId: string) => AccountFlavorKind | undefined;
}): PathCheck[] {
  const checks: PathCheck[] = [];
  const index = new Map(years.map((year, i) => [year, i]));
  const ofFlavor = (flavor: AccountFlavorKind) =>
    accountSeries.filter((s) => flavorOf(s.accountId) === flavor);

  // Withdrawal rate: a year's withdrawals over the invested balance it
  // started from, i.e. the prior year-end.
  const invested = ofFlavor("Investment");
  const rates = cashFlows.flatMap((flow) => {
    const i = index.get(flow.year - 1);
    if (i == null || flow.withdrawals <= 0) return [];
    const base = invested.reduce((sum, s) => sum + (s.values[i] ?? 0), 0);
    return base > 0 ? [{ year: flow.year, rate: flow.withdrawals / base, drawn: flow.withdrawals, base }] : [];
  });
  const high = rates.filter((r) => r.rate > WITHDRAWAL_WATCH);
  if (high.length > 0) {
    const peak = high.reduce((a, b) => (b.rate > a.rate ? b : a));
    checks.push({
      code: "withdrawal-rate",
      title: `Withdrawals reach ${fmtPercent(peak.rate)} of invested balances`,
      detail:
        `${peak.year}: ${fmtCompact(peak.drawn)} drawn from ${fmtCompact(peak.base)} invested. ` +
        `${high.length === 1 ? "One year runs" : `${high.length} years run`} above ` +
        `${fmtPercent(WITHDRAWAL_WATCH, 0)}, the rate a portfolio can usually sustain for ` +
        "thirty years. Sustained, it is the common reason a retirement portfolio runs down.",
    });
  }

  // Idle cash: a bank account that keeps growing past a couple of years of
  // spending, year after year.
  const spending = new Map(cashFlows.map((f) => [f.year, f.expenses]));
  let idle: { label: string; from: number; to: number; value: number; years: number } | undefined;
  for (const series of ofFlavor("Bank")) {
    let start = -1;
    for (let i = 1; i <= years.length; i++) {
      const value = series.values[i];
      const spent = spending.get(years[i]) ?? 0;
      const growing =
        i < years.length &&
        value > (series.values[i - 1] ?? 0) + 1 &&
        spent > 0 &&
        value > IDLE_CASH_YEARS * spent;
      if (growing) {
        if (start < 0) start = i;
        continue;
      }
      const end = i - 1;
      if (start >= 0 && end - start + 1 >= IDLE_CASH_RUN) {
        const peak = series.values[end];
        if (!idle || peak > idle.value) {
          idle = {
            label: series.label,
            from: years[start],
            to: years[end],
            value: peak,
            years: peak / (spending.get(years[end]) ?? 1),
          };
        }
      }
      start = -1;
    }
  }
  if (idle) {
    checks.push({
      code: "idle-cash",
      title: `${idle.label} keeps piling up cash`,
      detail:
        `It grows every year from ${idle.from} to ${idle.to}, reaching ${fmtCompact(idle.value)}: ` +
        `${idle.years.toFixed(1)} years of spending. Cash earns less than the portfolio; ` +
        "a sweep rule could invest what is beyond the buffer you want.",
    });
  }
  return checks;
}
