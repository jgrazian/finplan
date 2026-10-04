"use client";

import { useState } from "react";
import { Button, Dialog } from "@/components/ui";
import { localApi } from "@/lib/api/local";
import { remoteApi } from "@/lib/api/remote";
import type { Scenario } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import {
  type MovedToCloud,
  copyToDevice,
  deleteCloudCopy,
  moveToCloud,
} from "@/lib/local/homes";
import { LocalPlansPanel } from "./LocalPlansPanel";

type PlanRef = { id: number; name: string };

/**
 * Local to cloud. The one place a local plan is sent to the server by choice,
 * so the dialog says that, and says what happens to the local copy.
 */
export function MoveToCloudDialog({
  plan,
  onClose,
  onMoved,
}: {
  plan: PlanRef;
  onClose: () => void;
  /** The cloud plan exists; open it. */
  onMoved: (moved: MovedToCloud) => void;
}) {
  const submit = useSubmit();
  // One key per dialog, so a retry after a dropped answer cannot make two plans.
  const [requestId] = useState(() => crypto.randomUUID());
  const [partial, setPartial] = useState<MovedToCloud>();

  if (partial) {
    return (
      <Dialog
        title="Moved to the cloud"
        onClose={() => onMoved(partial)}
        onSubmit={() => onMoved(partial)}
        submitLabel="Open the cloud plan"
        error={`The plan is in the cloud, but the copy on this device could not be deleted: ${partial.deleteError ?? "unknown error"}. You can delete it yourself.`}
      >
        <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
          <strong style={{ fontWeight: 500 }}>{plan.name}</strong>{" "}is now stored on FinPlan&rsquo;s
          servers.
        </p>
      </Dialog>
    );
  }

  return (
    <Dialog
      title="Move to cloud"
      submitLabel="Move to cloud"
      busy={submit.busy}
      error={submit.error}
      onClose={onClose}
      onSubmit={() =>
        submit.run(
          async () => {
            const moved = await moveToCloud(
              { local: localApi, remote: remoteApi },
              plan.id,
              requestId,
            );
            if (moved.localDeleted) onMoved(moved);
            else setPartial(moved);
          },
          () => undefined,
        )
      }
    >
      <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
        <strong style={{ fontWeight: 500 }}>{plan.name}</strong>{" "}will be stored on FinPlan&rsquo;s
        servers, with your account. That is what lets you use it on another device and use AI
        review on it. Once it is there, the copy on this device is deleted.
      </p>
      <p style={{ margin: 0, fontSize: 12.5, lineHeight: 1.5 }} className="ns-mut">
        Saved runs are not moved; results are recomputed there. Your plan limit applies.
      </p>
    </Dialog>
  );
}

/**
 * Cloud to local. The copy is made first and nothing is deleted until the
 * person says so; the default is to keep the cloud copy.
 */
export function DownloadToDeviceDialog({
  plan,
  onClose,
  onDone,
}: {
  plan: PlanRef;
  onClose: () => void;
  /** The local plan to open, and whether the cloud copy was deleted. */
  onDone: (result: { localId: number; cloudDeleted: boolean }) => void;
}) {
  const submit = useSubmit();
  const [requestId] = useState(() => crypto.randomUUID());
  const [copied, setCopied] = useState<number>();

  if (copied !== undefined) {
    const finish = (cloudDeleted: boolean) =>
      submit.run(
        async () => {
          if (cloudDeleted) await deleteCloudCopy({ local: localApi, remote: remoteApi }, plan.id);
        },
        () => onDone({ localId: copied, cloudDeleted }),
      );
    return (
      <Dialog
        title="Delete the cloud copy?"
        busy={submit.busy}
        error={submit.error}
        onClose={() => onDone({ localId: copied, cloudDeleted: false })}
        onSubmit={() => finish(false)}
        footer={
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 4 }}>
            <Button type="button" disabled={submit.busy} onClick={() => finish(true)}>
              Delete cloud copy
            </Button>
            <Button type="submit" variant="primary" disabled={submit.busy}>
              Keep cloud copy
            </Button>
          </div>
        }
      >
        <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
          <strong style={{ fontWeight: 500 }}>{plan.name}</strong> is now on this device. Keep the
          copy in the cloud too, or delete it? A kept copy is a separate plan: the two do not sync.
        </p>
      </Dialog>
    );
  }

  return (
    <Dialog
      title="Move to device"
      submitLabel="Move to device"
      busy={submit.busy}
      error={submit.error}
      onClose={onClose}
      onSubmit={() =>
        submit.run(
          async () =>
            setCopied(
              (await copyToDevice({ local: localApi, remote: remoteApi }, plan.id, requestId))
                .localId,
            ),
          () => undefined,
        )
      }
    >
      <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
        A copy of <strong style={{ fontWeight: 500 }}>{plan.name}</strong> will be saved in this
        browser. AI review and plan chat only work on plans in the cloud, so they will not be
        available on the copy.
      </p>
      <p style={{ margin: 0, fontSize: 12.5, lineHeight: 1.5 }} className="ns-mut">
        Saved runs are not copied; results are recomputed here.
      </p>
    </Dialog>
  );
}

/** Export and import files, for plans on this device. */
export function PlanFilesDialog({
  plans,
  onClose,
  onMoveToCloud,
  onImported,
  onExported,
}: {
  plans: Scenario[];
  onClose: () => void;
  onMoveToCloud?: (plan: Scenario) => void;
  onImported: (scenarioIds: number[]) => void;
  onExported?: () => void;
}) {
  return (
    <Dialog
      title="Export or import plans"
      width={680}
      onClose={onClose}
      onSubmit={onClose}
      footer={
        <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 4 }}>
          <Button type="submit" variant="primary">
            Done
          </Button>
        </div>
      }
    >
      <LocalPlansPanel
        plans={plans}
        onMoveToCloud={onMoveToCloud}
        onImported={onImported}
        onExported={onExported}
      />
    </Dialog>
  );
}
