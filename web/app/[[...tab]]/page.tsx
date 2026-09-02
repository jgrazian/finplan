import { TAB_IDS } from "@/lib/nav";
import { App } from "../App";

/**
 * Every screen is a path — `/portfolio`, `/plan` — over one client
 * application. The segment is not read here: the app takes the tab off
 * `location` along with the rest of its URL state, and moves between tabs with
 * `history` rather than a router push, so the loaded scenario survives the
 * move. This route exists to make those paths real ones, so a refresh or a
 * pasted link is served rather than 404ed.
 */
export const dynamicParams = false;

export function generateStaticParams() {
  // `[]` is the bare `/`, which the app reads as its default tab.
  return [{ tab: [] }, ...TAB_IDS.map((tab) => ({ tab: [tab] }))];
}

export default function Page() {
  return <App />;
}
