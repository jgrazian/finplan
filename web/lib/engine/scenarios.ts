/**
 * The scenario, library and archive groups of the local backend: making,
 * renaming, copying, deleting and moving whole plans, and the device's library
 * of return profiles, inflation profiles and tax configs.
 */
import type { ArchiveImported } from "../api/generated/ArchiveImported.ts";
import type { ArchivePreview } from "../api/generated/ArchivePreview.ts";
import type { LibraryEditResult } from "../api/generated/LibraryEditResult.ts";
import type { PlanArchive } from "../api/generated/PlanArchive.ts";
import type { RestoreResult } from "../api/generated/RestoreResult.ts";
import type { SetupCreated } from "../api/generated/SetupCreated.ts";
import type {
  ArchivesApi,
  InflationProfilesApi,
  ReturnProfilesApi,
  ScenariosApi,
  TaxConfigsApi,
} from "../api/plan.ts";
import type {
  CompileReport,
  PreflightReport,
  Profile,
  Scenario,
  TaxConfig,
} from "../api/types.ts";
import { type Core, localSlug } from "./core.ts";
import { badRequest, conflict, guard, notFound } from "./errors.ts";
import { LIBRARY_LOCK, planLockName, withLock, withLocks } from "./locks.ts";
import type { PlanRecord, StoreTx } from "./store.ts";

/** Where the meta of one plan lives (what-if stack, cached sweep, review), so a delete can sweep it. */
export const planMetaKeys = (planId: number) => [
  `whatif:${planId}`,
  `sweep:${planId}`,
  `sweep-layout:${planId}`,
  `review:${planId}`,
  `dismissed:${planId}`,
];

/** A `Scenario` row of plan `id` as the engine reads it. */
function scenarioOf(core: Core, id: number): Promise<Scenario> {
  return core.read<Scenario>(id, { query: "scenario" });
}

/** Hooks the run group gives the scenario group, so deleting a plan stops and forgets its runs. */
export interface PlanLifecycle {
  /** The plan is being deleted: cancel its runs and analyses and drop what is cached about them. */
  forgetPlan(planId: number): void;
}

export function scenariosGroup(core: Core, lifecycle: PlanLifecycle): ScenariosApi {
  /**
   * A new plan from `build` (given its id, the library and the time), stored
   * with whatever `receipt` writes in the same transaction.
   */
  const make = async (
    build: (tx: StoreTx, id: number, library: string, now: string) => Promise<string> | string,
    receipt?: (tx: StoreTx, id: number) => Promise<void>,
  ): Promise<Scenario> => {
    const scenario = await core.store.transact("rw", async (tx) => {
      const id = await tx.nextId("plan");
      const library = await core.library(tx);
      const now = core.nowText();
      const graph = await build(tx, id, library, now);
      const name = (JSON.parse(graph) as { scenario: { name: string } }).scenario.name;
      await core.assertNameFree(tx, name);
      // The library is stored with the first plan, so its ids stay what the plan names.
      if ((await tx.getLibrary()) === undefined) await tx.putLibrary(library);
      await tx.putPlan({
        id,
        name,
        graph,
        created_at: now,
        updated_at: now,
        last_exported_at: null,
        edits_since_export: 0,
      });
      await receipt?.(tx, id);
      return core.readIn<Scenario>(tx, id, { query: "scenario" });
    });
    core.announce(null);
    return scenario;
  };

  return {
    list: () =>
      core.store.transact("r", async (tx) => {
        const plans = (await tx.listPlans()).sort(
          (a, b) => b.updated_at.localeCompare(a.updated_at) || b.id - a.id,
        );
        const library = await core.library(tx);
        const out: Scenario[] = [];
        for (const plan of plans) {
          const extras = await core.extras(tx, plan.id);
          out.push(
            core.call<Scenario>((engine) =>
              engine.read(plan.graph, library, JSON.stringify({ query: "scenario", extras })),
            ),
          );
        }
        return out;
      }),

    get: (id) => scenarioOf(core, id),

    create: (body) =>
      make((_tx, id, library, now) =>
        guard(() => core.engine.new_plan(JSON.stringify(body), library, id, now)),
      ),

    update: async (id, body) =>
      (await core.editPlan<Scenario>(id, { op: "update_scenario", body }, () => ({ query: "scenario" }))).read,

    setFunding: async (id, body) =>
      (await core.editPlan<Scenario>(id, { op: "set_funding", body }, () => ({ query: "scenario" }))).read,

    remove: async (id) => {
      lifecycle.forgetPlan(id);
      await withLock(planLockName(id), () =>
        core.store.transact("rw", async (tx) => {
          await core.requirePlan(tx, id);
          for (const run of await tx.listRuns(id)) await tx.deleteRun(run.id);
          for (const key of planMetaKeys(id)) await tx.deleteMeta(key);
          await tx.deletePlan(id);
        }),
      );
      core.announce(null);
    },

    duplicate: async (id, name) => {
      const source = await core.store.transact("r", (tx) => core.requirePlan(tx, id));
      return make((_tx, newId, _library, now) =>
        guard(() => core.engine.duplicate_plan(source.graph, newId, name, now)),
      );
    },

    setup: async (body) => {
      // A retry of the same answers returns the plan the first made.
      const receipt = `setup:${body.request_id}`;
      const printed = JSON.stringify(body);
      const prior = await core.store.transact("r", (tx) =>
        tx.getMeta<{ print: string; scenario_id: number }>(receipt),
      );
      if (prior) {
        if (prior.print !== printed) {
          throw badRequest("This setup was already saved with different inputs; start a new setup");
        }
        return { scenario_id: prior.scenario_id } satisfies SetupCreated;
      }
      const scenario = await make(
        (_tx, id, library, now) => guard(() => core.engine.setup_plan(printed, library, id, now)),
        (tx, id) => tx.putMeta(receipt, { print: printed, scenario_id: id }),
      );
      return { scenario_id: scenario.id };
    },

    compile: (id) => core.read<CompileReport>(id, { query: "compile_report" }),
    preflight: (id) => core.read<PreflightReport>(id, { query: "preflight" }),

    inputHash: (id) =>
      core.store.transact("r", async (tx) => {
        const plan = await core.requirePlan(tx, id);
        const library = await core.library(tx);
        const { hash } = core.call<{ hash: string }>((engine) => engine.snapshot(plan.graph, library));
        return { input_hash: hash };
      }),

    archive: (id) =>
      core.store.transact("r", async (tx) => {
        const plan = await core.requirePlan(tx, id);
        return core.call<PlanArchive>((engine) => engine.export_archive(`[${plan.graph}]`));
      }),
  };
}

// ── the library ─────────────────────────────────────────────────────────────

/** A library write: one `LibraryOp`, the plans it reached written back with it, then `then`'s read. */
async function writeLibrary<T>(
  core: Core,
  op: object,
  then?: (created: number | null) => object | undefined,
): Promise<T | undefined> {
  const ids = await core.store.transact("r", async (tx) => (await tx.listPlans()).map((p) => p.id));
  const result = await withLocks([LIBRARY_LOCK, ...ids.map(planLockName)], () =>
    core.store.transact("rw", async (tx) => {
      const library = await core.library(tx);
      const { plans, json } = await core.graphsJson(tx);
      const now = core.nowText();
      const out = core.call<LibraryEditResult>((engine) =>
        engine.apply_library(library, json, JSON.stringify(op), now),
      );
      const newLibrary = JSON.stringify(out.library);
      await tx.putLibrary(newLibrary);
      const changed = new Map<number, string>();
      for (const graph of out.changed_plans) {
        const id = (graph as { scenario: { id: number } }).scenario.id;
        changed.set(id, JSON.stringify(graph));
      }
      const after: string[] = [];
      for (const plan of plans) {
        const graph = changed.get(plan.id);
        if (graph !== undefined) {
          await tx.putPlan({
            ...plan,
            graph,
            updated_at: now,
            edits_since_export: plan.edits_since_export + 1,
          });
          after.push(graph);
        } else {
          after.push(plan.graph);
        }
      }
      const query = then?.(out.outcome.id);
      const read = query
        ? core.call<T>((engine) =>
            engine.read_library(newLibrary, `[${after.join(",")}]`, JSON.stringify(query)),
          )
        : undefined;
      return { read, changed: [...changed.keys()] };
    }),
  );
  core.announce(null);
  for (const id of result.changed) core.announce(id);
  return result.read;
}

async function readLibrary<T>(core: Core, query: object): Promise<T> {
  return core.store.transact("r", async (tx) => {
    const library = await core.library(tx);
    const { json } = await core.graphsJson(tx);
    return core.call<T>((engine) => engine.read_library(library, json, JSON.stringify(query)));
  });
}

const required = <T>(value: T | undefined): T => {
  if (value === undefined) throw notFound("library item");
  return value;
};

export function returnProfilesGroup(core: Core): ReturnProfilesApi {
  return {
    list: () => readLibrary<Profile[]>(core, { query: "return_profiles" }),
    create: async (body) =>
      required(
        await writeLibrary<Profile>(core, { op: "create_return_profile", body }, (id) => ({
          query: "return_profile",
          id,
        })),
      ),
    update: async (id, body) =>
      required(
        await writeLibrary<Profile>(core, { op: "update_return_profile", id, body }, () => ({
          query: "return_profile",
          id,
        })),
      ),
    remove: async (id) => {
      await writeLibrary(core, { op: "delete_return_profile", id });
    },
    reorder: async (ids) => {
      await writeLibrary(core, { op: "reorder_return_profiles", ids });
    },
  };
}

export function inflationProfilesGroup(core: Core): InflationProfilesApi {
  return {
    list: () => readLibrary<Profile[]>(core, { query: "inflation_profiles" }),
    create: async (body) => {
      let createdId: number | null = null;
      const profiles = required(
        await writeLibrary<Profile[]>(core, { op: "create_inflation_profile", body }, (id) => {
          createdId = id;
          return { query: "inflation_profiles" };
        }),
      );
      // The library has no single read for an inflation profile: take the new one from the list.
      const created = profiles.find((p) => p.id === createdId);
      if (!created) throw notFound("inflation profile");
      return created;
    },
    remove: async (id) => {
      await writeLibrary(core, { op: "delete_inflation_profile", id });
    },
    reorder: async (ids) => {
      await writeLibrary(core, { op: "reorder_inflation_profiles", ids });
    },
  };
}

export function taxConfigsGroup(core: Core): TaxConfigsApi {
  return {
    list: () => readLibrary<TaxConfig[]>(core, { query: "tax_configs" }),
    create: async (body) =>
      required(
        await writeLibrary<TaxConfig>(core, { op: "create_tax_config", body }, (id) => ({
          query: "tax_config",
          id,
        })),
      ),
    update: async (id, body) =>
      required(
        await writeLibrary<TaxConfig>(core, { op: "update_tax_config", id, body }, () => ({
          query: "tax_config",
          id,
        })),
      ),
    remove: async (id) => {
      await writeLibrary(core, { op: "delete_tax_config", id });
    },
  };
}

// ── archives ────────────────────────────────────────────────────────────────

/** A 53-bit string hash (cyrb53): enough to tell one import request from another. */
function fingerprint(text: string): string {
  let h1 = 0xdeadbeef;
  let h2 = 0x41c6ce57;
  for (let i = 0; i < text.length; i++) {
    const ch = text.charCodeAt(i);
    h1 = Math.imul(h1 ^ ch, 2654435761);
    h2 = Math.imul(h2 ^ ch, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  return (4294967296 * (2097151 & h2) + (h1 >>> 0)).toString(16);
}

export function archivesGroup(core: Core): ArchivesApi {
  const random = (): string =>
    Array.from(globalThis.crypto.getRandomValues(new Uint8Array(4)), (b) => b.toString(16).padStart(2, "0")).join("");

  return {
    exportAll: () =>
      core.store.transact("r", async (tx) => {
        const plans = (await tx.listPlans()).sort((a, b) => a.id - b.id);
        return core.call<PlanArchive>((engine) =>
          engine.export_archive(`[${plans.map((p) => p.graph).join(",")}]`),
        );
      }),

    preview: async (archive) =>
      core.call<ArchivePreview>((engine) => engine.preview_archive(JSON.stringify(archive))),

    import: async (body): Promise<ArchiveImported> => {
      if (!body.request_id || body.request_id.length > 100 || (body.name_prefix ?? "").length > 80) {
        throw badRequest("A request key and a name prefix of at most 80 characters are required.");
      }
      const archiveJson = JSON.stringify(body.archive);
      const print = fingerprint(`${archiveJson}\n${body.name_prefix}`);
      const receipt = `import:${body.request_id}`;
      const result = await core.store.transact("rw", async (tx) => {
        const prior = await tx.getMeta<{ print: string; scenario_ids: number[] }>(receipt);
        if (prior) {
          if (prior.print !== print) throw conflict("This request key was used for a different import.");
          return { scenario_ids: prior.scenario_ids };
        }
        const graphs = core.call<unknown[]>((engine) => engine.import_archive(archiveJson));
        let library = await core.library(tx);
        const now = core.nowText();
        const ids: number[] = [];
        const taken = new Set((await tx.listPlans()).map((p) => p.name));
        for (const source of graphs) {
          const name = `${body.name_prefix ?? ""}${(source as { scenario: { name: string } }).scenario.name}`;
          if (taken.has(name)) throw conflict("a scenario with that name already exists");
          taken.add(name);
          const id = await tx.nextId("plan");
          const restored = core.call<RestoreResult>((engine) =>
            engine.restore_plan(library, JSON.stringify(source), id, name, now, random()),
          );
          library = JSON.stringify(restored.library);
          const graph = JSON.stringify(restored.graph);
          await tx.putPlan({
            id,
            name: name.trim(),
            graph,
            created_at: now,
            updated_at: now,
            last_exported_at: null,
            edits_since_export: 0,
          });
          ids.push(id);
        }
        await tx.putLibrary(library);
        await tx.putMeta(receipt, { print, scenario_ids: ids });
        return { scenario_ids: ids };
      });
      core.announce(null);
      return result;
    },
  };
}

export type { PlanRecord };
export { localSlug };
