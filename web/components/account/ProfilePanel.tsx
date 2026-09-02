"use client";

import { useState } from "react";
import { DateInput, Field, Input } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { UserResponse } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { PanelNote, SaveRow } from "./chrome";

interface Draft {
  displayName: string;
  email: string;
  birthDate: string;
}

const toDraft = (user: UserResponse): Draft => ({
  displayName: user.display_name ?? "",
  email: user.email,
  birthDate: user.birth_date ?? "",
});

/**
 * Who the account belongs to, and what a new scenario inherits from them.
 *
 * One Save over the whole block rather than a field at a time: unlike the Plan
 * strip, nothing here is being iterated on with a chart watching, and a
 * save-per-keystroke on an email address would lock you out mid-word. That is
 * also why the route is a PUT — Discard is only meaningful against a form that
 * holds every field.
 */
export function ProfilePanel({
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

  // Derived rather than synced in an effect: a save returns a new user object,
  // and that is the moment the form should agree with the server again.
  if (synced !== user) {
    setSynced(user);
    setDraft(toDraft(user));
  }

  const saved = toDraft(user);
  const dirty =
    draft.displayName !== saved.displayName ||
    draft.email !== saved.email ||
    draft.birthDate !== saved.birthDate;

  const set = (patch: Partial<Draft>) => setDraft((held) => ({ ...held, ...patch }));

  return (
    <div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: 14,
          maxWidth: 520,
        }}
      >
        <Field label="Display name">
          <Input
            value={draft.displayName}
            readOnly={readOnly}
            placeholder="optional"
            onChange={(e) => set({ displayName: e.target.value })}
          />
        </Field>
        <Field label="Email">
          <Input
            type="email"
            value={draft.email}
            readOnly={readOnly}
            onChange={(e) => set({ email: e.target.value })}
          />
        </Field>
        <Field label="Birth date">
          <DateInput
            value={draft.birthDate}
            readOnly={readOnly}
            onValueChange={(birthDate) => set({ birthDate })}
          />
        </Field>
      </div>

      <PanelNote>
        The birth date seeds every new scenario — the one thing an age-based
        trigger cannot do without. A scenario that already exists keeps its own,
        and can still be changed on the Plan screen.
      </PanelNote>

      <SaveRow
        label="Save changes"
        dirty={dirty}
        busy={submit.busy}
        error={submit.error}
        readOnly={readOnly}
        onDiscard={() => setDraft(toDraft(user))}
        onSave={() =>
          submit.run(
            async () =>
              onSaved(
                await api.account.updateProfile({
                  email: draft.email.trim(),
                  display_name: draft.displayName.trim() || null,
                  birth_date: draft.birthDate || null,
                }),
              ),
            () => {},
          )
        }
      />
    </div>
  );
}
