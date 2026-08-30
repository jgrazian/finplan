/** Normal PDF, unnormalised — only the shape is drawn. */
function pdf(z: number): number {
  return Math.exp(-0.5 * z * z);
}

export interface CurveGeometry {
  w: number;
  h: number;
  /** Distance from the box edge to the baseline and sides. */
  pad: number;
}

export const CURVE_GEOMETRY: CurveGeometry = { w: 300, h: 76, pad: 8 };

/**
 * Bell curve for a profile's annual return distribution, spanning ±3σ across
 * the box. A zero-σ (Fixed) profile has no dispersion to draw, so it becomes
 * a single spike at the mean.
 */
export function distributionPath(sd: number, geo: CurveGeometry = CURVE_GEOMETRY): string {
  const { w, h, pad } = geo;
  const cx = w / 2;
  const baseline = h - pad;

  if (sd === 0) return `M${cx.toFixed(1)} ${baseline} L${cx.toFixed(1)} 10`;

  const points: string[] = [];
  for (let k = 0; k <= 60; k++) {
    const z = -3 + k * 0.1;
    const x = cx + (z / 3) * (w / 2 - pad);
    const y = baseline - pdf(z) * (h - 20);
    points.push(`${k ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`);
  }
  return `${points.join(" ")} L${w - pad} ${baseline} L${pad} ${baseline} Z`;
}

/**
 * Percent label, one decimal — the convention across every profile figure.
 * A resampled or regime-switching distribution has no closed-form mean, and a
 * dash is the honest reading of that.
 */
export function pct(v: number | null): string {
  return v == null ? "—" : `${v.toFixed(1)}%`;
}

/** Normal-approximation quantiles around the mean. */
export function quantiles(
  mean: number | null,
  sd: number | null,
): { p5: string; p95: string } {
  if (mean == null || sd == null) return { p5: "—", p95: "—" };
  return {
    p5: pct(mean - 1.645 * sd),
    p95: pct(mean + 1.645 * sd),
  };
}
