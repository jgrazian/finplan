/** Compact currency, matching the canvas's `fmt()` exactly. */
export function fmtCompact(v: number): string {
  if (v >= 1e6) return "$" + (v / 1e6).toFixed(v < 1e7 ? 2 : 1) + "M";
  if (v >= 1e3) return "$" + Math.round(v / 1e3) + "k";
  return "$0";
}

/** Full currency with thousands separators, e.g. `$188,400`. */
export function fmtCurrency(v: number): string {
  const sign = v < 0 ? "−" : "";
  return sign + "$" + Math.round(Math.abs(v)).toLocaleString("en-US");
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

/**
 * `$563k`, falling back to the exact figure under the rounding floor —
 * `fmtCompact` reads everything below $1,000 as `$0`, which is fine on a chart
 * axis and wrong beside a bar labelling a real balance.
 */
export function fmtCompactOrExact(v: number): string {
  const abs = Math.abs(v);
  if (abs < 1000) return fmtCurrency(v);
  return (v < 0 ? "\u2212" : "") + fmtCompact(abs);
}

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
 * a log axis floor can sit far below `fmtCompact`'s rounding floor.
 */
export function fmtAxis(v: number): string {
  const abs = Math.abs(v);
  if (abs < 1000) return fmtCurrency(v);
  const sign = v < 0 ? "\u2212" : "";
  const [scaled, unit] = abs >= 1e6 ? [abs / 1e6, "M"] : [abs / 1e3, "k"];
  const digits = scaled.toLocaleString("en-US", {
    maximumFractionDigits: scaled < 10 ? 2 : 0,
  });
  return `${sign}$${digits}${unit}`;
}
