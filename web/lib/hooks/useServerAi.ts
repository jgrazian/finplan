"use client";

import { useEffect, useState } from "react";
import type { Health } from "@/lib/api/generated/Health";

/** One question per page load: the answer is the server's configuration. */
let asked: Promise<boolean> | undefined;

function serverHasAi(): Promise<boolean> {
  asked ??= fetch("/api/health", { cache: "no-store" })
    .then((response) => (response.ok ? (response.json() as Promise<Partial<Health>>) : null))
    .then((health) => health?.ai === true)
    .catch(() => {
      // Unreached: ask again next time rather than remember a guess.
      asked = undefined;
      return false;
    });
  return asked;
}

/**
 * Whether the server has an AI model, which an account can use to draft and
 * review plans. Read from `/health`, which answers before sign-in, so a
 * visitor can be shown what an account adds. False until it answers.
 */
export function useServerAi(): boolean {
  const [ai, setAi] = useState(false);
  useEffect(() => {
    let live = true;
    void serverHasAi().then((answer) => live && setAi(answer));
    return () => {
      live = false;
    };
  }, []);
  return ai;
}
