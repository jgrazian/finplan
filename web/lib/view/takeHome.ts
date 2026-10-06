import type { TaxConfig } from "../api/types";

/** A first year of working income, as guided setup's take-home line shows it. */
export interface TakeHome {
  gross: number;
  /** Pre-tax 401(k) deferral, out of gross before tax. */
  deferral: number;
  federal: number;
  state: number;
  /** What lands in checking: gross − deferral − federal − state. */
  net: number;
  spending: number;
  /** net − spending; negative is a yearly shortfall. */
  surplus: number;
}

/**
 * Federal tax on `income` the way the engine taxes a year's ordinary income:
 * the standard deduction as a 0% band, then the progressive brackets
 * (`finplan_core::taxes::calculate_federal_tax` over `brackets_with_deduction`).
 */
export function federalTax(income: number, config: TaxConfig): number {
  const deduction = config.standard_deduction;
  const brackets = [...config.federal_brackets].sort((a, b) => a.threshold - b.threshold);
  let tax = 0;
  brackets.forEach((bracket, i) => {
    const lower = bracket.threshold + deduction;
    const upper = i + 1 < brackets.length ? brackets[i + 1].threshold + deduction : Infinity;
    tax += Math.max(0, Math.min(income, upper) - lower) * bracket.rate;
  });
  return tax;
}

/**
 * The first plan year's salary after the 401(k) deferral and income tax,
 * against that year's spending. Mirrors the salary template: the deferral is
 * pre-tax, the rest is taxable income with a flat state rate on top. Payroll
 * (FICA) tax is not modeled by the engine, so it is not here either.
 */
export function takeHome(
  gross: number,
  deferral: number,
  spending: number,
  config: TaxConfig | undefined,
): TakeHome {
  const taxable = Math.max(0, gross - deferral);
  const federal = config ? federalTax(taxable, config) : 0;
  const state = config ? taxable * config.state_rate : 0;
  const net = taxable - federal - state;
  return { gross, deferral, federal, state, net, spending, surplus: net - spending };
}
