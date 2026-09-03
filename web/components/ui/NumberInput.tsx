"use client";

import {
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { cx } from "./cx";

/**
 * A number field whose punctuation is furniture rather than text.
 *
 * `$`, thousands separators and a trailing unit all read as part of the
 * figure, but none of them are things anyone should have to type, delete or
 * step over. So the affixes are static spans flanking a bare input, the
 * separators are re-applied on every keystroke, and the caret is put back
 * where the digits — not the commas — say it belongs.
 */
interface NumberInputBase {
  /** Static text before the digits, e.g. `$`. */
  prefix?: ReactNode;
  /** Static text after the digits, e.g. `years`. */
  suffix?: ReactNode;
  /** Decimals shown when idle, and the cap while typing. Unset = as typed. */
  decimals?: number;
  /** Thousands separators in the integer part. */
  group?: boolean;
  allowNegative?: boolean;
  min?: number;
  max?: number;
  placeholder?: string;
  readOnly?: boolean;
  disabled?: boolean;
  id?: string;
  name?: string;
  className?: string;
  style?: CSSProperties;
  "aria-label"?: string;
}

/**
 * A field that can hold nothing at all — an absent contribution limit — is a
 * different contract from one that always holds a figure, so the two are
 * separate branches rather than one `number | null` everybody has to check.
 */
export type NumberInputProps =
  | (NumberInputBase & {
      nullable?: false;
      value: number;
      /** Fires on every keystroke, with the value as typed so far. */
      onValueChange?: (value: number) => void;
      /** Fires on blur and Enter, with the settled value, only when it changed. */
      onCommit?: (value: number) => void;
    })
  | (NumberInputBase & {
      /** An emptied field means `null` — not zero. */
      nullable: true;
      value: number | null;
      onValueChange?: (value: number | null) => void;
      onCommit?: (value: number | null) => void;
    });

/** The digits the field holds, with the punctuation stripped back out. */
interface Digits {
  negative: boolean;
  int: string;
  /** Undefined when no decimal point has been typed — `12` is not `12.`. */
  frac?: string;
}

const SIGNIFICANT = /[0-9.]/;

function digitsOf(text: string, decimals: number | undefined, allowNegative: boolean): Digits {
  const negative = allowNegative && text.trimStart().startsWith("-");
  const kept = text.replace(/[^0-9.]/g, "");
  const point = kept.indexOf(".");
  const int = (point < 0 ? kept : kept.slice(0, point)).replace(/^0+(?=\d)/, "");
  let frac = point < 0 ? undefined : kept.slice(point + 1).replace(/\./g, "");
  if (decimals === 0) frac = undefined;
  else if (decimals != null && frac != null) frac = frac.slice(0, decimals);
  return { negative, int, frac };
}

/** Back to display text, regrouped, keeping a half-typed `12.` intact. */
function textOf(d: Digits, group: boolean): string {
  const int = group ? d.int.replace(/\B(?=(\d{3})+(?!\d))/g, ",") : d.int;
  return (d.negative ? "-" : "") + int + (d.frac != null ? "." + d.frac : "");
}

/** Nothing typed at all — distinct from a typed zero. */
function isEmpty(d: Digits): boolean {
  return d.int === "" && d.frac == null;
}

function numberOf(d: Digits): number {
  const n = Number(`${d.negative ? "-" : ""}${d.int || "0"}.${d.frac || "0"}`);
  return Number.isFinite(n) ? n : 0;
}

function format(value: number | null, decimals: number | undefined, group: boolean): string {
  if (value == null || !Number.isFinite(value)) return "";
  return value.toLocaleString("en-US", {
    minimumFractionDigits: decimals ?? 0,
    maximumFractionDigits: decimals ?? 6,
    useGrouping: group,
  });
}

/** Digits and points left of the caret — what regrouping must not move. */
function significantBefore(text: string, caret: number): number {
  let n = 0;
  for (let i = 0; i < caret && i < text.length; i++) if (SIGNIFICANT.test(text[i])) n++;
  return n;
}

/** The offset just past the `count`-th digit, in the regrouped text. */
function caretAfter(text: string, count: number): number {
  if (count <= 0) return 0;
  let seen = 0;
  for (let i = 0; i < text.length; i++) {
    if (SIGNIFICANT.test(text[i]) && ++seen === count) return i + 1;
  }
  return text.length;
}

export function NumberInput(props: NumberInputProps) {
  const {
    value,
    nullable,
    prefix,
    suffix,
    decimals,
    group = true,
    allowNegative = false,
    min,
    max,
    placeholder,
    readOnly,
    disabled,
    id,
    name,
    className,
    style,
    "aria-label": ariaLabel,
  } = props;

  // The two branches of the union differ only in whether `null` is on the
  // wire; inside, everything speaks the wider type.
  const emit = (v: number | null) => {
    (props.onValueChange as ((value: number | null) => void) | undefined)?.(v);
  };
  const emitCommit = (v: number | null) => {
    (props.onCommit as ((value: number | null) => void) | undefined)?.(v);
  };
  /** What an empty field stands for. */
  const blank = nullable ? null : 0;

  const expected = format(value, decimals, group);
  const [text, setText] = useState(expected);
  const [synced, setSynced] = useState(expected);
  const [editing, setEditing] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const caret = useRef<number | null>(null);

  // Follow the prop whenever the field is not being typed into, so a reload
  // puts the stored figure back. Comparing the formatted strings rather than
  // the numbers means a reload landing on the same figure — the common case
  // right after a save — leaves the field, and the caret, alone.
  if (expected !== synced) {
    setSynced(expected);
    if (!editing) setText(expected);
  }

  // Regrouping rewrites the value under the caret; React has just re-rendered
  // it to the end of the text, so put it back.
  useLayoutEffect(() => {
    const at = caret.current;
    caret.current = null;
    if (at != null) input.current?.setSelectionRange(at, at);
  }, [text]);

  const commit = () => {
    const parts = digitsOf(text, decimals, allowNegative);
    let settled = isEmpty(parts) ? blank : numberOf(parts);
    if (settled != null) {
      if (min != null && settled < min) settled = min;
      if (max != null && settled > max) settled = max;
    }
    setText(format(settled, decimals, group));
    if (settled !== value) {
      emit(settled);
      emitCommit(settled);
    }
  };

  const empty = text === "";
  const shown = empty ? (placeholder ?? "0") : text;
  // A nullable field standing empty reads as its placeholder — "none" — and a
  // `$` in front of that says nothing true.
  const affixes = !empty || !nullable;

  return (
    <div
      className={cx("input numfield", className)}
      style={style}
      data-disabled={disabled ? "true" : undefined}
      // Clicking the affixes or the padding is aiming at the number.
      onMouseDown={(e) => {
        if (e.target === input.current || disabled) return;
        e.preventDefault();
        const el = input.current;
        el?.focus();
        el?.setSelectionRange(el.value.length, el.value.length);
      }}
    >
      {prefix != null && affixes && <span className="numfield-prefix">{prefix}</span>}
      <span className="numfield-grow">
        {/* Sizes the input to its own text, so the suffix sits against the
            number instead of at the far edge of the well. */}
        <span className="numfield-mirror" aria-hidden="true">
          {shown}
        </span>
        <input
          ref={input}
          id={id}
          name={name}
          aria-label={ariaLabel}
          inputMode={decimals === 0 ? "numeric" : "decimal"}
          value={text}
          placeholder={placeholder}
          readOnly={readOnly}
          disabled={disabled}
          onFocus={() => setEditing(true)}
          onBlur={() => {
            setEditing(false);
            commit();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
          }}
          onChange={(e) => {
            const el = e.currentTarget;
            const before = significantBefore(el.value, el.selectionStart ?? el.value.length);
            const parts = digitsOf(el.value, decimals, allowNegative);
            const next = textOf(parts, group);
            caret.current = caretAfter(next, before);
            setText(next);
            emit(isEmpty(parts) ? blank : numberOf(parts));
          }}
        />
      </span>
      {suffix != null && affixes && <span className="numfield-suffix">{suffix}</span>}
    </div>
  );
}

/** `412.5 Units` — the unit is a label on the field, not text to type. */
export function UnitInput({ unit, ...rest }: NumberInputProps & { unit: ReactNode }) {
  return <NumberInput {...rest} suffix={unit} />;
}

/** `$12,480.00` — the sign and the separators are not editable. */
export function CurrencyInput(props: NumberInputProps) {
  return <NumberInput decimals={2} {...props} prefix="$" />;
}

/** `9.9%` — a rate as typed, with the sign available for a bear regime. */
export function PercentInput(props: NumberInputProps) {
  return <NumberInput decimals={1} group={false} allowNegative {...props} suffix="%" />;
}
