"use client";

import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ClipboardEvent,
  type CSSProperties,
  type FocusEvent,
  type KeyboardEvent,
} from "react";
import { cx } from "./cx";

/** Wheel damping for the year column — see `useDampedWheel`. */
const WHEEL_SCALE = 0.3;
/** A wheel notch reported in lines rather than pixels is worth about a row. */
const LINE_HEIGHT = 28;

/** Both views are built to one height, so the flip can be decided up front. */
const PANEL_HEIGHT = 300;

const YEAR_MIN = 1920;
const YEAR_MAX = 2120;

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

const WEEKDAYS = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/** `YYYY-MM-DD` -> local midnight, or null. Never `new Date(iso)`, which is UTC. */
function parseIso(iso: string | null | undefined): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec((iso ?? "").trim());
  return m ? build(Number(m[1]), Number(m[2]), Number(m[3])) : null;
}

function toIso(d: Date) {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}


/* ── The well's three segments ──────────────────────────────────────────── */

type Part = "month" | "day" | "year";

const ORDER: Part[] = ["month", "day", "year"];
const WIDTH: Record<Part, number> = { month: 2, day: 2, year: 4 };
/** The largest a segment can read as — day is checked against the month later. */
const CEILING: Record<Part, number> = { month: 12, day: 31, year: 9999 };
const HINT: Record<Part, string> = { month: "MM", day: "DD", year: "YYYY" };

/** The digits each segment holds. Empty strings, not zeroes: nothing typed. */
type Parts = Record<Part, string>;

const EMPTY: Parts = { month: "", day: "", year: "" };

function partsOf(iso: string): Parts {
  const d = parseIso(iso);
  if (!d) return EMPTY;
  const pad = (n: number) => String(n).padStart(2, "0");
  return {
    month: pad(d.getMonth() + 1),
    day: pad(d.getDate()),
    year: String(d.getFullYear()),
  };
}

/** The ISO date the three segments spell, or null while they spell nothing. */
function isoOfParts(p: Parts): string | null {
  if (p.year.length !== 4 || p.month === "" || p.day === "") return null;
  const d = build(Number(p.year), Number(p.month), Number(p.day));
  return d ? toIso(d) : null;
}

/**
 * Feb 30 is nobody's intent — the native control clamps it too, rather than
 * refusing the date and leaving the field to be argued with.
 */
function clampParts(p: Parts): Parts {
  const month = Number(p.month);
  if (p.year.length !== 4 || p.month === "" || p.day === "") return p;
  if (month < 1 || month > 12) return p;
  const last = new Date(Number(p.year), month, 0).getDate();
  return Number(p.day) > last ? { ...p, day: String(last).padStart(2, "0") } : p;
}

function isBlank(p: Parts): boolean {
  return ORDER.every((part) => p[part] === "");
}

/** Compares by content, so a re-render with the same date leaves typing alone. */
function keyOf(p: Parts): string {
  return `${p.month}/${p.day}/${p.year}`;
}

/**
 * What someone might paste: `8/31/2026`, `08-31-2026`, or the ISO order the
 * value itself is in. A two-digit year is not a date — a plan runs a century,
 * so guessing which one is meant would be worse than refusing.
 */
function parseText(text: string): Date | null {
  const t = text.trim();
  const iso = /^(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})$/.exec(t);
  if (iso) return build(Number(iso[1]), Number(iso[2]), Number(iso[3]));
  const us = /^(\d{1,2})[-/.](\d{1,2})[-/.](\d{4})$/.exec(t);
  if (us) return build(Number(us[3]), Number(us[1]), Number(us[2]));
  return null;
}

/** A real day, or null — the Date constructor would roll Feb 31 into March. */
function build(year: number, month: number, day: number): Date | null {
  const d = new Date(year, month - 1, day);
  return year >= 100 && d.getMonth() === month - 1 && d.getDate() === day ? d : null;
}

function longDate(d: Date) {
  return `${MONTHS[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()}`;
}

function sameDay(a: Date, b: Date) {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function addDays(d: Date, n: number) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
}

/** Moves a date to another month, keeping the day where that month has one. */
function withYearMonth(d: Date, year: number, month: number) {
  const last = new Date(year, month + 1, 0).getDate();
  return new Date(year, month, Math.min(d.getDate(), last));
}

/** Month arithmetic that clamps rather than rolling forward: Jan 31 -> Feb 28. */
function addMonths(d: Date, n: number) {
  return withYearMonth(d, d.getFullYear(), d.getMonth() + n);
}

/** The 42 cells of a month's grid: six full weeks from the Sunday before it. */
function monthCells(year: number, month: number) {
  const first = new Date(year, month, 1);
  const start = addDays(first, -first.getDay());
  return Array.from({ length: 42 }, (_, i) => addDays(start, i));
}

/**
 * The browser's own month/year list moves a whole screen of years per notch.
 * A wheel event here scrolls the element by a fraction of its delta instead,
 * which needs a non-passive listener — React's onWheel cannot preventDefault.
 */
function useDampedWheel<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      // A column that cannot scroll should not swallow the page's wheel.
      if (el.scrollHeight <= el.clientHeight) return;
      const delta = e.deltaMode === 1 ? e.deltaY * LINE_HEIGHT : e.deltaY;
      e.preventDefault();
      el.scrollTop += delta * WHEEL_SCALE;
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);
  return ref;
}

export interface DateInputProps {
  /** ISO `YYYY-MM-DD`, or `""` for no date. */
  value: string;
  /** The new ISO date, or `""` when the field is cleared. */
  onValueChange: (iso: string) => void;
  /** Stands in for the whole well while it is empty, e.g. "plan start". */
  placeholder?: string;
  readOnly?: boolean;
  disabled?: boolean;
  required?: boolean;
  id?: string;
  name?: string;
  className?: string;
  style?: CSSProperties;
  ariaLabel?: string;
}

/**
 * Date field with an in-app calendar.
 *
 * Neither half of this is the platform's. Every engine ships a date popup that
 * cannot be themed, whose month/year list moves a decade a wheel notch with no
 * way back to the days — and Gecko opens it on a click anywhere in the well.
 *
 * So the well is three digit segments with the slashes as furniture between
 * them (the grammar NumberInput set for `$` and thousands separators): the
 * punctuation is never typed, deleted or stepped over, a full segment carries
 * the caret to the next, and the value on the wire stays ISO.
 */
export function DateInput({
  value,
  onValueChange,
  placeholder,
  readOnly,
  disabled,
  required,
  id,
  name,
  className,
  style,
  ariaLabel,
}: DateInputProps) {
  const root = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const segs = {
    month: useRef<HTMLInputElement>(null),
    day: useRef<HTMLInputElement>(null),
    year: useRef<HTMLInputElement>(null),
  };

  const [open, setOpen] = useState(false);
  const [up, setUp] = useState(false);
  const [view, setView] = useState<"day" | "monthYear">("day");
  /** The day the keyboard is on; also names the month the grid shows. */
  const [cursor, setCursor] = useState(() => new Date());

  const expected = partsOf(value);
  const [parts, setParts] = useState(expected);
  const [synced, setSynced] = useState(keyOf(expected));
  const [editing, setEditing] = useState(false);

  // Follow the prop whenever the well is not being typed into, so a save that
  // comes back — or a pick from the calendar — lands in the segments. Comparing
  // content means a reload onto the same date leaves the caret alone.
  if (keyOf(expected) !== synced) {
    setSynced(keyOf(expected));
    if (!editing) setParts(expected);
  }

  const today = useMemo(() => new Date(), []);
  const selected = parseIso(value);
  const locked = Boolean(readOnly || disabled);

  const focusSeg = (part: Part, select = true) => {
    const el = segs[part].current;
    el?.focus();
    if (select) el?.select();
  };

  const close = useCallback((refocus: Part | null = "month") => {
    setOpen(false);
    setView("day");
    if (refocus) segs[refocus].current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const openPanel = useCallback(
    (focusPanel: boolean) => {
      if (locked) return;
      setCursor(parseIso(value) ?? new Date());
      setView("day");
      // Drop above the field when the panel would run off the bottom of the
      // window — the Plan strip sits high, but a dialog's field can sit low.
      const field = root.current?.getBoundingClientRect();
      setUp(
        field != null &&
          field.bottom + PANEL_HEIGHT > window.innerHeight &&
          field.top > PANEL_HEIGHT,
      );
      setOpen(true);
      if (focusPanel) requestAnimationFrame(() => panel.current?.focus());
    },
    [locked, value],
  );

  /** Sends what the segments spell, once they spell something whole. */
  const emit = (next: Parts) => {
    if (isBlank(next)) {
      if (value !== "") onValueChange("");
      return;
    }
    const iso = isoOfParts(next);
    if (iso == null || iso === value) return;
    onValueChange(iso);
    setCursor(parseIso(iso) ?? cursor);
  };

  const write = (raw: Parts) => {
    const next = clampParts(raw);
    setParts(next);
    emit(next);
  };

  const commitDate = useCallback(
    (d: Date) => {
      setParts(partsOf(toIso(d)));
      setEditing(false);
      onValueChange(toIso(d));
      close();
    },
    [close, onValueChange],
  );

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) close(null);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [close, open]);

  /** A digit typed into one segment. Two of them carry the caret onward. */
  const onSegChange = (part: Part, raw: string) => {
    const digits = raw.replace(/\D/g, "").slice(0, WIDTH[part]);
    const next = { ...parts, [part]: digits };
    const n = Number(digits);
    // Full, or too large to take another digit — 8 can only be August.
    const done = digits.length === WIDTH[part] || (digits !== "" && n * 10 > CEILING[part]);
    if (done && part !== "year") next[part] = digits.padStart(WIDTH[part], "0");
    write(next);
    if (done) {
      const after = ORDER[ORDER.indexOf(part) + 1];
      if (after) focusSeg(after);
    }
  };

  /** Up and down step the segment the caret is in, as the native one does. */
  const stepSeg = (part: Part, by: 1 | -1) => {
    const floor = part === "year" ? YEAR_MIN : 1;
    const ceiling = part === "year" ? YEAR_MAX : CEILING[part];
    // An empty segment starts somewhere worth starting, not at zero.
    const start = part === "year" ? new Date().getFullYear() : by > 0 ? floor : ceiling;
    const from = parts[part] === "" ? null : Number(parts[part]);
    let n = from == null ? start : from + by;
    if (n > ceiling) n = floor;
    if (n < floor) n = ceiling;
    write({ ...parts, [part]: String(n).padStart(WIDTH[part], "0") });
  };

  const onSegKeyDown = (part: Part, e: KeyboardEvent<HTMLInputElement>) => {
    const el = e.currentTarget;
    const at = el.selectionStart ?? 0;
    const before = ORDER[ORDER.indexOf(part) - 1];
    const after = ORDER[ORDER.indexOf(part) + 1];

    switch (e.key) {
      case "Escape":
        if (!open) return;
        e.preventDefault();
        e.stopPropagation();
        close(part);
        return;
      case "ArrowUp":
        e.preventDefault();
        if (!locked) stepSeg(part, 1);
        return;
      case "ArrowDown":
        e.preventDefault();
        if (locked) return;
        // Alt+down is the standard way to ask a field for its picker.
        if (e.altKey) openPanel(true);
        else stepSeg(part, -1);
        return;
      case "ArrowLeft":
        if (at === 0 && before) {
          e.preventDefault();
          focusSeg(before);
        }
        return;
      case "ArrowRight":
        if (at === el.value.length && after) {
          e.preventDefault();
          focusSeg(after);
        }
        return;
      case "Backspace":
        // An empty segment hands the backspace back to the one before it.
        if (parts[part] === "" && before) {
          e.preventDefault();
          focusSeg(before, false);
          write({ ...parts, [before]: parts[before].slice(0, -1) });
        }
        return;
      case "/":
      case "-":
      case ".":
        // The separators are furniture, but typing one is still a way to say
        // "next segment" — it is what the printed date looks like.
        e.preventDefault();
        if (after) focusSeg(after);
        return;
      case "Enter":
        if (open) {
          e.preventDefault();
          close(part);
        }
        return;
    }
  };

  /** A pasted date fills the whole well, whichever order it is written in. */
  const onPaste = (e: ClipboardEvent<HTMLInputElement>) => {
    const text = e.clipboardData.getData("text");
    const d = parseText(text);
    if (!d) return;
    e.preventDefault();
    write(partsOf(toIso(d)));
    focusSeg("year");
  };

  // Leaving the well settles it: a half-typed date is not a date, so the
  // segments fall back to the value they were showing.
  const onWellBlur = (e: FocusEvent<HTMLDivElement>) => {
    if (root.current?.contains(e.relatedTarget)) return;
    setEditing(false);
    if (open) close(null);
    if (isBlank(parts)) {
      if (value !== "") onValueChange("");
      return;
    }
    const settled = clampParts(parts);
    const iso = isoOfParts(settled);
    if (iso == null) setParts(partsOf(value));
    else {
      setParts(settled);
      if (iso !== value) onValueChange(iso);
    }
  };

  /**
   * The grid's keys, live while the panel itself holds focus — which it does
   * when the calendar was asked for by the glyph or Alt+down. Opened by a
   * press in the well, focus stays on the segments and these never fire.
   */
  const onPanelKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = (d: Date) => {
      e.preventDefault();
      setCursor(d);
    };
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        e.stopPropagation();
        if (view === "monthYear") setView("day");
        else close();
        return;
      case "Enter":
      case " ":
        e.preventDefault();
        if (view === "monthYear") setView("day");
        else commitDate(cursor);
        return;
      case "Tab":
        close(null);
        return;
    }
    const months = view === "monthYear";
    switch (e.key) {
      case "ArrowLeft":
        return step(months ? addMonths(cursor, -1) : addDays(cursor, -1));
      case "ArrowRight":
        return step(months ? addMonths(cursor, 1) : addDays(cursor, 1));
      case "ArrowUp":
        return step(months ? addMonths(cursor, -3) : addDays(cursor, -7));
      case "ArrowDown":
        return step(months ? addMonths(cursor, 3) : addDays(cursor, 7));
      case "Home":
        if (months) return;
        return step(addDays(cursor, -cursor.getDay()));
      case "End":
        if (months) return;
        return step(addDays(cursor, 6 - cursor.getDay()));
      case "PageUp":
        return step(addMonths(cursor, months || e.shiftKey ? -12 : -1));
      case "PageDown":
        return step(addMonths(cursor, months || e.shiftKey ? 12 : 1));
    }
  };

  const blank = isBlank(parts);

  return (
    <div ref={root} className={cx("dp", className)} data-open={open ? "true" : undefined}>
      <div
        className={cx("input dp-well", blank && placeholder && "dp-ghosted")}
        style={style}
        data-disabled={disabled ? "true" : undefined}
        role="group"
        aria-label={ariaLabel}
        onFocus={() => setEditing(true)}
        onBlur={onWellBlur}
        // Clicking the slashes or the padding is aiming at a segment.
        onMouseDown={(e) => {
          if (locked || e.target !== e.currentTarget) return;
          e.preventDefault();
          focusSeg(blank ? "month" : "year");
        }}
        // The well opens the calendar from anywhere in it, as it did when the
        // platform's picker was the one it opened. The panel takes no focus,
        // so the segment the press landed in keeps the caret.
        onClick={() => {
          if (!open) openPanel(false);
        }}
      >
        {ORDER.map((part, i) => (
          <Fragment key={part}>
            {i > 0 && (
              <span className="dp-sep" aria-hidden="true">
                /
              </span>
            )}
            <input
              ref={segs[part]}
              id={i === 0 ? id : undefined}
              className={cx("dp-seg", part === "year" && "y")}
              type="text"
              inputMode="numeric"
              autoComplete="off"
              spellCheck={false}
              maxLength={WIDTH[part]}
              placeholder={HINT[part]}
              aria-label={`${ariaLabel ? `${ariaLabel}, ` : ""}${part}`}
              value={parts[part]}
              readOnly={readOnly}
              disabled={disabled}
              onChange={(e) => !locked && onSegChange(part, e.target.value)}
              onKeyDown={(e) => onSegKeyDown(part, e)}
              onPaste={onPaste}
              onFocus={(e) => e.currentTarget.select()}
            />
          </Fragment>
        ))}

        {blank && placeholder && (
          <span className="dp-ghost" aria-hidden="true">
            {placeholder}
          </span>
        )}

        {/* Carries the date's `required` for the form around it: the segments
            are three controls, and any one of them could be filled alone. */}
        {required && (
          <input
            className="dp-mirror"
            tabIndex={-1}
            aria-hidden="true"
            required
            name={name}
            value={value}
            onChange={() => {}}
          />
        )}
        {!required && name && <input type="hidden" name={name} value={value} readOnly />}
      </div>

      <button
        type="button"
        className="dp-glyph"
        tabIndex={-1}
        disabled={locked}
        aria-label={open ? "Close calendar" : "Open calendar"}
        aria-expanded={open}
        // Pointerdown, not click: the outside-press handler runs first on the
        // capture phase, so a click would close and reopen the panel.
        onPointerDown={(e) => {
          e.preventDefault();
          if (open) close(null);
          else openPanel(true);
        }}
      >
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
          <rect x="1.5" y="2.5" width="13" height="12" stroke="currentColor" strokeWidth="1.2" />
          <path d="M1.5 6h13" stroke="currentColor" strokeWidth="1.2" />
          <path d="M5 1v3M11 1v3" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
        </svg>
      </button>

      {open && (
        <div
          ref={panel}
          className="dp-panel"
          data-drop={up ? "up" : "down"}
          role="dialog"
          aria-label="Choose a date"
          tabIndex={-1}
          onKeyDown={onPanelKeyDown}
          // Keeps the caret where it was through a press on the panel, so a
          // mouse pick does not take the well's focus away.
          onMouseDown={(e) => e.preventDefault()}
        >
          {view === "day" ? (
            <DayView
              cursor={cursor}
              selected={selected}
              today={today}
              onCursor={setCursor}
              onPick={commitDate}
              onOpenMonthYear={() => setView("monthYear")}
              onClear={() => {
                setParts(EMPTY);
                onValueChange("");
                close();
              }}
            />
          ) : (
            <MonthYearView cursor={cursor} onCursor={setCursor} onBack={() => setView("day")} />
          )}
        </div>
      )}
    </div>
  );
}

function DayView({
  cursor,
  selected,
  today,
  onCursor,
  onPick,
  onOpenMonthYear,
  onClear,
}: {
  cursor: Date;
  selected: Date | null;
  today: Date;
  onCursor: (d: Date) => void;
  onPick: (d: Date) => void;
  onOpenMonthYear: () => void;
  onClear: () => void;
}) {
  const month = cursor.getMonth();
  const weeks = useMemo(() => {
    const cells = monthCells(cursor.getFullYear(), month);
    return Array.from({ length: 6 }, (_, w) => cells.slice(w * 7, w * 7 + 7));
  }, [cursor, month]);

  return (
    <>
      <div className="dp-head">
        <button
          type="button"
          className="dp-step"
          aria-label="Previous month"
          onClick={() => onCursor(addMonths(cursor, -1))}
        >
          <span className="dp-chev left" aria-hidden="true" />
        </button>
        <button
          type="button"
          className="dp-cap"
          aria-label="Choose month and year"
          onClick={onOpenMonthYear}
        >
          {MONTHS[month]} {cursor.getFullYear()}
          <span className="dp-chev down" aria-hidden="true" />
        </button>
        <button
          type="button"
          className="dp-step"
          aria-label="Next month"
          onClick={() => onCursor(addMonths(cursor, 1))}
        >
          <span className="dp-chev right" aria-hidden="true" />
        </button>
      </div>

      <div className="dp-week" aria-hidden="true">
        {WEEKDAYS.map((d) => (
          <span key={d}>{d}</span>
        ))}
      </div>

      <div className="dp-grid" role="grid">
        {weeks.map((week) => (
          <div className="dp-row" role="row" key={week[0].getTime()}>
            {week.map((d) => {
              const outside = d.getMonth() !== month;
              return (
                <button
                  key={d.getTime()}
                  type="button"
                  role="gridcell"
                  className={cx("dp-day", outside && "out")}
                  aria-label={longDate(d)}
                  aria-current={sameDay(d, today) ? "date" : undefined}
                  aria-selected={selected != null && sameDay(d, selected)}
                  data-cursor={sameDay(d, cursor) ? "true" : undefined}
                  data-today={sameDay(d, today) ? "true" : undefined}
                  onClick={() => onPick(d)}
                >
                  {d.getDate()}
                </button>
              );
            })}
          </div>
        ))}
      </div>

      <div className="dp-foot">
        <button type="button" className="dp-link" onClick={() => onPick(new Date())}>
          Today
        </button>
        <button type="button" className="dp-link" onClick={onClear}>
          Clear
        </button>
      </div>
    </>
  );
}

function MonthYearView({
  cursor,
  onCursor,
  onBack,
}: {
  cursor: Date;
  onCursor: (d: Date) => void;
  onBack: () => void;
}) {
  const years = useMemo(
    () => Array.from({ length: YEAR_MAX - YEAR_MIN + 1 }, (_, i) => YEAR_MIN + i),
    [],
  );
  const list = useDampedWheel<HTMLDivElement>();
  const year = cursor.getFullYear();

  // Open on the current year rather than at 1920.
  useLayoutEffect(() => {
    list.current
      ?.querySelector<HTMLElement>('[data-on="true"]')
      ?.scrollIntoView({ block: "center" });
    // Once, on entry: later picks scroll the list themselves.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <>
      <div className="dp-head">
        {/* The way back the browser's own month/year list never had. */}
        <button type="button" className="dp-back" onClick={onBack}>
          <span className="dp-chev left" aria-hidden="true" />
          Back
        </button>
        <span className="dp-cap dp-cap-static">
          {MONTHS[cursor.getMonth()]} {year}
        </span>
      </div>

      <div className="dp-my">
        <div className="dp-years" ref={list} role="listbox" aria-label="Year">
          {years.map((y) => (
            <button
              key={y}
              type="button"
              className="dp-year"
              role="option"
              aria-selected={y === year}
              data-on={y === year ? "true" : undefined}
              onClick={() => onCursor(withYearMonth(cursor, y, cursor.getMonth()))}
            >
              {y}
            </button>
          ))}
        </div>

        <div className="dp-months" role="listbox" aria-label="Month">
          {MONTHS.map((name, i) => (
            <button
              key={name}
              type="button"
              className="dp-month"
              role="option"
              aria-selected={i === cursor.getMonth()}
              // Picking a month is the other way back: the days it names are
              // what the caller came for.
              onClick={() => {
                onCursor(withYearMonth(cursor, year, i));
                onBack();
              }}
            >
              {name.slice(0, 3)}
            </button>
          ))}
        </div>
      </div>
    </>
  );
}
