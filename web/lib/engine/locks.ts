/**
 * Cross-tab coordination (spec 19, "Local store"): Web Locks around writes, and
 * a `BroadcastChannel` announcing them.
 *
 * IndexedDB already orders conflicting transactions across tabs, so a single
 * read-modify-write is safe on its own; the locks cover what spans several
 * transactions (a run's life, the library write that follows plans) and keep
 * two tabs from interleaving them. Without `navigator.locks` (old browsers,
 * Node) work simply runs, as it would for one tab.
 */

interface LockManagerLike {
  request<T>(name: string, callback: () => Promise<T>): Promise<T>;
}

function manager(): LockManagerLike | undefined {
  const nav = (globalThis as { navigator?: { locks?: LockManagerLike } }).navigator;
  return nav?.locks;
}

export const planLockName = (planId: number) => `finplan-plan-${planId}`;
export const LIBRARY_LOCK = "finplan-library";

/** Run `fn` holding the exclusive lock `name`. */
export function withLock<T>(name: string, fn: () => Promise<T>): Promise<T> {
  const locks = manager();
  return locks ? locks.request(name, fn) : fn();
}

interface LockOptions {
  ifAvailable?: boolean;
}

/**
 * Run `fn` holding the lock `name` only if nobody holds it now. Resolves true
 * when it ran, false when another tab had the lock. Without Web Locks nobody
 * can be holding it.
 */
export async function tryLock(name: string, fn: () => Promise<void>): Promise<boolean> {
  const locks = manager() as
    | { request<T>(name: string, options: LockOptions, callback: (lock: unknown) => Promise<T>): Promise<T> }
    | undefined;
  if (!locks) {
    await fn();
    return true;
  }
  return locks.request(name, { ifAvailable: true }, async (lock) => {
    if (!lock) return false;
    await fn();
    return true;
  });
}

/**
 * Run `fn` holding every lock in `names`, taken in sorted order so two callers
 * that want overlapping sets cannot each hold half of them.
 */
export function withLocks<T>(names: readonly string[], fn: () => Promise<T>): Promise<T> {
  const ordered = [...new Set(names)].sort();
  const take = (index: number): Promise<T> =>
    index >= ordered.length ? fn() : withLock(ordered[index], () => take(index + 1));
  return take(0);
}

export const CHANNEL_NAME = "finplan-local";

/** What a tab announces after a write: the plan that changed, or null when the set of plans or the library did. */
export interface ChangeMessage {
  /** Which store worker sent it, so a listener can tell its own from another tab's. */
  origin: string;
  plan: number | null;
}

export interface ChangeChannel {
  announce(plan: number | null): void;
  /** Called for changes made by other tabs only. Returns unsubscribe. */
  onExternal(listener: (plan: number | null) => void): () => void;
  close(): void;
}

/** The channel over `BroadcastChannel`, or a silent one where it does not exist. */
export function openChangeChannel(origin: string): ChangeChannel {
  const Channel = (globalThis as { BroadcastChannel?: typeof BroadcastChannel }).BroadcastChannel;
  if (!Channel) {
    return { announce() {}, onExternal: () => () => undefined, close() {} };
  }
  const channel = new Channel(CHANNEL_NAME);
  const listeners = new Set<(plan: number | null) => void>();
  channel.onmessage = (event: MessageEvent) => {
    const message = event.data as Partial<ChangeMessage> | null;
    if (!message || message.origin === origin) return;
    const plan = typeof message.plan === "number" ? message.plan : null;
    for (const listener of [...listeners]) listener(plan);
  };
  return {
    announce(plan) {
      channel.postMessage({ origin, plan } satisfies ChangeMessage);
    },
    onExternal(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    close() {
      channel.close();
    },
  };
}
