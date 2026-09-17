"use client";

import { RecoveryForm } from "./RecoveryForm";
import { useState } from "react";
import { Button, CompactInput, Field } from "@/components/ui";
import type { Session } from "@/lib/hooks/useSession";

/**
 * Sign in, or register. The server seeds a new account with a starter library
 * of return profiles and a tax table, so registering is enough to start.
 */
export function LoginForm({ session }: { session: Session }) {
  const [recovering, setRecovering] = useState(false);
  const [mode, setMode] = useState<"signIn" | "signUp">("signIn");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");

  const registering = mode === "signUp";

  if (recovering) return <RecoveryForm onBack={() => setRecovering(false)} />;

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (registering) session.signUp(email, password, displayName || undefined);
        else session.signIn(email, password);
      }}
      style={{
        maxWidth: 360,
        margin: "72px auto",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      }}
    >
      <span className="nav-brand" style={{ margin: 0 }}>
        FINPLAN
      </span>
      <h4 style={{ margin: "4px 0 6px" }}>
        {registering ? "Create an account" : "Sign in"}
      </h4>

      <Field label="Email">
        <CompactInput
          type="email"
          autoComplete="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
        />
      </Field>

      <Field label={registering ? "Password — at least 10 characters" : "Password"}>
        <CompactInput
          type="password"
          autoComplete={registering ? "new-password" : "current-password"}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
      </Field>

      {registering && (
        <Field label="Display name (optional)">
          <CompactInput value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
        </Field>
      )}

      {session.error && (
        <p style={{ margin: 0, fontSize: 12, color: "var(--color-accent-700)" }}>
          {session.error}
        </p>
      )}

      <Button type="submit" variant="primary" block disabled={session.busy}>
        {session.busy ? "…" : registering ? "Register" : "Sign in"}
      </Button>
      <Button
        type="button"
        variant="ghost"
        block
        onClick={() => setMode(registering ? "signIn" : "signUp")}
      >
        {registering ? "I already have an account" : "Create an account"}
      </Button>
      {!registering && <Button type="button" variant="ghost" onClick={() => setRecovering(true)}>Forgot password?</Button>}
    </form>
  );
}
