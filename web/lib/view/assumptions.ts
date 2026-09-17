/**
 * The two assumptions a plan is pointed at — inflation and tax — as rows a
 * picker can show.
 *
 * A name alone ("Moderate", "US Federal 2024 (single)") does not say what
 * choosing it does to the numbers, and these are the two settings that move
 * every figure in the plan at once. So each row carries the rate behind the
 * name as well: the mean the inflation profile draws around, the top marginal
 * and state rates the tax configuration charges.
 */
import type { TaxConfig } from "@/lib/api/types";
import type { AssumptionChoice, InflationProfile } from "@/lib/types";

/** `2.5` → `2.5%`, without a trailing `.0` on the round ones. */
function pct(value: number): string {
  return `${Number(value.toFixed(1))}%`;
}

/**
 * Inflation profiles, already summarised by `toViewInflationProfiles`.
 *
 * Two distribution shapes have no closed-form mean — a bootstrap resamples a
 * real history, a regime model blends two distributions — and rather than
 * fabricate a figure those say where their numbers come from instead.
 */
export function toInflationChoices(profiles: InflationProfile[]): AssumptionChoice[] {
  return profiles.map((profile) => ({
    id: profile.serverId,
    name: profile.id,
    detail:
      profile.mean != null
        ? `${pct(profile.mean)}/yr`
        : profile.kind === "Bootstrap"
          ? "sampled history"
          : "varies by year",
    note: profile.note,
  }));
}

/**
 * Tax configurations. The detail is the top federal bracket and the flat state
 * rate: the top bracket because it is what the plan's largest withdrawals are
 * charged at, the state rate because it is the figure that most often differs
 * between two otherwise identical configurations.
 */
export function toTaxChoices(configs: TaxConfig[]): AssumptionChoice[] {
  return configs.map((config) => {
    const top = config.federal_brackets.reduce((max, b) => Math.max(max, b.rate), 0);
    return {
      id: config.id,
      name: config.name,
      detail: `top ${pct(top * 100)} · state ${pct(config.state_rate * 100)}`,
      note:
        config.description ??
        `${pct(config.capital_gains_rate * 100)} long-term gains, ${pct(
          config.early_withdrawal_penalty_rate * 100,
        )} early-withdrawal penalty.`,
    };
  });
}
