import assert from "node:assert/strict";
import { test } from "node:test";
import type { PlanArchive } from "../lib/api/generated/PlanArchive.ts";
import type { PlanApi } from "../lib/api/plan.ts";
import {
  archiveFileName,
  copyToDevice,
  deleteCloudCopy,
  exportLocal,
  importIntoDevice,
  migrateGuest,
  moveToCloud,
  parseArchive,
  previewLocalArchive,
} from "../lib/local/homes.ts";

const ARCHIVE: PlanArchive = { format: "finplan.plan-archive", version: 3, plans: [{ name: "P" }] };

/** A PlanApi that records its calls in one shared log, so cross-home order can be asserted. */
function fakeHome(name: string, log: string[], overrides: Record<string, unknown> = {}): PlanApi {
  const step =
    (what: string, answer: unknown = undefined) =>
    async (...args: unknown[]) => {
      log.push(`${name}.${what}`);
      const custom = overrides[what];
      if (typeof custom === "function") return custom(...args);
      return answer;
    };
  return {
    scenarios: {
      archive: step("scenarios.archive", ARCHIVE),
      remove: step("scenarios.remove"),
      get: step("scenarios.get", { id: 1, slug: "x" }),
    },
    archives: {
      import: step("archives.import", { scenario_ids: [7] }),
      preview: step("archives.preview", { names: ["P"], accounts: 1, events: 2, assumptions: 0 }),
      exportAll: step("archives.exportAll", ARCHIVE),
    },
  } as unknown as PlanApi;
}

test("move to cloud imports on the server before it deletes the local plan", async () => {
  const log: string[] = [];
  const result = await moveToCloud(
    { local: fakeHome("local", log), remote: fakeHome("remote", log) },
    3,
    "key-1",
  );
  assert.deepEqual(log, ["local.scenarios.archive", "remote.archives.import", "local.scenarios.remove"]);
  assert.deepEqual(result, { cloudId: 7, localDeleted: true });
});

test("move to cloud sends the archive unprefixed with the caller's idempotency key", async () => {
  const sent: unknown[] = [];
  const remote = fakeHome("remote", [], {
    "archives.import": (body: unknown) => {
      sent.push(body);
      return { scenario_ids: [9] };
    },
  });
  await moveToCloud({ local: fakeHome("local", []), remote }, 3, "key-2");
  assert.deepEqual(sent, [{ archive: ARCHIVE, name_prefix: "", request_id: "key-2", from_guest: false }]);
});

test("a refused import (plan limit) throws as returned and deletes nothing", async () => {
  const log: string[] = [];
  const remote = fakeHome("remote", log, {
    "archives.import": () => {
      throw new Error("You have reached your plan limit.");
    },
  });
  await assert.rejects(
    moveToCloud({ local: fakeHome("local", log), remote }, 3, "k"),
    /plan limit/,
  );
  assert.ok(!log.includes("local.scenarios.remove"), "the local plan must survive a failed move");
});

test("an import that created nothing is a failure, not a delete", async () => {
  const log: string[] = [];
  const remote = fakeHome("remote", log, { "archives.import": () => ({ scenario_ids: [] }) });
  await assert.rejects(moveToCloud({ local: fakeHome("local", log), remote }, 3, "k"));
  assert.ok(!log.includes("local.scenarios.remove"));
});

test("a failed local delete after a good import is reported, not thrown", async () => {
  const local = fakeHome("local", [], {
    "scenarios.remove": () => {
      throw new Error("store busy");
    },
  });
  const result = await moveToCloud({ local, remote: fakeHome("remote", []) }, 3, "k");
  assert.equal(result.cloudId, 7);
  assert.equal(result.localDeleted, false);
  assert.equal(result.deleteError, "store busy");
});

test("download to device copies first and deletes nothing", async () => {
  const log: string[] = [];
  const { localId } = await copyToDevice(
    { local: fakeHome("local", log), remote: fakeHome("remote", log) },
    5,
    "k",
  );
  assert.equal(localId, 7);
  assert.deepEqual(log, ["remote.scenarios.archive", "local.archives.import"]);
});

test("a failed local import leaves the cloud plan alone", async () => {
  const log: string[] = [];
  const local = fakeHome("local", log, {
    "archives.import": () => {
      throw new Error("storage full");
    },
  });
  await assert.rejects(copyToDevice({ local, remote: fakeHome("remote", log) }, 5, "k"), /storage full/);
  assert.ok(!log.includes("remote.scenarios.remove"));
});

test("the cloud copy is deleted only when asked", async () => {
  const log: string[] = [];
  await deleteCloudCopy({ local: fakeHome("local", log), remote: fakeHome("remote", log) }, 5);
  assert.deepEqual(log, ["remote.scenarios.remove"]);
});

test("export names the file for the plan and the day, and marks the plan exported after saving", async () => {
  const log: string[] = [];
  const saved: Array<[string, unknown]> = [];
  const runtime = {
    markExported: async (ids: number[]) => void log.push(`mark:${ids.join(",")}`),
  };
  await exportLocal(
    { local: fakeHome("local", log) },
    runtime,
    { kind: "one", id: 4, name: "Retire @ 65!" },
    (filename, archive) => {
      log.push("save");
      saved.push([filename, archive]);
    },
    new Date("2026-10-03T10:00:00Z"),
  );
  assert.deepEqual(saved, [["finplan-retire-65-2026-10-03.json", ARCHIVE]]);
  assert.deepEqual(log, ["local.scenarios.archive", "save", "mark:4"]);
});

test("a download that fails is not marked as a backup", async () => {
  const marked: number[][] = [];
  await assert.rejects(
    exportLocal(
      { local: fakeHome("local", []) },
      { markExported: async (ids) => void marked.push(ids) },
      { kind: "one", id: 4, name: "P" },
      () => {
        throw new Error("blocked");
      },
    ),
  );
  assert.deepEqual(marked, []);
});

test("export all uses the whole-home archive and marks every plan", async () => {
  const marked: number[][] = [];
  const saved: string[] = [];
  await exportLocal(
    { local: fakeHome("local", []) },
    { markExported: async (ids) => void marked.push(ids) },
    { kind: "all", ids: [1, 2, 3] },
    (filename) => void saved.push(filename),
    new Date("2026-10-03T10:00:00Z"),
  );
  assert.deepEqual(saved, ["finplan-plans-2026-10-03.json"]);
  assert.deepEqual(marked, [[1, 2, 3]]);
});

test("file names fall back when the plan name has nothing to keep", () => {
  assert.equal(archiveFileName("???", new Date("2026-01-02T00:00:00Z")), "finplan-plan-2026-01-02.json");
});

test("a file that is not an archive is refused in words", () => {
  assert.throws(() => parseArchive("not json"), /not valid JSON/);
  assert.throws(() => parseArchive('{"plans": []}'), /not a FinPlan archive/);
  assert.throws(() => parseArchive("null"), /not a FinPlan archive/);
  assert.deepEqual(parseArchive(JSON.stringify(ARCHIVE)), ARCHIVE);
});

test("preview falls back to nothing when the store has none, and still throws for a bad file", async () => {
  const unsupported = {
    archives: {
      preview: async () => {
        throw Object.assign(new Error("no"), { code: "local_unavailable" });
      },
    },
  } as unknown as PlanApi;
  assert.equal(await previewLocalArchive({ local: unsupported }, ARCHIVE), undefined);

  const invalid = {
    archives: {
      preview: async () => {
        throw Object.assign(new Error("bad plan"), { code: "invalid" });
      },
    },
  } as unknown as PlanApi;
  await assert.rejects(previewLocalArchive({ local: invalid }, ARCHIVE), /bad plan/);
});

test("import into the device returns the new ids", async () => {
  assert.deepEqual(await importIntoDevice({ local: fakeHome("local", []) }, ARCHIVE, "k"), [7]);
});

// --- spec 17 guests ---------------------------------------------------------

function migration(log: string[], overrides: { local?: Record<string, unknown>; remote?: Record<string, unknown>; ready?: boolean; logout?: () => Promise<void> } = {}) {
  return {
    local: fakeHome("local", log, overrides.local),
    remote: fakeHome("remote", log, overrides.remote),
    logout: overrides.logout ?? (async () => void log.push("logout")),
    localReady: async () => overrides.ready ?? true,
    requestId: "migration-key",
  };
}

test("a guest's plans are exported, imported locally, and only then is the guest logged out", async () => {
  const log: string[] = [];
  const outcome = await migrateGuest(migration(log));
  assert.deepEqual(outcome, { status: "migrated", plans: 1 });
  assert.deepEqual(log, ["remote.archives.exportAll", "local.archives.import", "logout"]);
});

test("the import marks the plans as a guest's, unprefixed", async () => {
  const sent: unknown[] = [];
  await migrateGuest(
    migration([], {
      local: {
        "archives.import": (body: unknown) => {
          sent.push(body);
          return { scenario_ids: [1] };
        },
      },
    }),
  );
  assert.deepEqual(sent, [
    { archive: ARCHIVE, name_prefix: "", request_id: "migration-key", from_guest: true },
  ]);
});

test("no logout when the local import fails: the guest is left alone", async () => {
  const log: string[] = [];
  const outcome = await migrateGuest(
    migration(log, {
      local: {
        "archives.import": () => {
          throw new Error("quota exceeded");
        },
      },
    }),
  );
  assert.deepEqual(outcome, { status: "failed", error: "quota exceeded" });
  assert.ok(!log.includes("logout"));
});

test("no logout when the local store never comes up", async () => {
  const log: string[] = [];
  const outcome = await migrateGuest(migration(log, { ready: false }));
  assert.equal(outcome.status, "failed");
  assert.deepEqual(log, ["remote.archives.exportAll"]);
});

test("no logout when the export itself fails", async () => {
  const log: string[] = [];
  const outcome = await migrateGuest(
    migration(log, {
      remote: {
        "archives.exportAll": () => {
          throw new Error("offline");
        },
      },
    }),
  );
  assert.deepEqual(outcome, { status: "failed", error: "offline" });
  assert.ok(!log.includes("logout"));
});

test("an import that saved nothing is a failure, so the guest stays", async () => {
  const log: string[] = [];
  const outcome = await migrateGuest(
    migration(log, { local: { "archives.import": () => ({ scenario_ids: [] }) } }),
  );
  assert.equal(outcome.status, "failed");
  assert.ok(!log.includes("logout"));
});

test("a guest with no plans is logged out without an import", async () => {
  const log: string[] = [];
  const outcome = await migrateGuest(
    migration(log, { remote: { "archives.exportAll": () => ({ ...ARCHIVE, plans: [] }) } }),
  );
  assert.deepEqual(outcome, { status: "empty" });
  assert.deepEqual(log, ["remote.archives.exportAll", "logout"]);
});

test("a logout that fails after the copy is safe does not undo the migration", async () => {
  const outcome = await migrateGuest(
    migration([], {
      logout: async () => {
        throw new Error("cookie expired");
      },
    }),
  );
  assert.deepEqual(outcome, { status: "migrated", plans: 1 });
});
