import type { ReactNode } from "react";
import { cx } from "./cx";

/** Small uppercase label used above every figure in the system. */
export function StatLabel({ children }: { children: ReactNode }) {
  return <div className="stat-l">{children}</div>;
}

/** Label over a display-size figure — the rail's vocabulary. */
export function Stat({
  label,
  value,
  className,
}: {
  label: ReactNode;
  value: ReactNode;
  className?: string;
}) {
  return (
    <div className={className}>
      <StatLabel>{label}</StatLabel>
      <div className="stat-v">{value}</div>
    </div>
  );
}

/** Label over a 17px figure — the chart readout's vocabulary. */
export function InlineStat({
  label,
  value,
  emphasis,
}: {
  label: ReactNode;
  value: ReactNode;
  /** Tints the value with the accent, marking the median among percentiles. */
  emphasis?: boolean;
}) {
  return (
    <div>
      <StatLabel>{label}</StatLabel>
      <div
        className={cx("font-[var(--font-heading)] font-semibold text-[17px]")}
        style={emphasis ? { color: "var(--color-accent-800)" } : undefined}
      >
        {value}
      </div>
    </div>
  );
}
