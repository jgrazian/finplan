"use client";

import { Dialog } from "@/components/ui";
import { api } from "@/lib/api/client";
import { useSubmit } from "@/lib/hooks/useSubmit";

/**
 * Deleting one scenario, from the Plan tab's scenario strip or the Data list.
 *
 * A confirmation rather than a typed "delete": unlike the account, a scenario
 * can be exported first and imported back, which the dialog says.
 */
export function DeleteScenarioDialog({
  scenario,
  onClose,
  onDeleted,
}: {
  scenario: { id: number; name: string };
  onClose: () => void;
  onDeleted: (id: number) => void;
}) {
  const submit = useSubmit();
  return (
    <Dialog
      title="Delete scenario"
      submitLabel="Delete scenario"
      busy={submit.busy}
      error={submit.error}
      onClose={onClose}
      onSubmit={() => submit.run(() => api.scenarios.remove(scenario.id), () => onDeleted(scenario.id))}
    >
      <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
        Delete <strong style={{ fontWeight: 500 }}>{scenario.name}</strong> with its accounts,
        events, parameters and saved runs? This cannot be undone — export it from Account → Data
        first to keep a copy.
      </p>
    </Dialog>
  );
}
