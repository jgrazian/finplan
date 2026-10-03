"use client";

import { RecoveryForm } from "./RecoveryForm";
import { useState } from "react";
import { Button, CompactInput, Field } from "@/components/ui";
import { BrandMark, Wordmark } from "@/components/layout/Brand";
import type { Session } from "@/lib/hooks/useSession";

/**
 * Sign in, or register. The server seeds a new account with a starter library
 * of return profiles and a tax table, so registering is enough to start.
 *
 * For a guest (spec 17) registering claims the guest in place, so the plan
 * built so far is the new account's, and signing in offers to bring it along.
 * `embedded` drops the page chrome for use inside a dialog over the workbench.
 */
export function LoginForm({
  session,
  initialMode = "signIn",
  embedded,
}: {
  session: Session;
  initialMode?: "signIn" | "signUp";
  embedded?: boolean;
}) {
  const guest = session.user?.guest === true;
  const [recovering, setRecovering] = useState(false);
  const [mode, setMode] = useState<"signIn" | "signUp">(initialMode);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [passwordConfirmation, setPasswordConfirmation] = useState("");
  const [displayName, setDisplayName] = useState("");

  const registering = mode === "signUp";
  const passwordsMatch = password === passwordConfirmation;

  if (recovering) return <RecoveryForm onBack={() => setRecovering(false)} />;

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (registering) {
          if (!passwordsMatch) return;
          const register = guest ? session.claim : session.signUp;
          register(email, password, passwordConfirmation, displayName || undefined);
        }
        else session.signIn(email, password);
      }}
      style={{
        maxWidth: 360,
        margin: embedded ? 0 : "72px auto",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      }}
    >
      {!embedded && (
        <span className="nav-brand" style={{ margin: 0, fontSize: 24 }}>
          <BrandMark size={26} />
          <Wordmark />
        </span>
      )}
      <h4 style={{ margin: "4px 0 6px" }}>
        {registering ? (guest ? "Create a free account" : "Create an account") : "Sign in"}
      </h4>
      {guest && (
        <p style={{ margin: 0, fontSize: 12, lineHeight: 1.5 }}>
          {registering
            ? "Your guest plan becomes your account's plan."
            : "Already have an account? Sign in, and you can bring your guest plan with you."}
        </p>
      )}

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
          minLength={registering ? 10 : undefined}
          required
        />
      </Field>

      {registering && (
        <>
          <Field label="Confirm password">
            <CompactInput
              type="password"
              autoComplete="new-password"
              value={passwordConfirmation}
              onChange={(e) => setPasswordConfirmation(e.target.value)}
              minLength={10}
              required
            />
          </Field>
          {passwordConfirmation && !passwordsMatch && (
            <p role="alert" style={{ margin: 0, fontSize: 12, color: "var(--color-accent-700)" }}>
              Passwords do not match.
            </p>
          )}
          <Field label="Display name (optional)">
            <CompactInput value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
          </Field>
        </>
      )}

      {session.error && (
        <p style={{ margin: 0, fontSize: 12, color: "var(--color-accent-700)" }}>
          {session.error}
        </p>
      )}

      <Button
        type="submit"
        variant="primary"
        block
        disabled={session.busy || (registering && (!passwordConfirmation || !passwordsMatch))}
      >
        {session.busy ? "…" : registering ? (guest ? "Create account" : "Register") : "Sign in"}
      </Button>
      <Button
        type="button"
        variant="ghost"
        block
        onClick={() => {
          setMode(registering ? "signIn" : "signUp");
          setPasswordConfirmation("");
        }}
      >
        {registering ? "I already have an account" : guest ? "Create a free account" : "Create an account"}
      </Button>
      {!registering && <Button type="button" variant="ghost" onClick={() => setRecovering(true)}>Forgot password?</Button>}
    </form>
  );
}
