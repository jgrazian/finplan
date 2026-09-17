"use client";
import { useState } from "react";
import { Button, CompactInput, Field } from "@/components/ui";
import { http } from "@/lib/api/http";

export function RecoveryForm({ onBack }: { onBack: () => void }) {
  const [email, setEmail] = useState("");
  const [token, setToken] = useState("");
  const [password, setPassword] = useState("");
  const [resetting, setResetting] = useState(false);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  return <form style={{ maxWidth: 400, margin: "72px auto", display: "grid", gap: 12 }} onSubmit={async (event) => {
    event.preventDefault(); setBusy(true); setMessage("");
    try {
      if (resetting) {
        await http.post<void>("/auth/reset-password", { token, new_password: password });
        setPassword(""); setToken(""); setMessage("Password reset. All previous sessions have ended. Return to sign in.");
      } else {
        await http.post<void>("/auth/forgot-password", { email });
        setMessage("If that account exists, recovery instructions have been sent. The token expires after 30 minutes.");
      }
    } catch (error) { setMessage(error instanceof Error ? error.message : "Recovery request failed."); }
    finally { setBusy(false); }
  }}>
    <h3>Recover your account</h3>
    {resetting ? <>
      <Field label="Recovery token"><CompactInput value={token} onChange={(e) => setToken(e.target.value)} required autoComplete="off" /></Field>
      <Field label="New password — at least 10 characters"><CompactInput type="password" value={password} onChange={(e) => setPassword(e.target.value)} minLength={10} required autoComplete="new-password" /></Field>
    </> : <Field label="Account email"><CompactInput type="email" value={email} onChange={(e) => setEmail(e.target.value)} required autoComplete="email" /></Field>}
    {message && <p role="status">{message}</p>}
    <Button type="submit" variant="primary" disabled={busy}>{busy ? "Working…" : resetting ? "Reset password" : "Request recovery"}</Button>
    <Button type="button" onClick={() => { setResetting(!resetting); setMessage(""); }}>{resetting ? "Request a new token" : "I have a recovery token"}</Button>
    <Button type="button" variant="ghost" onClick={onBack}>Back to sign in</Button>
  </form>;
}
