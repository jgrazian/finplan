import type {
  CSSProperties,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
} from "react";
import { cx } from "./cx";

/**
 * Label + control pair. Any control can be the child. The label may be
 * omitted where the surrounding heading already names the control, which
 * keeps the field's spacing without emitting an empty <label>.
 */
export function Field({
  label,
  children,
  className,
  style,
}: {
  label?: ReactNode;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <div className={cx("field", className)} style={style}>
      {label != null && <label>{label}</label>}
      {children}
    </div>
  );
}

export function Input({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cx("input", className)} {...rest} />;
}

export function Select({
  className,
  children,
  ...rest
}: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select className={cx("input", className)} {...rest}>
      {children}
    </select>
  );
}

/** Compact input used inside the inspector drawer, where rows are 32px. */
export function CompactInput({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <Input className={className} style={{ minHeight: 32 }} {...rest} />;
}

/** A labelled slider whose label carries the live value. */
export function RangeField({
  label,
  value,
  onValueChange,
  min,
  max,
  step = 1,
  className,
}: {
  label: ReactNode;
  value: number;
  onValueChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  className?: string;
}) {
  return (
    <Field label={label} className={className}>
      <input
        className="input range"
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onValueChange(Number(e.target.value))}
      />
    </Field>
  );
}
