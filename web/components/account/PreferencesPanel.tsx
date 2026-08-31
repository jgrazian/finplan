"use client";

import { useState } from "react";
import { Field, NumberInput } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { UserResponse } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { PanelNote, SaveRow } from "./chrome";

interface Draft {
  iterations: number;
  years: number;
  autoRun: boolean;
}

const toDraft = (user: UserResponse): Draft => ({
  iterations: user.default_iterations,
  years: user.default_duration_years,
  autoRun: user.auto_run,
});

/**
 * The defaults a new run and a new scenario start from.
 *
 * Deliberately short: every field here is one the app actually reads. A
 * preference that is stored and never consulted is worse than an absent one,
 * because it looks like it is doing something.
 */
export function PreferencesPanel({
  user,
  onSaved,
  readOnly,
}: {
  user: UserResponse;
  onSaved: (user: UserResponse) => void;
  readOnly?: boolean;
}) {
  const [draft, setDraft] = useState(() => toDraft(user));
  const [synced, setSynced] = useState(user);
  const submit = useSubmit();

  if (synced !== user) {
    setSynced(user);
    setDraft(toDraft(user));
  }

  const saved = toDraft(user);
  const dirty =
    draft.iterations !== saved.iterations ||
    draft.years !== saved.years ||
    draft.autoRun !== saved.autoRun;

  return (
    <div>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14, maxWidth: 520 }}>
        <Field label="Default Monte Carlo iterations">
          <NumberInput
            value={draft.iterations}
            decimals={0}
            min={1}
            readOnly={readOnly}
            aria-label="Default Monte Carlo iterations"
            onValueChange={(iterations) => setDraft((held) => ({ ...held, iterations }))}
          />
        </Field>
        <Field label="Default scenario horizon">
          <NumberInput
            value={draft.years}
            suffix="years"
            decimals={0}
            min={1}
            max={120}
            readOnly={readOnly}
            aria-label="Default scenario horizon in years"
            onValueChange={(years) => setDraft((held) => ({ ...held, years }))}
          />
        </Field>
      </div>

      <label className="radio" style={{ marginTop: 14 }}>
        <input
          type="checkbox"
          checked={draft.autoRun}
          disabled={readOnly}
          onChange={(e) => setDraft((held) => ({ ...held, autoRun: e.target.checked }))}
        />
        <span className="dot" />
        Re-run the active scenario automatically after an edit
      </label>

      <PanelNote>
        Iterations are what the Run button asks for; the server still caps them
        at its own <code style={{ fontFamily: "ui-monospace, Menlo, monospace" }}>
          --max-iterations
        </code>
        . The horizon seeds a new scenario. Auto re-run waits for an edit to
        settle before starting, and never while the server is unreachable.
      </PanelNote>

      <SaveRow
        label="Save defaults"
        dirty={dirty}
        busy={submit.busy}
        error={submit.error}
        readOnly={readOnly}
        onDiscard={() => setDraft(toDraft(user))}
        onSave={() =>
          submit.run(
            async () =>
              onSaved(
                await api.account.updatePreferences({
                  default_iterations: draft.iterations,
                  default_duration_years: draft.years,
                  auto_run: draft.autoRun,
                }),
              ),
            () => {},
          )
        }
      />
    </div>
  );
}
