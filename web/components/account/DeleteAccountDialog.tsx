"use client";

import { useState } from "react";
import { Dialog, Field, Input } from "@/components/ui";
import { api } from "@/lib/api/client";
import { useSubmit } from "@/lib/hooks/useSubmit";

/**
 * Deleting the account.
 *
 * The password is re-typed rather than taken from the cookie: this is the one
 * action in the app that cannot be undone, and the session that opened the tab
 * this morning is not enough of an intent signal for it.
 */
export function DeleteAccountDialog({
  email,
  scenarioCount,
  onClose,
  onDeleted,
}: {
  email: string;
  scenarioCount: number;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const submit = useSubmit();

  const armed = confirm.trim().toLowerCase() === "delete" && password.length > 0;

  return (
    <Dialog
      title="Delete account"
      onClose={onClose}
      submitLabel="Delete account"
      busy={submit.busy || !armed}
      error={submit.error}
      onSubmit={() => {
        if (!armed) return;
        submit.run(() => api.account.remove({ password }), onDeleted);
      }}
    >
      <p style={{ margin: 0, fontSize: 13, lineHeight: 1.55 }}>
        This removes <strong style={{ fontWeight: 500 }}>{email}</strong>, its{" "}
        {scenarioCount} {scenarioCount === 1 ? "scenario" : "scenarios"}, every
        cached run and every signed-in device. Export anything you want to keep
        first — this cannot be undone.
      </p>
      <Field label="Password">
        <Input
          type="password"
          autoComplete="current-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      </Field>
      <Field label={`Type "delete" to confirm`}>
        <Input value={confirm} onChange={(e) => setConfirm(e.target.value)} />
      </Field>
    </Dialog>
  );
}
