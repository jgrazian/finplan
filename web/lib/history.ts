/**
 * Statistics of an observed return history.
 *
 * A bootstrap profile is the one shape with no parameters to read: it resamples
 * a real series, so everything anyone wants to say about it — a mean, a spread,
 * the worst year — is a measurement rather than a setting. Kept here, in neither
 * the view layer nor the drawing one, because both need it: the tables report
 * the figures and the shape draws the years.
 *
 * Returns arrive as fractions and leave as percent, the unit every profile
 * figure is shown in.
 */

export interface HistoryStats {
  mean: number;
  sd: number;
  min: number;
  max: number;
  years: number;
}

export function historyStats(history: readonly number[]): HistoryStats | null {
  if (history.length === 0) return null;
  const pcts = history.map((r) => r * 100);
  const mean = pcts.reduce((a, b) => a + b, 0) / pcts.length;
  // Sample variance: these are a century of observed years, not the population
  // of every year that could have happened.
  const variance =
    pcts.length < 2 ? 0 : pcts.reduce((a, v) => a + (v - mean) ** 2, 0) / (pcts.length - 1);
  return {
    mean,
    sd: Math.sqrt(variance),
    min: Math.min(...pcts),
    max: Math.max(...pcts),
    years: pcts.length,
  };
}

/** Quantiles read straight off the sorted years, interpolating between them. */
export function empiricalQuantiles(
  history: readonly number[],
  qs: number[],
): number[] | null {
  if (history.length === 0) return null;
  const sorted = history.map((r) => r * 100).sort((a, b) => a - b);
  return qs.map((q) => {
    const at = (sorted.length - 1) * q;
    const i = Math.floor(at);
    const next = sorted[Math.min(i + 1, sorted.length - 1)];
    return sorted[i] + (next - sorted[i]) * (at - i);
  });
}
