"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { cx } from "@/components/ui";

export interface ColumnOption<K extends string> {
  key: K;
  label: string;
  /** What the column holds, for anyone who cannot tell from the label. */
  note?: string;
  /** Always shown, never listed as something to switch off. */
  fixed?: boolean;
}

/**
 * A remembered set of columns.
 *
 * `lib/nav` puts in the URL what the app is *open onto* — the scenario, the
 * tab, the row — because those are what a bookmark or a pasted link should
 * carry. Which columns someone likes to read is not that: it belongs to the
 * person rather than to the plan, and it would only be noise in a link. So it
 * lives in their browser, under `key`, and every table keeps its own.
 *
 * Read through `useSyncExternalStore` rather than copied into state on mount:
 * storage is genuinely outside React, the server has no access to it, and this
 * is the one hook that will render the defaults into the prerendered markup
 * and the stored choice from the first client render, with no mismatch and no
 * frame of the wrong columns in between.
 *
 * `options` and `defaults` are read on every render, so pass constants.
 */
export function useStoredColumns<K extends string>(
  key: string,
  options: ReadonlyArray<ColumnOption<K>>,
  defaults: readonly K[],
): [ReadonlySet<K>, (next: Set<K>) => void] {
  const fallback = useMemo(() => new Set(defaults), [defaults]);

  // The store is one string; this is the parsed view of it, kept so that an
  // unchanged string hands back the same Set. A fresh one every read would
  // tell React the store had changed, on every render, forever.
  const cache = useRef<{ raw: string | null; value: ReadonlySet<K> }>(undefined);

  const getSnapshot = useCallback((): ReadonlySet<K> => {
    const raw = readStore(key);
    if (!cache.current || cache.current.raw !== raw) {
      cache.current = { raw, value: parseColumns(raw, options) ?? fallback };
    }
    return cache.current.value;
  }, [key, options, fallback]);

  const getServerSnapshot = useCallback(() => fallback, [fallback]);

  const visible = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  const update = useCallback(
    (next: Set<K>) => {
      writeStore(key, JSON.stringify([...next]));
      announce();
    },
    [key],
  );

  return [visible, update];
}

// ── the store ───────────────────────────────────────────────────────────────

const listeners = new Set<() => void>();

/** Our own writes; `storage` only ever fires in the tabs that did not write. */
function announce() {
  for (const listener of listeners) listener();
}

function subscribe(onChange: () => void) {
  listeners.add(onChange);
  // Two tabs open on the same plan should not disagree about the table.
  window.addEventListener("storage", onChange);
  return () => {
    listeners.delete(onChange);
    window.removeEventListener("storage", onChange);
  };
}

/**
 * Storage can refuse outright — a private window, or third-party storage
 * switched off — and a table drawn with its default columns is a perfectly
 * good answer to that, so neither reading nor writing reports failure.
 */
function readStore(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStore(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // The choice still holds for this session, which is the part anyone is
    // actually looking at.
  }
}

/**
 * What was stored, or nothing if it cannot be trusted.
 *
 * Storage is hand-editable and outlives the code that wrote it, so a key that
 * no longer names a column is dropped rather than carried, and the fixed
 * columns are put back whatever it says.
 */
function parseColumns<K extends string>(
  raw: string | null,
  options: ReadonlyArray<ColumnOption<K>>,
): ReadonlySet<K> | undefined {
  if (!raw) return undefined;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return undefined;
  }
  if (!Array.isArray(parsed)) return undefined;

  const known = new Set<string>(options.map((o) => o.key));
  const next = new Set(parsed.filter((k): k is K => typeof k === "string" && known.has(k)));

  // Nothing recognised means the store has lost its meaning — not that someone
  // asked for a table with no columns. Tested before the fixed columns go back
  // in, or every unusable store would read as "just the fixed ones".
  if (next.size === 0) return undefined;

  for (const option of options) {
    if (option.fixed) next.add(option.key);
  }
  return next;
}

/**
 * The cash-flow table's column switch.
 *
 * `Dropdown` commits one value and closes; this menu is a set of independent
 * toggles that stays open while the table rebuilds underneath it, so the whole
 * point — seeing what a column does to the table — is not lost to a reopen on
 * every click.
 */
export function ColumnPicker<K extends string>({
  options,
  visible,
  onChange,
}: {
  options: ReadonlyArray<ColumnOption<K>>;
  visible: ReadonlySet<K>;
  onChange: (next: Set<K>) => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const toggle = useCallback(
    (key: K) => {
      const next = new Set(visible);
      // A table with no columns is not a state worth being able to reach.
      if (next.has(key) && next.size > 1) next.delete(key);
      else next.add(key);
      onChange(next);
    },
    [onChange, visible],
  );

  return (
    <div ref={root} className="dd dd-wide" style={{ width: 170 }} data-open={open ? "true" : undefined}>
      <button
        type="button"
        className="dd-trigger"
        aria-haspopup="true"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span
          style={{
            fontFamily: "var(--font-heading)",
            fontWeight: 600,
            letterSpacing: "0.04em",
            textTransform: "uppercase",
            fontSize: 11,
          }}
        >
          Columns
        </span>
        <span
          style={{
            marginLeft: "auto",
            fontFamily: "ui-monospace, Menlo, monospace",
            fontSize: 11,
            color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
          }}
        >
          {visible.size}/{options.length}
        </span>
        <span className="cv" aria-hidden="true" />
      </button>

      {open && (
        <div className="dd-menu" role="group" aria-label="Columns">
          {options.map((option) => {
            const on = visible.has(option.key);
            return (
              <div
                key={option.key}
                role="checkbox"
                tabIndex={0}
                aria-checked={on}
                aria-disabled={option.fixed || undefined}
                className={cx("dd-opt")}
                onClick={() => !option.fixed && toggle(option.key)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    if (!option.fixed) toggle(option.key);
                  }
                }}
                style={option.fixed ? { opacity: 0.55, cursor: "default" } : undefined}
              >
                {on ? (
                  <svg
                    className="mk"
                    width="12"
                    height="12"
                    viewBox="0 0 16 16"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.8"
                    strokeLinecap="round"
                    aria-hidden="true"
                  >
                    <path d="M3 8.4 6.3 11.7 13 5" />
                  </svg>
                ) : (
                  <span className="mk" aria-hidden="true" />
                )}
                <span className="dd-label">{option.label}</span>
                {option.note && <span className="sub">{option.note}</span>}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
