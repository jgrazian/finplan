/**
 * Moving a plan between homes, and files in and out of the local store (spec
 * 19, "Plan homes"). Each plan has exactly one home and there is no sync: a
 * move is a copy into the new home followed, only if the copy succeeded, by a
 * delete in the old one.
 *
 * The two `PlanApi`s are passed in, so this module reaches neither `fetch` nor
 * the store worker and the order of the steps is tested with stand-ins. The
 * rule every function here keeps: the delete comes after the import, and never
 * when the import failed.
 */
import type { PlanApi } from "../api/plan.ts";
import type { ArchivePreview } from "../api/generated/ArchivePreview.ts";
import type { PlanArchive } from "../api/generated/PlanArchive.ts";
import type { LocalRuntime } from "./runtime.ts";

export interface HomeDeps {
  local: PlanApi;
  remote: PlanApi;
}

export interface MovedToCloud {
  cloudId: number;
  /** False when the cloud copy exists but the local one could not be deleted. */
  localDeleted: boolean;
  deleteError?: string;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * Local to cloud: export the plan, import it on the server, and only then
 * delete the local copy. `requestId` makes the server import idempotent, so the
 * dialog keeps one per attempt and a retry after a dropped answer cannot make
 * two plans. An import refused (plan-slot limit, validation) throws with the
 * server's own words and leaves the local plan untouched.
 */
export async function moveToCloud(
  { local, remote }: HomeDeps,
  localId: number,
  requestId: string,
): Promise<MovedToCloud> {
  const archive = await local.scenarios.archive(localId);
  const imported = await remote.archives.import({
    archive,
    name_prefix: "",
    request_id: requestId,
    from_guest: false,
  });
  const cloudId = imported.scenario_ids[0];
  if (cloudId === undefined) throw new Error("The server did not create the plan.");
  try {
    await local.scenarios.remove(localId);
    return { cloudId, localDeleted: true };
  } catch (error) {
    return { cloudId, localDeleted: false, deleteError: message(error) };
  }
}

/** Cloud to local, first half: the copy. The cloud plan is not touched. */
export async function copyToDevice(
  { local, remote }: HomeDeps,
  cloudId: number,
  requestId: string,
): Promise<{ localId: number }> {
  const archive = await remote.scenarios.archive(cloudId);
  const imported = await local.archives.import({
    archive,
    name_prefix: "",
    request_id: requestId,
    from_guest: false,
  });
  const localId = imported.scenario_ids[0];
  if (localId === undefined) throw new Error("The plan could not be saved to this device.");
  return { localId };
}

/** Cloud to local, second half: the person's answer to "Delete the cloud copy?". */
export async function deleteCloudCopy({ remote }: HomeDeps, cloudId: number): Promise<void> {
  await remote.scenarios.remove(cloudId);
}

/** `Retirement plan` on 2026-10-03 is `finplan-retirement-plan-2026-10-03.json`. */
export function archiveFileName(name: string, now: Date = new Date()): string {
  const slug = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
  return `finplan-${slug || "plan"}-${now.toISOString().slice(0, 10)}.json`;
}

export type ExportTarget =
  | { kind: "one"; id: number; name: string }
  | { kind: "all"; ids: number[] };

/**
 * Hands one plan, or every local plan, to `save` as an archive, then records
 * the export so the backup reminder stands down. Marking comes after `save`
 * returns: a download that threw was not a backup.
 */
export async function exportLocal(
  { local }: Pick<HomeDeps, "local">,
  runtime: Pick<LocalRuntime, "markExported"> | undefined,
  target: ExportTarget,
  save: (filename: string, archive: PlanArchive) => void,
  now: Date = new Date(),
): Promise<void> {
  if (target.kind === "one") {
    save(archiveFileName(target.name, now), await local.scenarios.archive(target.id));
    await runtime?.markExported([target.id]);
    return;
  }
  const archive = await local.archives.exportAll();
  save(archiveFileName("plans", now), archive);
  await runtime?.markExported(target.ids);
}

/** A file's text as an archive, or why it is not one, in words for the person who chose it. */
export function parseArchive(text: string): PlanArchive {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error("That file is not a FinPlan archive: it is not valid JSON.");
  }
  const archive = parsed as Partial<PlanArchive> | null;
  if (
    archive === null ||
    typeof archive !== "object" ||
    typeof archive.format !== "string" ||
    !Array.isArray(archive.plans)
  ) {
    throw new Error("That file is not a FinPlan archive.");
  }
  return archive as PlanArchive;
}

/**
 * What importing would add, when the local store can say; `undefined` when it
 * does not preview, in which case the import goes ahead on the file's say-so.
 * A refusal that is about the file itself still throws.
 */
export async function previewLocalArchive(
  { local }: Pick<HomeDeps, "local">,
  archive: PlanArchive,
): Promise<ArchivePreview | undefined> {
  try {
    return await local.archives.preview(archive);
  } catch (error) {
    if ((error as { code?: unknown } | null)?.code === "local_unavailable") return undefined;
    throw error;
  }
}

/** Imports a file's plans into the local store as new plans; names are kept. */
export async function importIntoDevice(
  { local }: Pick<HomeDeps, "local">,
  archive: PlanArchive,
  requestId: string,
): Promise<number[]> {
  const imported = await local.archives.import({
    archive,
    name_prefix: "",
    request_id: requestId,
    from_guest: false,
  });
  return imported.scenario_ids;
}

export interface GuestMigrationDeps extends HomeDeps {
  /** Ends the guest's session. */
  logout: () => Promise<void>;
  /** Resolves true once the local store can take the import, false if it never will. */
  localReady: () => Promise<boolean>;
  requestId: string;
}

export type GuestMigration =
  | { status: "migrated"; plans: number }
  /** The guest had no plans; it was logged out all the same. */
  | { status: "empty" }
  /** Nothing was changed and the guest is still signed in. */
  | { status: "failed"; error: string };

/**
 * A spec 17 guest's first visit with local mode on: export their plans, import
 * them into the local store, then log the guest out. The guest is logged out
 * only if the local import succeeded; any failure before that leaves the
 * session, and so the plans on the server, exactly as they were.
 */
export async function migrateGuest(deps: GuestMigrationDeps): Promise<GuestMigration> {
  try {
    const archive = await deps.remote.archives.exportAll();
    if (archive.plans.length === 0) {
      await deps.logout().catch(() => undefined);
      return { status: "empty" };
    }
    if (!(await deps.localReady())) {
      return { status: "failed", error: "Local plans are not available in this browser." };
    }
    const imported = await deps.local.archives.import({
      archive,
      name_prefix: "",
      request_id: deps.requestId,
      from_guest: true,
    });
    if (imported.scenario_ids.length === 0) {
      return { status: "failed", error: "The plans could not be saved to this device." };
    }
    // The copy is safe on this device. A logout that fails (an expired cookie)
    // changes nothing the person can lose, so it is not a failure.
    await deps.logout().catch(() => undefined);
    return { status: "migrated", plans: imported.scenario_ids.length };
  } catch (error) {
    return { status: "failed", error: message(error) };
  }
}
