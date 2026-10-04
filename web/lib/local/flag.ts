/**
 * Whether plans may live in this browser (spec 19).
 *
 * On by default. Two keys can turn it off, and either off is off. The
 * visitor's side: a `localStorage` switch for one browser, then a build-time
 * variable for a deployment. The server's side is `/health`, where an operator
 * who wants every plan on their server reports `local_mode: false`. Off means
 * the app is exactly what it was before local plans existed.
 *
 * Plain imports only, so the Node test runner can load it.
 */

/** The `localStorage` key that turns local mode on (`"1"`) or off (`"0"`) for one browser. */
export const LOCAL_MODE_STORAGE_KEY = "finplan.localMode";

const ON = new Set(["1", "true"]);
const OFF = new Set(["0", "false"]);

function clientFlag(): boolean {
  // One browser's own choice first, so a tester can flip it without a rebuild.
  try {
    const stored = globalThis.localStorage?.getItem(LOCAL_MODE_STORAGE_KEY);
    if (stored != null && ON.has(stored)) return true;
    if (stored != null && OFF.has(stored)) return false;
  } catch {
    // Storage can throw outright: blocked site data, a sandboxed frame.
  }
  try {
    // Next inlines `NEXT_PUBLIC_*` only when it is spelled out like this, and
    // where nothing inlined it and there is no `process`, the lookup throws.
    const built = process.env.NEXT_PUBLIC_FINPLAN_LOCAL_MODE;
    if (built !== undefined && OFF.has(built)) return false;
  } catch {
    // No build-time value to read: the default stands.
  }
  return true;
}

/** What `/health` said about local mode; undefined until asked, or when it did not say. */
let serverAllows: boolean | undefined;

/** Records the server's answer. `undefined` forgets it, which is how an absent field reads. */
export function setServerLocalMode(allowed: boolean | undefined): void {
  serverAllows = allowed;
}

/**
 * Local mode is on: the client has not turned it off and the server has not said no. A
 * server that has not answered yet, or whose `/health` has no `local_mode`
 * (an older one), counts as yes: the field exists to turn the feature off.
 */
export function localModeEnabled(): boolean {
  return clientFlag() && serverAllows !== false;
}

/**
 * Asks `/health` whether the server allows local mode, once, and remembers.
 *
 * Skipped entirely when the client has turned it off, so a deployment that
 * opted out makes no request it did not make before. `/health` carries no plan
 * data, which is why it may be asked in local mode. Any failure is the same as
 * silence: the browser may still be offline, and local plans work offline.
 */
export async function loadLocalModePolicy(
  fetchHealth: () => Promise<Response> = () => fetch("/api/health", { cache: "no-store" }),
): Promise<boolean> {
  if (!clientFlag()) return false;
  try {
    const response = await fetchHealth();
    if (response.ok) {
      const reported = localModeOf(await response.text());
      if (reported !== undefined) serverAllows = reported;
    }
  } catch {
    // Unreached or unparseable: leave what is known as it was.
  }
  return localModeEnabled();
}

/** `local_mode` out of a `/health` body, which is JSON on a server that reports it and plain text before. */
function localModeOf(body: string): boolean | undefined {
  try {
    const parsed: unknown = JSON.parse(body);
    if (parsed !== null && typeof parsed === "object" && "local_mode" in parsed) {
      const value = (parsed as { local_mode: unknown }).local_mode;
      return typeof value === "boolean" ? value : undefined;
    }
  } catch {
    // Not JSON: an older server's "ok".
  }
  return undefined;
}
