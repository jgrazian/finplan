"use client";

import { localApi } from "@/lib/api/local";
import { remoteApi } from "@/lib/api/remote";
import type { Scenario } from "@/lib/api/types";
import { DownloadToDeviceDialog, MoveToCloudDialog, PlanFilesDialog } from "./HomeDialogs";

/** What the person asked to do about a plan's home. */
export type HomeRequest =
  | { kind: "move"; plan: Scenario }
  | { kind: "download"; plan: Scenario }
  | { kind: "files" };

/**
 * The dialogs for changing a plan's home, opened by a request held in the
 * workbench so the plan bar, the Data panel and the empty state can all ask
 * for them. Opening the result is the workbench's: `onOpen` gets the plan that
 * now exists, in whichever home it landed.
 */
export function HomeDialogHost({
  request,
  localPlans,
  account,
  onRequest,
  onClose,
  onOpen,
  onChanged,
}: {
  request: HomeRequest | undefined;
  /** Every plan on this device, for the files dialog. */
  localPlans: Scenario[];
  /** Signed in to an account, so Move to cloud is on offer from the files list. */
  account: boolean;
  onRequest: (request: HomeRequest) => void;
  onClose: () => void;
  onOpen: (plan: Scenario) => void;
  /** The set of plans, or a plan's export state, changed. */
  onChanged: () => void;
}) {
  if (!request) return null;

  if (request.kind === "move") {
    return (
      <MoveToCloudDialog
        plan={request.plan}
        onClose={onClose}
        onMoved={(moved) => {
          onClose();
          void remoteApi.scenarios.get(moved.cloudId).then((created) => {
            onOpen(created);
          });
        }}
      />
    );
  }
  if (request.kind === "download") {
    return (
      <DownloadToDeviceDialog
        plan={request.plan}
        onClose={onClose}
        onDone={({ localId }) => {
          onClose();
          void localApi.scenarios.get(localId).then(onOpen);
        }}
      />
    );
  }
  return (
    <PlanFilesDialog
      plans={localPlans}
      onClose={onClose}
      onMoveToCloud={account ? (plan) => onRequest({ kind: "move", plan }) : undefined}
      onExported={onChanged}
      onImported={(ids) => {
        onClose();
        onChanged();
        const first = ids[0];
        if (first !== undefined) void localApi.scenarios.get(first).then(onOpen);
      }}
    />
  );
}
