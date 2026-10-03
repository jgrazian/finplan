/**
 * The address bar as application state.
 *
 * Everything the app opens onto — which scenario, which tab, which sub-tab,
 * which row — is in the URL, so a refresh, a bookmark or a pasted link comes
 * back to the same place. The tab is the path; everything under it is query:
 *
 *     /plan?scenario=s8a4f21b7c903&sel=Retire+at+65
 *
 * A plan kept in this browser is `scenario=l12` rather than a slug; see
 * `homeOf`.
 *
 * Values are written in each screen's own vocabulary — an account is its
 * display name, not its row id — so the URL stays readable and survives a
 * database that renumbers. Nothing here validates that a name still exists:
 * the screens already fall back to their first row for a stale pick, and that
 * is the same behaviour a hand-edited URL should get.
 */

export type TabId = "portfolio" | "plan" | "results" | "analysis" | "review" | "account";

/** Every path the app answers on, in the order the route enumerates them. */
export const TAB_IDS: ReadonlyArray<TabId> = [
  "portfolio",
  "plan",
  "results",
  "analysis",
  "review",
  "account",
];

/** Where a bare `/` lands. Written out as `/results` from then on. */
export const DEFAULT_TAB: TabId = "results";

/**
 * The sub-tab each tab opens on, and the record of which tabs have a second
 * level at all: a tab absent from here carries no section, so `sec` is dropped
 * rather than kept as a value nothing reads. A section equal to its tab's
 * default is left out of the URL too.
 */
export const DEFAULT_SECTION: Partial<Record<TabId, string>> = {
  portfolio: "accounts",
  review: "notes",
  analysis: "what-if",
  account: "profile",
};

export interface NavState {
  /** Stable slug of the open scenario; undefined lets the list pick one. */
  scenario?: string;
  tab: TabId;
  /** Sub-tab within the tab, defaulted per `DEFAULT_SECTION`. */
  section?: string;
  /** Selected row within the section, undefined meaning "the first one". */
  selection?: string;
}

/** Reads a URL — `/plan?sel=Retire` — back into nav state. */
export function parseNav(pathname: string, search: string): NavState {
  const query = new URLSearchParams(search);
  // Only the first segment is the tab; `/` is the default one, and anything
  // deeper cannot arrive because the route enumerates the paths it serves.
  const tab = TAB_IDS.find((id) => id === pathname.split("/")[1]) ?? DEFAULT_TAB;
  const scenario = query.get("scenario");
  const fallback = DEFAULT_SECTION[tab];
  return {
    scenario: scenario != null && /^[a-zA-Z0-9]+$/.test(scenario) ? scenario : undefined,
    tab,
    section: fallback == null ? undefined : (query.get("sec") ?? fallback),
    selection: query.get("sel") ?? undefined,
  };
}

/**
 * The other direction, defaults omitted: the Results tab of a scenario is
 * `/results?scenario=s8a4f21b7c903`, not `/results?scenario=s8a4f21b7c903&sec=&sel=`. The tab is
 * always named, including the default one, so every screen has one address
 * rather than two.
 */
export function toHref(state: NavState): string {
  const query = new URLSearchParams();
  if (state.scenario != null) query.set("scenario", String(state.scenario));
  if (state.section != null && state.section !== DEFAULT_SECTION[state.tab]) {
    query.set("sec", state.section);
  }
  if (state.selection != null && state.selection !== "") {
    query.set("sel", state.selection);
  }
  const text = query.toString();
  return text === "" ? `/${state.tab}` : `/${state.tab}?${text}`;
}

/** One navigation for scenario creation: no intermediate old-scenario tab move. */
export function scenarioDestination(scenario: string, tab: TabId): NavState {
  return { scenario, tab, section: DEFAULT_SECTION[tab] };
}

/**
 * Where a plan lives. Each plan has exactly one home (spec 19): the browser
 * (`local`) or the server (`cloud`).
 */
export type PlanHome = "local" | "cloud";

/**
 * A local plan's ref is `l` and its numeric id; a cloud slug is `s` and hex, so
 * the first character alone says which home answers. Nothing else starts with
 * `l`, which is what lets `?scenario=` carry both without a second parameter.
 */
const LOCAL_REF = /^l(\d+)$/;

/** The home a plan ref names. A legacy numeric bookmark and no ref at all are the cloud's. */
export function homeOf(ref: string | undefined): PlanHome {
  return ref != null && LOCAL_REF.test(ref) ? "local" : "cloud";
}

/** The local row id inside a local ref (`l12` is 12); undefined for anything else. */
export function localPlanId(ref: string | undefined): number | undefined {
  const match = ref == null ? null : LOCAL_REF.exec(ref);
  return match ? Number(match[1]) : undefined;
}

/**
 * The ref for a plan in `home`. A local plan is its id, prefixed once (so a ref
 * passes through unchanged); a cloud plan is its slug.
 */
export function planRef(home: PlanHome, idOrSlug: number | string): string {
  if (home === "cloud") return String(idOrSlug);
  const text = String(idOrSlug);
  return LOCAL_REF.test(text) ? text : `l${text}`;
}

/**
 * The open plan out of a list that may hold both homes.
 *
 * Local plans are listed with `slug = "l<id>"`, and cloud rows and local rows
 * number their ids independently, so a bare id is ambiguous across homes. The
 * slug is matched first; after that an `l<id>` ref finds a local row by id and
 * a bare number — an old bookmark from before slugs — finds a cloud row only,
 * never a local one that happens to share the number.
 */
export function resolveScenario<T extends { id: number; slug: string }>(
  scenarios: readonly T[],
  reference: string | undefined,
): T | undefined {
  const local = localPlanId(reference);
  return scenarios.find((scenario) => scenario.slug === reference)
    ?? scenarios.find((scenario) =>
      local != null
        ? homeOf(scenario.slug) === "local" && scenario.id === local
        : homeOf(scenario.slug) === "cloud" && String(scenario.id) === reference,
    )
    ?? scenarios[0];
}
