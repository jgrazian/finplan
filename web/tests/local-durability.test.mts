import assert from "node:assert/strict";
import { test } from "node:test";
import {
  PERSIST_KEY,
  backupDue,
  isInstalled,
  isSafari,
  recordedPersist,
  requestPersistOnce,
  showSafariNudge,
} from "../lib/local/durability.ts";
import type { LocalPlanMeta } from "../lib/local/runtime.ts";

const DAY = 24 * 60 * 60 * 1000;
const NOW = Date.parse("2026-10-03T12:00:00Z");
const ago = (days: number) => new Date(NOW - days * DAY).toISOString();

const meta = (over: Partial<LocalPlanMeta>): LocalPlanMeta => ({
  id: 1,
  lastExportedAt: null,
  editsSinceExport: 5,
  updatedAt: ago(0),
  ...over,
});

test("backup is due after five edits and more than 14 days since the last export", () => {
  assert.equal(
    backupDue({ meta: meta({ lastExportedAt: ago(15) }), createdAt: ago(90), now: NOW }),
    true,
  );
});

test("not due with fewer than five edits, however old the export", () => {
  assert.equal(
    backupDue({
      meta: meta({ lastExportedAt: ago(100), editsSinceExport: 4 }),
      createdAt: ago(200),
      now: NOW,
    }),
    false,
  );
});

test("not due within 14 days of the export", () => {
  assert.equal(
    backupDue({ meta: meta({ lastExportedAt: ago(14) }), createdAt: ago(90), now: NOW }),
    false,
  );
});

test("a plan never exported counts from its creation", () => {
  assert.equal(backupDue({ meta: meta({}), createdAt: ago(20), now: NOW }), true);
  assert.equal(backupDue({ meta: meta({}), createdAt: ago(3), now: NOW }), false);
});

test("row timestamps in the plain UTC form are read as UTC", () => {
  assert.equal(
    backupDue({ meta: meta({}), createdAt: "2026-09-01 08:00:00", now: NOW }),
    true,
  );
});

test("nothing known, nothing due", () => {
  assert.equal(backupDue({ meta: undefined, createdAt: ago(90), now: NOW }), false);
  assert.equal(backupDue({ meta: meta({}), createdAt: undefined, now: NOW }), false);
});

const SAFARI_MAC =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15";
const CHROME_MAC =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const EDGE =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0";
const FIREFOX = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:127.0) Gecko/20100101 Firefox/127.0";
const ANDROID =
  "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
const IPHONE_SAFARI =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";
const IPHONE_CHROME =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/126.0.0.0 Mobile/15E148 Safari/604.1";

test("Safari is detected, and the browsers that say Safari too are not", () => {
  assert.equal(isSafari(SAFARI_MAC), true);
  assert.equal(isSafari(IPHONE_SAFARI), true);
  assert.equal(isSafari(CHROME_MAC), false);
  assert.equal(isSafari(EDGE), false);
  assert.equal(isSafari(FIREFOX), false);
  assert.equal(isSafari(ANDROID), false);
});

test("on iOS every browser is WebKit, so Chrome there counts", () => {
  assert.equal(isSafari(IPHONE_CHROME), true);
});

test("the nudge shows for Safari that is neither installed nor dismissed", () => {
  const base = { userAgent: SAFARI_MAC, installed: false, dismissed: false };
  assert.equal(showSafariNudge(base), true);
  assert.equal(showSafariNudge({ ...base, installed: true }), false);
  assert.equal(showSafariNudge({ ...base, dismissed: true }), false);
  assert.equal(showSafariNudge({ ...base, userAgent: CHROME_MAC }), false);
});

test("an installed app is recognised by display mode or the iOS flag", () => {
  assert.equal(isInstalled({ matchMedia: () => ({ matches: true }) }), true);
  assert.equal(isInstalled({ navigator: { standalone: true } }), true);
  assert.equal(isInstalled({ matchMedia: () => ({ matches: false }) }), false);
  assert.equal(isInstalled({}), false);
});

function memory(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    data,
  };
}

function runtimeAnswering(answer: boolean | undefined, calls: string[]) {
  return {
    storage: {
      persisted: async () => answer,
      requestPersist: async () => {
        calls.push("requestPersist");
        return answer;
      },
    },
  };
}

test("persistence is requested on the first saved edit only, and the answer remembered", async () => {
  const store = memory();
  const calls: string[] = [];
  const runtime = runtimeAnswering(false, calls);

  assert.equal(await requestPersistOnce(runtime, store), "denied");
  assert.equal(store.data.get(PERSIST_KEY), "denied");
  assert.equal(await requestPersistOnce(runtime, store), undefined);
  assert.deepEqual(calls, ["requestPersist"]);
  assert.equal(recordedPersist(store), "denied");
});

test("a granted request and an unsupported browser are both recorded", async () => {
  const granted = memory();
  assert.equal(await requestPersistOnce(runtimeAnswering(true, []), granted), "granted");
  const unsupported = memory();
  assert.equal(await requestPersistOnce(runtimeAnswering(undefined, []), unsupported), "unsupported");
});

test("no runtime, no request", async () => {
  const store = memory();
  assert.equal(await requestPersistOnce(undefined, store), undefined);
  assert.equal(store.data.size, 0);
});

test("blocked storage does not break the request", async () => {
  const blocked = {
    getItem: () => {
      throw new Error("blocked");
    },
    setItem: () => {
      throw new Error("blocked");
    },
  };
  assert.equal(await requestPersistOnce(runtimeAnswering(true, []), blocked), "granted");
});
