"use client";
import { useState } from "react";
import { Button, CompactInput, Field } from "@/components/ui";
import { http } from "@/lib/api/http";
import type { UserResponse } from "@/lib/api/types";

export function VerificationPanel({ user, onSaved, readOnly }: { user: UserResponse; onSaved: (user: UserResponse) => void; readOnly?: boolean }) {
  const [token, setToken] = useState("");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  async function run(verify: boolean) {
    setBusy(true); setMessage("");
    try {
      if (verify) {
        await http.post<void>("/auth/verify-email", { token: token.trim() });
        onSaved(await http.get<UserResponse>("/auth/me")); setToken(""); setMessage("Email verified.");
      } else {
        await http.post<void>("/auth/request-verification", { email: user.email });
        setMessage("If this account exists, verification instructions have been sent. Paste the token from your email below. Tokens expire in 30 minutes.");
      }
    } catch (error) { setMessage(error instanceof Error ? error.message : "Verification failed."); }
    finally { setBusy(false); }
  }
  return <section style={{ marginTop: 24 }}>
    <h4>Email verification</h4>
    {user.email_verified_at ? <p>{user.email} is verified.</p> : <>
      <p>Verify ownership of {user.email}.</p>
      <Button disabled={busy || readOnly} onClick={() => run(false)}>Request verification</Button>
      <Field label="Verification token from your email"><CompactInput value={token} onChange={(e) => setToken(e.target.value)} autoComplete="off" spellCheck={false} autoCapitalize="none" /></Field>
      <Button disabled={busy || readOnly || !token.trim()} onClick={() => run(true)}>Verify email</Button>
    </>}
    {message && <p role="status">{message}</p>}
  </section>;
}
