"use client";

import {
  Fragment,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { cx } from "./cx";

export interface DropdownOption<T extends string | number> {
  value: T;
  label: string;
  /** Right-aligned secondary value, so the menu can be read as a table. */
  detail?: ReactNode;
  /** Section band above the row. Consecutive options sharing a name group. */
  group?: string;
  /**
   * The row runs a command instead of naming a value — "New scenario…" at the
   * foot of the list. It takes the accent, never the selected fill, and stays
   * pinned to the bottom of a menu that scrolls.
   */
  action?: boolean;
  disabled?: boolean;
}

export interface DropdownProps<T extends string | number> {
  options: ReadonlyArray<DropdownOption<T>>;
  value: T | null | undefined;
  onChange: (value: T) => void;
  /** Shown, greyed, when nothing is selected yet. */
  placeholder?: string;
  disabled?: boolean;
  /** Sits in a sentence: shrinks to its value, menu grows to its content. */
  inline?: boolean;
  /** Scrolls the menu past this height rather than growing the page. */
  maxMenuHeight?: number;
  ariaLabel?: string;
  /** Set when a <label> elsewhere names the control. */
  ariaLabelledBy?: string;
  id?: string;
  className?: string;
  style?: CSSProperties;
}

/** Rows a search or an arrow key can land on. */
function isSelectable<T extends string | number>(opt: DropdownOption<T> | undefined) {
  return opt != null && !opt.disabled;
}

/**
 * The select, rebuilt (canvas 11a/11b).
 *
 * A native popup takes the platform's list — no hairlines, no secondary
 * column, no accent fill on the current value — and no amount of CSS reaches
 * inside it. So the trigger is a button and the menu is a listbox, with the
 * keyboard contract the native control had: type-ahead, arrows, Home/End,
 * Enter to commit, Escape to abandon.
 *
 * Focus never leaves the trigger; the active row is named by
 * `aria-activedescendant`, which is what keeps one highlight on screen whether
 * it was the mouse or the arrow keys that moved it.
 */
export function Dropdown<T extends string | number>({
  options,
  value,
  onChange,
  placeholder = "Select…",
  disabled,
  inline,
  maxMenuHeight,
  ariaLabel,
  ariaLabelledBy,
  id,
  className,
  style,
}: DropdownProps<T>) {
  const generatedId = useId();
  const rootId = id ?? generatedId;
  const menuId = `${rootId}-menu`;
  const optionId = (index: number) => `${rootId}-opt-${index}`;

  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);

  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);

  const selected = useMemo(
    () => options.findIndex((o) => o.value === value),
    [options, value],
  );

  /** Consecutive runs of one group name, in the order the caller gave them. */
  const runs = useMemo(() => {
    const out: Array<{ group?: string; items: Array<{ opt: DropdownOption<T>; index: number }> }> =
      [];
    options.forEach((opt, index) => {
      const last = out[out.length - 1];
      if (last && last.group === opt.group) last.items.push({ opt, index });
      else out.push({ group: opt.group, items: [{ opt, index }] });
    });
    return out;
  }, [options]);

  const close = useCallback((refocus = true) => {
    setOpen(false);
    setActive(-1);
    if (refocus) trigger.current?.focus();
  }, []);

  const openMenu = useCallback(
    (from: number = selected) => {
      if (disabled) return;
      setOpen(true);
      setActive(
        isSelectable(options[from])
          ? from
          : options.findIndex((o) => !o.disabled),
      );
    },
    [disabled, options, selected],
  );

  const commit = useCallback(
    (index: number) => {
      const opt = options[index];
      if (!isSelectable(opt)) return;
      onChange(opt.value);
      close();
    },
    [close, onChange, options],
  );

  /** Steps over disabled rows, and stops at the ends rather than wrapping. */
  const move = useCallback(
    (from: number, step: number) => {
      for (let i = from + step; i >= 0 && i < options.length; i += step) {
        if (!options[i].disabled) return i;
      }
      return from;
    },
    [options],
  );

  const edge = useCallback(
    (step: 1 | -1) => {
      const start = step === 1 ? -1 : options.length;
      return move(start, step);
    },
    [move, options.length],
  );

  // A click outside is the other way to abandon the menu. Pointerdown rather
  // than click, so a drag that starts elsewhere closes it too — and focus stays
  // where the pointer went instead of snapping back to the trigger.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) close(false);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [close, open]);

  // Keep the active row in view when the arrow keys walk a scrolling menu.
  useEffect(() => {
    if (!open || active < 0) return;
    menu.current
      ?.querySelector<HTMLElement>(`#${CSS.escape(`${rootId}-opt-${active}`)}`)
      ?.scrollIntoView({ block: "nearest" });
  }, [active, open, rootId]);

  // Type-ahead, as the native control has it: the letters typed within a beat
  // of each other are one query, and a repeated letter cycles the matches.
  const query = useRef({ text: "", at: 0 });
  const typeAhead = useCallback(
    (char: string) => {
      const now = Date.now();
      const text = (now - query.current.at < 800 ? query.current.text : "") + char.toLowerCase();
      query.current = { text, at: now };

      const repeat = text.length > 1 && text.split("").every((c) => c === text[0]);
      const needle = repeat ? text[0] : text;
      const from = open ? Math.max(active, 0) : Math.max(selected, 0);

      for (let step = repeat ? 1 : 0; step <= options.length; step += 1) {
        const i = (from + step) % options.length;
        const opt = options[i];
        if (isSelectable(opt) && opt.label.toLowerCase().startsWith(needle)) {
          if (open) setActive(i);
          else onChange(opt.value);
          return;
        }
      }
    },
    [active, onChange, open, options, selected],
  );

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return;

    if (!open) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        openMenu();
        return;
      }
    } else {
      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setActive((i) => (i < 0 ? edge(1) : move(i, 1)));
          return;
        case "ArrowUp":
          e.preventDefault();
          setActive((i) => (i < 0 ? edge(-1) : move(i, -1)));
          return;
        case "Home":
          e.preventDefault();
          setActive(edge(1));
          return;
        case "End":
          e.preventDefault();
          setActive(edge(-1));
          return;
        case "Enter":
        case " ":
          e.preventDefault();
          commit(active);
          return;
        case "Escape":
          e.preventDefault();
          close();
          return;
        case "Tab":
          // Tab leaves the control; it does not commit the highlight.
          close(false);
          return;
      }
    }

    if (e.key.length === 1 && !e.altKey && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      typeAhead(e.key);
    }
  };

  const current = selected >= 0 ? options[selected] : undefined;

  return (
    <div
      ref={root}
      className={cx("dd", inline && "dd-inline", className)}
      style={style}
      data-open={open ? "true" : undefined}
    >
      <button
        ref={trigger}
        type="button"
        id={rootId}
        className="dd-trigger"
        disabled={disabled}
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        aria-activedescendant={open && active >= 0 ? optionId(active) : undefined}
        aria-label={ariaLabel}
        aria-labelledby={ariaLabelledBy}
        onClick={() => (open ? close() : openMenu())}
        onKeyDown={onKeyDown}
      >
        <span className={cx("dd-value", !current && "dd-empty")}>
          {current ? current.label : placeholder}
        </span>
        <span className="cv" aria-hidden="true" />
      </button>

      {open && (
        <div
          ref={menu}
          id={menuId}
          className="dd-menu"
          role="listbox"
          aria-label={ariaLabel}
          aria-labelledby={ariaLabelledBy}
          style={maxMenuHeight != null ? { maxHeight: maxMenuHeight, overflowY: "auto" } : undefined}
          // The trigger keeps focus while the menu is open, so a press on a row
          // must not take it away — otherwise the button loses its highlight
          // and the click-outside handler races the selection.
          onMouseDown={(e) => e.preventDefault()}
        >
          {runs.map((run, r) => {
            const rows = run.items.map(({ opt, index }) => (
              <Row
                key={opt.value}
                id={optionId(index)}
                option={opt}
                selected={index === selected}
                active={index === active}
                onHover={() => !opt.disabled && setActive(index)}
                onPick={() => commit(index)}
              />
            ));
            // Ungrouped rows stay direct children of the menu, so the first
            // one drops its hairline.
            if (!run.group) return <Fragment key={`run-${r}`}>{rows}</Fragment>;
            return (
              <div className="dd-group" key={`run-${r}`} role="group" aria-label={run.group}>
                <div className="dd-head">{run.group}</div>
                {rows}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function Row<T extends string | number>({
  id,
  option,
  selected,
  active,
  onHover,
  onPick,
}: {
  id: string;
  option: DropdownOption<T>;
  selected: boolean;
  active: boolean;
  onHover: () => void;
  onPick: () => void;
}) {
  // An action names no value, so it never reads as the selection even if the
  // caller's sentinel happens to be the current one.
  const marked = selected && !option.action;
  return (
    <div
      id={id}
      className={cx("dd-opt", option.action && "dd-action", active && "act")}
      role="option"
      aria-selected={marked}
      aria-disabled={option.disabled || undefined}
      onMouseEnter={onHover}
      onClick={() => !option.disabled && onPick()}
    >
      {marked ? (
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
      {option.detail != null && <span className="sub">{option.detail}</span>}
    </div>
  );
}
