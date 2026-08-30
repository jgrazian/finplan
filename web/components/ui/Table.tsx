import type { ReactNode, TdHTMLAttributes, ThHTMLAttributes } from "react";
import { cx } from "./cx";

export function Table({
  children,
  className,
  compact,
}: {
  children: ReactNode;
  className?: string;
  /** 12px type, for tables nested inside the inspector. */
  compact?: boolean;
}) {
  return (
    <table className={cx("table", className)} style={compact ? { fontSize: 12 } : undefined}>
      {children}
    </table>
  );
}

export function Th({
  align = "left",
  children,
  ...rest
}: ThHTMLAttributes<HTMLTableCellElement> & { align?: "left" | "right" }) {
  return (
    <th style={{ textAlign: align }} {...rest}>
      {children}
    </th>
  );
}

export function Td({
  align = "left",
  muted,
  children,
  style,
  ...rest
}: TdHTMLAttributes<HTMLTableCellElement> & {
  align?: "left" | "right";
  muted?: boolean;
}) {
  return (
    <td
      style={{
        textAlign: align,
        ...(muted
          ? { color: "color-mix(in srgb, var(--color-text) 55%, transparent)" }
          : null),
        ...style,
      }}
      {...rest}
    >
      {children}
    </td>
  );
}
