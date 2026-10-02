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
  /**
   * Marks the median among percentiles with a dashed rule under it — the
   * same dash as the P50 line it reads off — rather than the accent, which is
   * kept for actions.
   */
  emphasis?: boolean;
}) {
  return (
    <div>
      <StatLabel>{label}</StatLabel>
      <div
        className={cx("font-[var(--font-heading)] font-semibold text-[17px]")}
        style={
          emphasis
            ? {
                textDecoration: "underline dashed",
                textDecorationColor: "var(--color-envelope-p50)",
                textDecorationThickness: 1.5,
                textUnderlineOffset: 5,
              }
            : undefined
        }
      >
        {value}
      </div>
    </div>
  );
}
