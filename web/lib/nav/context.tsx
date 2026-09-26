"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { DEFAULT_SECTION, type NavState, type TabId, parseNav, toHref, scenarioDestination } from "./url";

export interface Nav extends NavState {
  /** Opens a different scenario, dropping a row pick its ids no longer name. */
  setScenario: (slug: string) => void;
  openScenario: (slug: string, tab: TabId, selection?: string) => void;
  /** Records the scenario actually opened, without a history entry. */
  adoptScenario: (slug: string) => void;
  setTab: (tab: TabId) => void;
  setSection: (section: string) => void;
  setSelection: (selection: string | undefined) => void;
}

const NavContext = createContext<Nav | undefined>(undefined);

/**
 * Holds the URL that the screens navigate by.
 *
 * `history` rather than the Next router: the tabs are separate paths, but they
 * are one application holding one scenario's worth of loaded data, and a router
 * push would tear that down and build it back to land somewhere it never left.
 * Next patches `pushState` to fold an outside change into its own canonical URL
 * without refetching, which is exactly the move this wants.
 *
 * What earns a history entry is what reads as a navigation — a scenario, a tab,
 * a sub-tab. Picking a row replaces instead, so arrowing down a table does not
 * bury the tab you came from under thirty back presses.
 */
export function NavProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<NavState>(() =>
    typeof window === "undefined"
      ? parseNav("", "")
      : parseNav(window.location.pathname, window.location.search),
  );

  // Back and forward are navigations like any other, so they are read out of
  // the address bar rather than out of a second copy of the stack kept here.
  useEffect(() => {
    const onPop = () => setState(parseNav(window.location.pathname, window.location.search));
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  const go = useCallback((next: NavState, push: boolean) => {
    setState(next);
    const url = `${toHref(next)}${window.location.hash}`;
    if (push) window.history.pushState(null, "", url);
    else window.history.replaceState(null, "", url);
  }, []);

  const nav = useMemo<Nav>(
    () => ({
      ...state,
      openScenario: (scenario, tab, selection) => go({ ...scenarioDestination(scenario, tab), selection }, true),
      setScenario: (scenario) => go({ ...state, scenario, selection: undefined }, true),
      adoptScenario: (scenario) => {
        if (state.scenario === scenario) return;
        go({ ...state, scenario }, false);
      },
      // A tab change starts its section over: the sub-tab and the row belong to
      // the tab that was open, and mean nothing to the one arriving.
      setTab: (tab) => go({ scenario: state.scenario, tab, section: DEFAULT_SECTION[tab] }, true),
      setSection: (section) => go({ ...state, section, selection: undefined }, true),
      setSelection: (selection) => go({ ...state, selection }, false),
    }),
    [go, state],
  );

  return <NavContext.Provider value={nav}>{children}</NavContext.Provider>;
}

/** The current query state, and the moves that change it. */
export function useNav(): Nav {
  const nav = useContext(NavContext);
  if (!nav) throw new Error("useNav must be used inside a NavProvider");
  return nav;
}
