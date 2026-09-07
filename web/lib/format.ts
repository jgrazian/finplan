/** Compact currency without hiding debt or small nonzero balances. */
export function fmtCompact(v: number): string {
  if (!Number.isFinite(v)) return "—";
  const abs = Math.abs(v);
  const sign = v < 0 ? "−" : "";
  if (abs >= 1e6) return sign + "$" + (abs / 1e6).toFixed(abs < 1e7 ? 2 : 1) + "M";
  if (abs >= 1e3) return sign + "$" + Math.round(abs / 1e3) + "k";
  return fmtCurrency(v);
}

/** Full currency with thousands separators, e.g. `$188,400`. */
export function fmtCurrency(v: number): string {
  if (!Number.isFinite(v)) return "—";
  const abs = Math.abs(v);
  const sign = v < 0 ? "−" : "";
  // Even a sub-dollar balance is not zero. Keep cents and identify sub-cent amounts.
  if (abs > 0 && abs < 1) return sign + (abs < 0.01 ? "<$0.01" : "$" + abs.toFixed(2));
  return sign + "$" + Math.round(abs).toLocaleString("en-US");
}

export function fmtPercent(fraction: number, digits = 1): string {
  return (fraction * 100).toFixed(digits) + "%";
}

export function fmtInt(v: number): string {
  return Math.round(v).toLocaleString("en-US");
}

export function fmtShare(fraction: number): string {
  return Math.round(fraction * 100) + "%";
}

/** Share units, always to one decimal — `412.5 sh`, `901.0 sh`. */
export function fmtUnits(v: number): string {
  return v.toLocaleString("en-US", {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });
}

/** Compatibility name for compact currency with small balances preserved. */
export const fmtCompactOrExact = fmtCompact;

/** One decimal, for a share sitting next to the bar that draws it — `8.9%`. */
export function fmtShareFine(fraction: number): string {
  return (fraction * 100).toFixed(1) + "%";
}

/** `14:06` — the clock every "saved at" line in the app quotes. */
export function fmtClock(at: number): string {
  return new Date(at).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

/**
 * A value-axis tick: `$2M`, `$1.5M`, `$245k`, `$40`. Trailing zeros are dropped
 * — a log axis labels round numbers, and `$2.00M` reads as a measurement rather
 * than as the gridline it marks. Under $1,000 the exact figure is kept, because
 * a log axis floor can sit far below the compact thousands unit.
 */
export function fmtAxis(v: number): string {
  if (!Number.isFinite(v)) return "—";
  const abs = Math.abs(v);
  if (abs < 1000) return fmtCurrency(v);
  const sign = v < 0 ? "\u2212" : "";
  const [scaled, unit] = abs >= 1e6 ? [abs / 1e6, "M"] : [abs / 1e3, "k"];
  const digits = scaled.toLocaleString("en-US", {
    maximumFractionDigits: scaled < 10 ? 2 : 0,
  });
  return `${sign}$${digits}${unit}`;
}
