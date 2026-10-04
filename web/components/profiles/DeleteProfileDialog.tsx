"use client";

import { Dialog } from "@/components/ui";
import { usePlanApi } from "@/lib/nav";
import { useSubmit } from "@/lib/hooks/useSubmit";

/**
 * Deleting a return or inflation profile from the library.
 *
 * The library is shared by every scenario, so the dialog says what the delete
 * reaches beyond this one. A return profile still mapped anywhere is refused
 * (the inspector does not offer it; the server says which rows if it is asked
 * anyway). An inflation profile can go while in use: the scenarios that used
 * it run without inflation until another is chosen, which the dialog says
 * when this scenario is one of them.
 */
export function DeleteProfileDialog({
  kind,
  profile,
  active,
  onClose,
  onDeleted,
}: {
  kind: "return" | "inflation";
  /** `id` is the profile's name in the view models; `serverId` its row id. */
  profile: { id: string; serverId: number };
  /** The inflation profile is this scenario's. */
  active?: boolean;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const api = usePlanApi();
  const submit = useSubmit();
  const label = kind === "return" ? "return profile" : "inflation profile";
  return (
    <Dialog
      title={`Delete ${label}`}
      submitLabel="Delete profile"
      busy={submit.busy}
      error={submit.error}
      onClose={onClose}
      onSubmit={() =>
        submit.run(
          () =>
            kind === "return"
              ? api.returnProfiles.remove(profile.serverId)
              : api.inflationProfiles.remove(profile.serverId),
          onDeleted,
        )
      }
    >
      <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
        Delete <strong style={{ fontWeight: 500 }}>{profile.id}</strong> from your profile
        library? The library is shared, so it is gone from every scenario. This cannot be undone.
      </p>
      {kind === "inflation" && (
        <p style={{ margin: "10px 0 0", fontSize: 13, lineHeight: 1.5 }} className="ns-mut">
          {active
            ? "This scenario uses it: it will run without inflation adjustment until you choose another profile. "
            : ""}
          Any other scenario using it is left with none as well.
        </p>
      )}
    </Dialog>
  );
}
