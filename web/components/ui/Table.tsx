import type { CSSProperties, ReactNode, TdHTMLAttributes, ThHTMLAttributes } from "react";
import { cx } from "./cx";

export function Table({
  children,
  className,
  compact,
  fixed,
  style,
}: {
  children: ReactNode;
  className?: string;
  /** 12px type, for tables nested inside the inspector. */
  compact?: boolean;
  /**
   * Honour the widths the header cells declare. Without it a wide table lets
   * its numbers drift to the far margin, away from the label they belong to.
   */
  fixed?: boolean;
  style?: CSSProperties;
}) {
  return (
    <table
      className={cx("table", className)}
      style={{
        ...(compact ? { fontSize: 12 } : null),
        ...(fixed ? { tableLayout: "fixed" as const } : null),
        ...style,
      }}
    >
      {children}
    </table>
  );
}

export function Th({
  align = "left",
  children,
  style,
  ...rest
}: ThHTMLAttributes<HTMLTableCellElement> & { align?: "left" | "right" }) {
  return (
    <th style={{ textAlign: align, ...style }} {...rest}>
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
