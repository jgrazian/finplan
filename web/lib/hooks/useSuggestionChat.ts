"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import type { SuggestionThread } from "@/lib/api/suggestions";
import { settled } from "@/lib/view/chat";

export interface SuggestionChatState {
  thread: SuggestionThread | undefined;
  /** The thread could not be read. */
  loadError: string | undefined;
  /** A message is on its way, before the server says the turn is running. */
  sending: boolean;
  /** The last send was refused or never arrived; the draft is kept. */
  sendError: string | undefined;
  /** Resolves true when the server took the message. */
  send: (message: string) => Promise<boolean>;
}

/** How often a thread whose reply is being written is read again. */
const POLL_MS = 2_500;

/**
 * The follow-up thread about one note, read when it is opened and polled
 * while a reply is being written. `onSettled` runs when a turn finishes, so
 * the board can reload and show any suggestion the reply added.
 */
export function useSuggestionChat(
  suggestionId: number,
  open: boolean,
  onSettled: () => void,
): SuggestionChatState {
  const [loaded, setLoaded] = useState<{ id?: number; thread?: SuggestionThread; error?: string }>({});
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string>();
  const settle = useRef(onSettled);
  useEffect(() => {
    settle.current = onSettled;
  }, [onSettled]);

  // The last thread read for this note, to tell when a read ends a turn.
  const last = useRef<{ id: number; thread: SuggestionThread } | undefined>(undefined);

  // Take a fresh read, and say so when it ends a turn. Called from settled
  // requests, never while rendering.
  const take = useCallback(
    (thread: SuggestionThread) => {
      const before = last.current?.id === suggestionId ? last.current.thread : undefined;
      last.current = { id: suggestionId, thread };
      setLoaded({ id: suggestionId, thread });
      if (settled(before, thread)) settle.current();
    },
    [suggestionId],
  );

  useEffect(() => {
    if (!open) return;
    let live = true;
    api.suggestions.chat.get(suggestionId).then(
      (thread) => live && take(thread),
      (error: Error) => live && setLoaded({ id: suggestionId, error: error.message }),
    );
    return () => {
      live = false;
    };
  }, [open, suggestionId, take]);

  const current = loaded.id === suggestionId ? loaded : {};
  const running = open && current.thread?.status === "running";
  useEffect(() => {
    if (!running) return;
    let live = true;
    const timer = setInterval(() => {
      api.suggestions.chat.get(suggestionId).then(
        (thread) => live && take(thread),
        // A failed poll is retried on the next tick; the thread keeps what it has.
        () => undefined,
      );
    }, POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [running, suggestionId, take]);

  const send = useCallback(
    async (message: string) => {
      setSending(true);
      setSendError(undefined);
      try {
        take(await api.suggestions.chat.send(suggestionId, message));
        return true;
      } catch (error) {
        setSendError(error instanceof Error ? error.message : String(error));
        return false;
      } finally {
        setSending(false);
      }
    },
    [suggestionId, take],
  );

  return { thread: current.thread, loadError: current.error, sending, sendError, send };
}
