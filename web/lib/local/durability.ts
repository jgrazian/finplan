/**
 * Keeping a local plan from being lost (spec 19, "Durability"): the persistence
 * request, the backup reminder, the Safari warning, and the words for each.
 * Pure and storage-injected, so the rules are tested without a browser.
 */
import type { LocalPlanMeta, LocalRuntime } from "./runtime.ts";

/** Shown once, where plans live: on the new-plan screen and as the home badge's tooltip. */
export const PRIVACY_PROMISE =
  "Your plan stays in your browser. Nothing leaves it unless you move the plan to the cloud, offload a run, or use an AI feature.";

export const PERSIST_DENIED_NOTE =
  "This browser may clear local plans — back up or move to cloud.";

export const STORE_CLEARED_NOTICE =
  "Your browser cleared this site's data. Import a backup file to restore your plan.";

export const SAFARI_NUDGE =
  "Safari deletes a website's data after 7 days without a visit unless the site is installed. Use Add to Dock (Mac) or Add to Home Screen (iPhone, iPad) so your plans stay.";

/** Edits since the last export before a backup is worth suggesting. */
export const BACKUP_MIN_EDITS = 5;
/** Days since the last export (or since creation) before it is. */
export const BACKUP_AFTER_DAYS = 14;

const DAY_MS = 24 * 60 * 60 * 1000;

/** A plan row's timestamp: ISO, or the `YYYY-MM-DD HH:MM:SS` UTC the rows use. */
export function parseStamp(text: string | null | undefined): number | undefined {
  if (!text) return undefined;
  const iso = text.replace(" ", "T");
  const time = Date.parse(/(?:Z|[+-]\d\d:?\d\d)$/.test(iso) ? iso : `${iso}Z`);
  return Number.isNaN(time) ? undefined : time;
}

/**
 * Whether to show "Back up this plan": at least five saved edits since the
 * last export, and the last export (or, never having exported, the plan's
 * creation) more than 14 days ago.
 */
export function backupDue(input: {
  meta: LocalPlanMeta | undefined;
  /** The plan's `created_at`, for a plan never exported. */
  createdAt: string | undefined;
  now: number;
}): boolean {
  const { meta, createdAt, now } = input;
  if (!meta || meta.editsSinceExport < BACKUP_MIN_EDITS) return false;
  const since = parseStamp(meta.lastExportedAt) ?? parseStamp(createdAt);
  if (since === undefined) return false;
  return now - since > BACKUP_AFTER_DAYS * DAY_MS;
}

/**
 * Safari, desktop or iOS, which evicts script-written storage for a site not
 * visited in seven days unless it is installed. Chrome, Firefox, Edge and
 * Opera on desktop say "Safari" in their user agent too, so those are ruled
 * out by name; on iOS every browser is WebKit and Apple's rule applies to them
 * all, so Chrome and Firefox there (`CriOS`, `FxiOS`) count.
 */
export function isSafari(userAgent: string): boolean {
  const ios = /\b(iPhone|iPad|iPod)\b/.test(userAgent);
  if (ios) return true;
  if (/\b(Chrome|Chromium|Edg|OPR|Opera|Android|Firefox)\b/.test(userAgent)) return false;
  return /\bSafari\b/.test(userAgent);
}

/** Running as an installed app, where Safari's eviction rule no longer applies. */
export function isInstalled(env: {
  matchMedia?: (query: string) => { matches: boolean };
  navigator?: { standalone?: boolean };
}): boolean {
  if (env.navigator?.standalone === true) return true;
  try {
    return env.matchMedia?.("(display-mode: standalone)").matches === true;
  } catch {
    return false;
  }
}

/** The Safari nudge: Safari, not installed, not dismissed. */
export function showSafariNudge(input: {
  userAgent: string;
  installed: boolean;
  dismissed: boolean;
}): boolean {
  return isSafari(input.userAgent) && !input.installed && !input.dismissed;
}

/** The slice of `localStorage` these rules use. */
export interface KeyValue {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const PERSIST_KEY = "finplan.persist";
export const SAFARI_DISMISSED_KEY = "finplan.safariNudgeDismissed";

export type PersistAnswer = "granted" | "denied" | "unsupported";

/** What was recorded the last time persistence was asked for; undefined if it never was. */
export function recordedPersist(store: KeyValue | undefined): PersistAnswer | undefined {
  try {
    const value = store?.getItem(PERSIST_KEY);
    return value === "granted" || value === "denied" || value === "unsupported" ? value : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Asks the browser to keep this site's storage, once: on the first saved edit
 * of a local plan. The answer is remembered, so a later edit asks nothing, and
 * a "denied" is the plan header's note rather than a nag. Returns the answer
 * on, and only on, the call that asked.
 */
export async function requestPersistOnce(
  runtime: Pick<LocalRuntime, "storage"> | undefined,
  store: KeyValue | undefined,
): Promise<PersistAnswer | undefined> {
  if (!runtime || recordedPersist(store) !== undefined) return undefined;
  let answer: PersistAnswer;
  try {
    const granted = await runtime.storage.requestPersist();
    answer = granted === undefined ? "unsupported" : granted ? "granted" : "denied";
  } catch {
    answer = "unsupported";
  }
  try {
    store?.setItem(PERSIST_KEY, answer);
  } catch {
    // Storage blocked: the request is simply asked again next time.
  }
  return answer;
}
