"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import type { PlanThread, SuggestionThread } from "@/lib/api/suggestions";
import { type ChatThread, settled } from "@/lib/view/chat";

export interface ChatState<T extends ChatThread> {
  thread: T | undefined;
  /** The thread could not be read. */
  loadError: string | undefined;
  /** A message is on its way, before the server says the turn is running. */
  sending: boolean;
  /** The last send was refused or never arrived; the draft is kept. */
  sendError: string | undefined;
  /** Resolves true when the server took the message. */
  send: (message: string) => Promise<boolean>;
}

export type SuggestionChatState = ChatState<SuggestionThread>;

export interface PlanChatState extends ChatState<PlanThread> {
  /** Start over; resolves true when the thread is gone. */
  clear: () => Promise<boolean>;
  clearing: boolean;
}

/** How often a thread whose reply is being written is read again. */
const POLL_MS = 2_500;

/** How a thread is read and written, by its key. */
interface ThreadIo<T> {
  get: (key: number) => Promise<T>;
  send: (key: number, message: string) => Promise<T>;
}

/**
 * A thread, read when it is opened and polled while a reply is being
 * written. `onSettled` runs when a turn finishes, so the board can reload
 * and show any suggestion the reply added.
 */
function useChatThread<T extends ChatThread>(
  key: number,
  open: boolean,
  onSettled: () => void,
  io: ThreadIo<T>,
) {
  const [loaded, setLoaded] = useState<{ key?: number; thread?: T; error?: string }>({});
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string>();
  const settle = useRef(onSettled);
  useEffect(() => {
    settle.current = onSettled;
  }, [onSettled]);
  // Held by reference so a caller's inline object does not restart the reads.
  const calls = useRef(io);
  useEffect(() => {
    calls.current = io;
  }, [io]);

  // The last thread read for this key, to tell when a read ends a turn.
  const last = useRef<{ key: number; thread: T } | undefined>(undefined);

  // Take a fresh read, and say so when it ends a turn. Called from settled
  // requests, never while rendering.
  const take = useCallback(
    (thread: T) => {
      const before = last.current?.key === key ? last.current.thread : undefined;
      last.current = { key, thread };
      setLoaded({ key, thread });
      if (settled(before, thread)) settle.current();
    },
    [key],
  );

  useEffect(() => {
    if (!open) return;
    let live = true;
    calls.current.get(key).then(
      (thread) => live && take(thread),
      (error: Error) => live && setLoaded({ key, error: error.message }),
    );
    return () => {
      live = false;
    };
  }, [open, key, take]);

  const current = loaded.key === key ? loaded : {};
  const running = open && current.thread?.status === "running";
  useEffect(() => {
    if (!running) return;
    let live = true;
    const timer = setInterval(() => {
      calls.current.get(key).then(
        (thread) => live && take(thread),
        // A failed poll is retried on the next tick; the thread keeps what it has.
        () => undefined,
      );
    }, POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [running, key, take]);

  const send = useCallback(
    async (message: string) => {
      setSending(true);
      setSendError(undefined);
      try {
        take(await calls.current.send(key, message));
        return true;
      } catch (error) {
        setSendError(error instanceof Error ? error.message : String(error));
        return false;
      } finally {
        setSending(false);
      }
    },
    [key, take],
  );

  return { thread: current.thread, loadError: current.error, sending, sendError, send, take, setSendError };
}

const NOTE_IO: ThreadIo<SuggestionThread> = {
  get: (id) => api.suggestions.chat.get(id),
  send: (id, message) => api.suggestions.chat.send(id, message),
};

const PLAN_IO: ThreadIo<PlanThread> = {
  get: (scenarioId) => api.planChat.get(scenarioId),
  send: (scenarioId, message) => api.planChat.send(scenarioId, message),
};

/** The follow-up thread about one note. */
export function useSuggestionChat(
  suggestionId: number,
  open: boolean,
  onSettled: () => void,
): SuggestionChatState {
  const { thread, loadError, sending, sendError, send } = useChatThread(suggestionId, open, onSettled, NOTE_IO);
  return { thread, loadError, sending, sendError, send };
}

/** The plan's own thread on its Review tab (plan chat). */
export function usePlanChat(scenarioId: number, onSettled: () => void): PlanChatState {
  const chat = useChatThread(scenarioId, true, onSettled, PLAN_IO);
  const { take, setSendError } = chat;
  const [clearing, setClearing] = useState(false);
  const clear = useCallback(async () => {
    setClearing(true);
    setSendError(undefined);
    try {
      await api.planChat.clear(scenarioId);
      take({ scenario_id: scenarioId, status: "idle", error: null, messages: [], activity: [] });
      return true;
    } catch (error) {
      setSendError(error instanceof Error ? error.message : String(error));
      return false;
    } finally {
      setClearing(false);
    }
  }, [scenarioId, take, setSendError]);
  return {
    thread: chat.thread,
    loadError: chat.loadError,
    sending: chat.sending,
    sendError: chat.sendError,
    send: chat.send,
    clear,
    clearing,
  };
}
