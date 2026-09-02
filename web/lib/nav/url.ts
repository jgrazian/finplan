/**
 * The address bar as application state.
 *
 * Everything the app opens onto — which scenario, which tab, which sub-tab,
 * which row — is in the URL, so a refresh, a bookmark or a pasted link comes
 * back to the same place. The tab is the path; everything under it is query:
 *
 *     /plan?scenario=3&sel=Retire+at+65
 *
 * Values are written in each screen's own vocabulary — an account is its
 * display name, not its row id — so the URL stays readable and survives a
 * database that renumbers. Nothing here validates that a name still exists:
 * the screens already fall back to their first row for a stale pick, and that
 * is the same behaviour a hand-edited URL should get.
 */

export type TabId = "portfolio" | "plan" | "results" | "analysis" | "account";

/** Every path the app answers on, in the order the route enumerates them. */
export const TAB_IDS: ReadonlyArray<TabId> = [
  "portfolio",
  "plan",
  "results",
  "analysis",
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
  account: "profile",
};

export interface NavState {
  /** Database id of the open scenario; undefined lets the list pick one. */
  scenario?: number;
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
  // `Number(null)` and `Number("")` are both 0, which the positive test drops
  // along with `?scenario=abc`.
  const scenario = Number(query.get("scenario"));
  const fallback = DEFAULT_SECTION[tab];
  return {
    scenario: Number.isSafeInteger(scenario) && scenario > 0 ? scenario : undefined,
    tab,
    section: fallback == null ? undefined : (query.get("sec") ?? fallback),
    selection: query.get("sel") ?? undefined,
  };
}

/**
 * The other direction, defaults omitted: the Results tab of scenario 3 is
 * `/results?scenario=3`, not `/results?scenario=3&sec=&sel=`. The tab is
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
