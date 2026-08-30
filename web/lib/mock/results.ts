import type {
  AccountSeries,
  NetWorthBands,
  ResultsData,
  YearlyCashFlow,
} from "@/lib/types";

const START_AGE = 45;
const START_YEAR = 2026;
const YEARS = 36;
const START_VALUE = 1.28e6;

/** Account bands in the stacked view: [share at start, share at end]. */
const STACK_SHARES: Array<{ id: string; label: string; color: string; ramp: [number, number] }> = [
  { id: "401k", label: "401(k)", color: "#1d2d3d", ramp: [0.44, 0.16] },
  { id: "brokerage", label: "Brokerage", color: "#41617f", ramp: [0.31, 0.3] },
  { id: "roth-ira", label: "Roth IRA", color: "#749dc4", ramp: [0.14, 0.46] },
  { id: "hysa", label: "Cash / HYSA", color: "#b5d9fd", ramp: [0.11, 0.08] },
];

/**
 * Deterministic stand-in for `monte_carlo_simulate`. Three fixed real-return
 * paths stand in for the P5 / P50 / P95 runs: contributions until the
 * retirement age, inflation-adjusted spending after it, Social Security at 67.
 * Same recurrence as the canvas mock, so the curves match the design.
 */
export function buildBands(
  opts: { retirementAge?: number; annualSpend?: number } = {},
): NetWorthBands {
  const retirementAge = opts.retirementAge ?? 62;
  const annualSpend = opts.annualSpend ?? 145_000;

  const bands: NetWorthBands = { years: [], ages: [], p5: [], p50: [], p95: [] };
  let v5 = START_VALUE;
  let v50 = START_VALUE;
  let v95 = START_VALUE;

  for (let i = 0; i < YEARS; i++) {
    const age = START_AGE + i;
    bands.years.push(START_YEAR + i);
    bands.ages.push(age);
    bands.p5.push(v5);
    bands.p50.push(v50);
    bands.p95.push(v95);

    const contrib = age < retirementAge ? 78_000 : 0;
    const spend =
      age >= retirementAge ? annualSpend * Math.pow(1.025, age - retirementAge) : 0;
    const socialSecurity = age >= 67 ? 44_000 : 0;
    const step = (v: number, r: number) =>
      Math.max(v * (1 + r) + contrib - spend + socialSecurity, 0);

    v5 = step(v5, 0.005);
    v50 = step(v50, 0.048);
    v95 = step(v95, 0.068);
  }
  return bands;
}

/** Splits a net-worth path into per-account bands that ramp over the horizon. */
export function buildAccountSeries(path: number[]): AccountSeries[] {
  return STACK_SHARES.map(({ id, label, color, ramp: [a, b] }) => ({
    accountId: id,
    label,
    color,
    values: path.map((v, i) => {
      const t = path.length > 1 ? i / (path.length - 1) : 0;
      return v * (a + (b - a) * t);
    }),
  }));
}

const CASH_FLOWS: YearlyCashFlow[] = [
  { year: 2043, income: 188_400, expenses: 145_000, taxes: 31_120 },
  { year: 2044, income: 0, expenses: 148_600, taxes: 18_940 },
  { year: 2048, income: 44_000, expenses: 164_000, taxes: 22_310 },
  { year: 2052, income: 48_600, expenses: 181_000, taxes: 29_870 },
];

export function buildResults(
  opts: { retirementAge?: number; annualSpend?: number; iterations?: number } = {},
): ResultsData {
  const bands = buildBands(opts);
  const iterations = opts.iterations ?? 10_000;
  const successRate = 0.914;
  const last = bands.p50.length - 1;

  return {
    stats: {
      numIterations: iterations,
      successRate,
      meanFinalNetWorth: bands.p50[last],
      stdDevFinalNetWorth: (bands.p95[last] - bands.p5[last]) / 3.29,
      minFinalNetWorth: bands.p5[last],
      maxFinalNetWorth: bands.p95[last],
      percentileValues: [
        [5, bands.p5[last]],
        [50, bands.p50[last]],
        [95, bands.p95[last]],
      ],
      converged: true,
      convergedAt: 6_400,
      lifetimeTaxes: 1.12e6,
    },
    bands,
    accountSeries: buildAccountSeries(bands.p50),
    cashFlows: CASH_FLOWS,
    warnings: [
      {
        id: "failed-withdrawal",
        kind: "EffectSkipped",
        title: "Failed withdrawal",
        detail:
          "860 runs could not fund Retirement spending after 2057. Sources exhausted in order Taxable → TaxDeferred → TaxFree.",
      },
      {
        id: "rmd-shortfall",
        kind: "EvaluationFailed",
        title: "RMD shortfall",
        detail:
          "Apply RMD forced $41,800 of taxable income in 12% of runs at age 75.",
      },
    ],
    finalAge: bands.ages[last],
  };
}

export const MOCK_RESULTS = buildResults();
