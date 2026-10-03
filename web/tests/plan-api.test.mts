import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { afterEach, test } from "node:test";
import { LOCAL_MODE_STORAGE_KEY, loadLocalModePolicy, localModeEnabled, setServerLocalMode } from "../lib/local/flag.ts";
import { LocalUnavailableError, hasLocalBackend, localApi, setLocalBackend } from "../lib/api/local.ts";
import type { PlanApi } from "../lib/api/plan.ts";
import { createPlanRouter, homeOfTarget } from "../lib/api/route.ts";
import { MOVE_TO_CLOUD_FOR_AI, planCapabilities } from "../lib/nav/capabilities.ts";

/** A backend that answers only what a test asks of it; the cast is the point of a stand-in. */
function fakeBackend(calls: unknown[][] = []): PlanApi {
  const record =
    (name: string, answer: unknown = undefined) =>
    async (...args: unknown[]) => {
      calls.push([name, ...args]);
      return answer;
    };
  return {
    scenarios: {
      get: record("scenarios.get", { id: 4, slug: "l4" }),
      list: record("scenarios.list", [{ id: 4, slug: "l4" }]),
      update: record("scenarios.update"),
    },
    accounts: { positions: record("accounts.positions", []) },
    runs: { ledger: record("runs.ledger", { total: 0 }) },
    historyPresets: record("historyPresets", []),
  } as unknown as PlanApi;
}

afterEach(() => setLocalBackend(undefined));

test("a local call with no backend is refused in words, as a rejection", async () => {
  assert.equal(hasLocalBackend(), false);
  const calls = [
    () => localApi.scenarios.list(),
    () => localApi.accounts.positions(1, 2),
    () => localApi.runs.results(1),
    () => localApi.archives.exportAll(),
    () => localApi.historyPresets(),
  ];
  for (const call of calls) {
    await assert.rejects(call, (error: unknown) => {
      assert.ok(error instanceof LocalUnavailableError);
      assert.match(error.message, /Local plans are not available/);
      // The fields code reads off an ApiError.
      assert.equal(error.status, 503);
      assert.equal(error.code, "local_unavailable");
      assert.equal(error.isUnauthorized, false);
      return true;
    });
  }
});

test("local calls forward to the registered backend, arguments and answers intact", async () => {
  const calls: unknown[][] = [];
  setLocalBackend(fakeBackend(calls));
  assert.equal(hasLocalBackend(), true);

  assert.deepEqual(await localApi.scenarios.get(4), { id: 4, slug: "l4" });
  await localApi.scenarios.update(4, { name: "Retire early" });
  await localApi.runs.ledger(9, { year: 2031 });
  assert.deepEqual(await localApi.historyPresets(), []);
  assert.deepEqual(calls, [
    ["scenarios.get", 4],
    ["scenarios.update", 4, { name: "Retire early" }],
    ["runs.ledger", 9, { year: 2031 }],
    ["historyPresets"],
  ]);
});

test("the backend is looked up per call, so one registered later or replaced is the one asked", async () => {
  const first: unknown[][] = [];
  const second: unknown[][] = [];
  const scenarios = localApi.scenarios;

  setLocalBackend(fakeBackend(first));
  await scenarios.list();
  setLocalBackend(fakeBackend(second));
  await scenarios.list();
  assert.deepEqual([first.length, second.length], [1, 1]);

  setLocalBackend(undefined);
  await assert.rejects(() => scenarios.list(), LocalUnavailableError);
});

test("a method the backend does not have is refused by name, not as a TypeError", async () => {
  setLocalBackend(fakeBackend());
  await assert.rejects(
    () => localApi.whatIf.get(1),
    (error: unknown) => error instanceof LocalUnavailableError && /"get"/.test(error.message),
  );
});

test("a group is not mistaken for a promise by await", async () => {
  const group = await Promise.resolve(localApi.scenarios);
  assert.equal(group, localApi.scenarios);
});

test("localApi never reaches fetch, with a backend or without one", async () => {
  const original = globalThis.fetch;
  let reached = 0;
  globalThis.fetch = (() => {
    reached += 1;
    throw new Error("a local call must not touch the network");
  }) as typeof fetch;
  try {
    await assert.rejects(() => localApi.scenarios.list(), LocalUnavailableError);
    setLocalBackend(fakeBackend());
    await localApi.scenarios.list();
    await localApi.scenarios.get(4);
    await localApi.accounts.positions(4, 1);
    await localApi.runs.ledger(1);
    await localApi.historyPresets();
    assert.equal(reached, 0);
  } finally {
    globalThis.fetch = original;
  }
});

test("the local module imports types and nothing else, so it cannot reach http or fetch", () => {
  const source = readFileSync(new URL("../lib/api/local.ts", import.meta.url), "utf8");
  const code = source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");
  const imports = code.match(/^\s*import\b.*$/gm) ?? [];
  assert.ok(imports.length > 0);
  for (const line of imports) assert.match(line, /^\s*import type\b/, line);
  assert.doesNotMatch(code, /\bfetch\s*\(/);
  assert.doesNotMatch(code, /\bhttp\b|remoteApi/);
});

test("the router sends a ref, or a home, to that home's implementation", () => {
  const local = fakeBackend();
  const cloud = fakeBackend();
  const planApiFor = createPlanRouter({ local, cloud });

  assert.equal(planApiFor("l12"), local);
  assert.equal(planApiFor("local"), local);
  assert.equal(planApiFor("s8a4f21b7c903"), cloud);
  assert.equal(planApiFor("cloud"), cloud);
  // Old numeric bookmarks, and no plan open yet, are the cloud's.
  assert.equal(planApiFor("7"), cloud);
  assert.equal(planApiFor(undefined), cloud);
  assert.equal(planApiFor(), cloud);
  assert.equal(homeOfTarget("l1"), "local");
  assert.equal(homeOfTarget("local"), "local");
});

test("a local plan has no AI and says how to get it; a cloud plan has it and nothing to offload", () => {
  const signedIn = { account: true };
  const guest = { account: false };

  const local = planCapabilities("l3", signedIn);
  assert.deepEqual(local, {
    home: "local",
    ai: false,
    offload: true,
    goalSeekQuota: false,
    cloudOnlyReason: "Move to cloud to use AI",
  });
  assert.equal(local.cloudOnlyReason, MOVE_TO_CLOUD_FOR_AI);

  // Offload costs compute, so it needs an account: a guest is not one.
  assert.equal(planCapabilities("l3", guest).offload, false);
  assert.equal(planCapabilities("l3", guest).ai, false);

  const cloud = planCapabilities("s8a4f21b7c903", signedIn);
  assert.deepEqual(cloud, { home: "cloud", ai: true, offload: false, goalSeekQuota: true });
  assert.equal("cloudOnlyReason" in cloud, false);
  // A home can be named before there is a plan, for the new-plan screen.
  assert.equal(planCapabilities("local", signedIn).ai, false);
  assert.equal(planCapabilities(undefined, signedIn).home, "cloud");
});

/** Runs `body` with the local-mode inputs set, putting the environment back after. */
async function withFlag(
  { env, storage }: { env?: string; storage?: string | (() => never) },
  body: () => void | Promise<void>,
) {
  const key = "NEXT_PUBLIC_FINPLAN_LOCAL_MODE";
  const savedEnv = process.env[key];
  const savedStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  if (env === undefined) delete process.env[key];
  else process.env[key] = env;
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (name: string) => {
        if (typeof storage === "function") return storage();
        return name === LOCAL_MODE_STORAGE_KEY && storage !== undefined ? storage : null;
      },
    },
  });
  setServerLocalMode(undefined);
  try {
    await body();
  } finally {
    if (savedEnv === undefined) delete process.env[key];
    else process.env[key] = savedEnv;
    if (savedStorage) Object.defineProperty(globalThis, "localStorage", savedStorage);
    else delete (globalThis as { localStorage?: unknown }).localStorage;
    setServerLocalMode(undefined);
  }
}

test("local mode is off unless the build or the browser turns it on", async () => {
  await withFlag({}, () => assert.equal(localModeEnabled(), false));
  await withFlag({ env: "0" }, () => assert.equal(localModeEnabled(), false));
  await withFlag({ storage: "0" }, () => assert.equal(localModeEnabled(), false));
  await withFlag({ env: "1" }, () => assert.equal(localModeEnabled(), true));
  await withFlag({ env: "true" }, () => assert.equal(localModeEnabled(), true));
  await withFlag({ storage: "1" }, () => assert.equal(localModeEnabled(), true));
});

test("blocked storage reads as off rather than throwing", async () => {
  const blocked = () => {
    throw new Error("SecurityError");
  };
  await withFlag({ storage: blocked }, () => assert.equal(localModeEnabled(), false));
  // The build-time switch does not need storage at all.
  await withFlag({ env: "1", storage: blocked }, () => assert.equal(localModeEnabled(), true));
});

test("the server can turn local mode off, and silence on its part means on", async () => {
  await withFlag({ env: "1" }, async () => {
    assert.equal(localModeEnabled(), true);
    setServerLocalMode(false);
    assert.equal(localModeEnabled(), false);
    setServerLocalMode(true);
    assert.equal(localModeEnabled(), true);
  });
  // The server's word is not enough to turn it on.
  await withFlag({}, () => {
    setServerLocalMode(true);
    assert.equal(localModeEnabled(), false);
  });
});

test("/health is read for local_mode, and an older server's plain answer leaves local mode on", async () => {
  const answer = (body: string, ok = true) => async () => new Response(body, { status: ok ? 200 : 503 });
  await withFlag({ env: "1" }, async () => {
    assert.equal(await loadLocalModePolicy(answer('{"status":"ok","local_mode":false}')), false);
    assert.equal(localModeEnabled(), false);
  });
  await withFlag({ env: "1" }, async () => {
    assert.equal(await loadLocalModePolicy(answer('{"status":"ok","local_mode":true}')), true);
  });
  // Missing field, plain text, a failed request, a thrown one: all "not said", so on.
  for (const fetchHealth of [
    answer('{"status":"ok"}'),
    answer("ok"),
    answer("", false),
    async () => {
      throw new Error("offline");
    },
  ]) {
    await withFlag({ env: "1" }, async () => {
      assert.equal(await loadLocalModePolicy(fetchHealth), true);
    });
  }
});

test("with the flag off the server is never asked, so the app makes no request it did not before", async () => {
  let asked = 0;
  await withFlag({}, async () => {
    const allowed = await loadLocalModePolicy(async () => {
      asked += 1;
      return new Response('{"local_mode":true}');
    });
    assert.equal(allowed, false);
  });
  assert.equal(asked, 0);
});
