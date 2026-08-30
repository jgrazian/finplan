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
