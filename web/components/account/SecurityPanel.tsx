"use client";

import { useState } from "react";
import { Button, Field, Input, Table, Tag, Td, Th } from "@/components/ui";
import { api } from "@/lib/api/client";
import { useAsync } from "@/lib/hooks/useAsync";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { PanelHeading, PanelNote } from "./chrome";
import { deviceLabel, isoDate, timeAgo } from "./device";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/**
 * The password, and every device holding a key to this account.
 *
 * The session list is the honest half of the security story: a password is a
 * secret you can change, but a stolen session is a door that stays open until
 * someone shuts it. Each row is identified by an opaque id, never by anything
 * derived from the token itself.
 */
export function SecurityPanel({ readOnly }: { readOnly?: boolean }) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [changed, setChanged] = useState(false);
  const password = useSubmit();
  const revoking = useSubmit();
  const sessions = useAsync(() => api.account.sessions(), []);

  const rows = sessions.data ?? [];

  return (
    <div>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14, maxWidth: 520 }}>
        <Field label="Current password">
          <Input
            type="password"
            autoComplete="current-password"
            value={current}
            readOnly={readOnly}
            onChange={(e) => {
              setCurrent(e.target.value);
              setChanged(false);
            }}
          />
        </Field>
        <Field label="New password">
          <Input
            type="password"
            autoComplete="new-password"
            placeholder="10+ characters"
            value={next}
            readOnly={readOnly}
            onChange={(e) => {
              setNext(e.target.value);
              setChanged(false);
            }}
          />
        </Field>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 12 }}>
        <Button
          disabled={!current || !next || password.busy || readOnly}
          title={readOnly ? "No connection to the server." : undefined}
          onClick={() =>
            password.run(
              () => api.account.changePassword({ current_password: current, new_password: next }),
              () => {
                setCurrent("");
                setNext("");
                setChanged(true);
                // Every other session was just ended, so the list is stale.
                sessions.reload();
              },
            )
          }
        >
          {password.busy ? "…" : "Update password"}
        </Button>
        {changed && <span style={{ fontSize: 12, color: MUTED }}>Password changed.</span>}
        {password.error && (
          <span style={{ fontSize: 12, color: "var(--color-accent-700)" }}>{password.error}</span>
        )}
      </div>

      <PanelNote>
        Changing the password ends every other session — a password change is
        usually an attempt to revoke someone, and leaving their door open would
        defeat it. This device stays signed in.
      </PanelNote>

      <PanelHeading>Sessions</PanelHeading>
      {sessions.error ? (
        <p style={{ fontSize: 12, color: "var(--color-accent-700)" }}>{sessions.error.message}</p>
      ) : rows.length === 0 ? (
        <p style={{ fontSize: 12, color: MUTED }}>
          {sessions.loading ? "Loading…" : "No active sessions."}
        </p>
      ) : (
        <div style={{ maxWidth: 700 }}>
          <Table>
            <thead>
              <tr>
                <Th>Device</Th>
                <Th>Signed in</Th>
                <Th>Last active</Th>
                <Th align="right" />
              </tr>
            </thead>
            <tbody>
              {rows.map((session) => (
                <tr key={session.id}>
                  <Td>
                    {deviceLabel(session.user_agent)}
                    {session.current && (
                      <span style={{ marginLeft: 6 }}>
                        <Tag tone="accent">this device</Tag>
                      </span>
                    )}
                  </Td>
                  <Td muted>{isoDate(session.created_at)}</Td>
                  <Td muted>{session.current ? "now" : timeAgo(session.last_seen)}</Td>
                  <Td align="right">
                    {!session.current && (
                      <Button
                        variant="ghost"
                        disabled={revoking.busy || readOnly}
                        onClick={() =>
                          revoking.run(
                            () => api.account.revokeSession(session.id),
                            () => sessions.reload(),
                          )
                        }
                      >
                        Revoke
                      </Button>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </Table>
        </div>
      )}
      {revoking.error && (
        <p style={{ margin: "8px 0 0", fontSize: 12, color: "var(--color-accent-700)" }}>
          {revoking.error}
        </p>
      )}
    </div>
  );
}
