"use client";

import { HOME_LABEL, HomeIcon } from "@/components/local/HomeIcon";
import { useState } from "react";
import { Field, Hr, NumberInput, SectionHeading, SegmentedControl } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { UserResponse } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { useLocalMode } from "@/lib/local/useLocalMode";
import type { PlanHome } from "@/lib/nav";
import { AppearanceFields } from "./AppearanceFields";
import { PanelNote, SaveRow } from "./chrome";

interface Draft {
  iterations: number;
  years: number;
  autoRun: boolean;
  home: PlanHome;
}

const toDraft = (user: UserResponse): Draft => ({
  iterations: user.default_iterations,
  years: user.default_duration_years,
  autoRun: user.auto_run,
  home: user.default_plan_home ?? "local",
});

/**
 * The defaults a new run and a new scenario start from, and — kept on the
 * device rather than the account — how the app is drawn while it does it.
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
  const localMode = useLocalMode();

  if (synced !== user) {
    setSynced(user);
    setDraft(toDraft(user));
  }

  const saved = toDraft(user);
  const dirty =
    draft.iterations !== saved.iterations ||
    draft.years !== saved.years ||
    draft.autoRun !== saved.autoRun ||
    draft.home !== saved.home;

  return (
    <div>
      <div className="mobile-stack" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14, maxWidth: 520 }}>
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

      {localMode && (
        <div style={{ marginTop: 14 }}>
          <Field label="Default home for new plans">
            <SegmentedControl<PlanHome>
              name="default-plan-home"
              ariaLabel="Default home for new plans"
              value={draft.home}
              onChange={(home) => !readOnly && setDraft((held) => ({ ...held, home }))}
              options={[
                {
                  value: "local",
                  label: HOME_LABEL.local,
                  icon: <HomeIcon home="local" />,
                  title: "New plans stay in this browser.",
                },
                {
                  value: "cloud",
                  label: HOME_LABEL.cloud,
                  icon: <HomeIcon home="cloud" />,
                  title: "New plans are saved to your FinPlan account.",
                },
              ]}
            />
          </Field>
        </div>
      )}

      <PanelNote>
        These defaults set the simulation count and length of new plans.
        You can adjust the simulation count on Results, within the service limit.
        Auto re-run starts after saved edits and pauses while you are offline.
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
                  // A full replace on the server's side for the fields above;
                  // this one is sent as it stands, saved or not.
                  default_plan_home: draft.home,
                }),
              ),
            () => {},
          )
        }
      />

      <Hr />
      <div style={{ marginBottom: 10 }}>
        <SectionHeading>Appearance</SectionHeading>
      </div>
      {/* After Save rather than above it: these apply at once and Save never
          sees them, so they sit outside the fields it commits. */}
      <AppearanceFields />
    </div>
  );
}
