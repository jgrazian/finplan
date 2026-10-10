//! Dynamic spending: rules that decide each year's draw from the market.
//!
//! A `Spending` effect (see [`super::EventEffect::Spending`]) decides one
//! number per calendar year, the draw `W`, at the event's first occurrence in
//! the year, and pays `W / n` at each of the year's `n` occurrences. The rule
//! remembers its last decision in real terms ([`SpendingRuleState`]), which is
//! what an amount expression cannot do: guardrails step *last year's* draw.
//!
//! Every dollar figure on a rule (an initial amount, a floor, a ceiling) is in
//! plan-start dollars; the draw it produces is nominal.

use serde::{Deserialize, Serialize};

use super::ids::AccountId;

/// How the first year's draw is set.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SpendingInitial {
    /// A yearly amount in plan-start dollars.
    Amount(f64),
    /// A share of the portfolio at the first occurrence.
    Rate(f64),
}

/// How a `Spending` effect decides each year's draw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SpendingRule {
    /// `rate × P`, clamped to `floor` and `ceiling`.
    PercentOfPortfolio {
        rate: f64,
        #[serde(default)]
        floor: Option<f64>,
        #[serde(default)]
        ceiling: Option<f64>,
    },
    /// Guyton-Klinger: last year's draw carried forward for inflation, cut by
    /// `step` when the withdrawal rate rises `band` above where it started,
    /// raised by `step` when it falls `band` below.
    Guardrails {
        initial: SpendingInitial,
        band: f64,
        step: f64,
        /// Keep last year's nominal draw after a losing year when the raise
        /// would put the rate above where it started (never made up).
        skip_inflation_after_loss: bool,
        /// No capital-preservation cut in the plan's last this-many years.
        protect_final_years: u8,
        #[serde(default)]
        floor: Option<f64>,
        #[serde(default)]
        ceiling: Option<f64>,
    },
    /// A target of `rate × P`, approached by at most `max_cut` down or
    /// `max_raise` up in real terms each year.
    CappedChange {
        rate: f64,
        max_cut: f64,
        max_raise: f64,
    },
    /// Variable percentage withdrawal: the payment that spends `P` down to
    /// nothing by `final_age` at `real_return`.
    Vpw { real_return: f64, final_age: u8 },
}

/// The accounts whose balance a rule reads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum SpendingPortfolio {
    /// Every Bank and Investment account: everything but Property and Liability.
    #[default]
    Liquid,
    /// Exactly these accounts.
    Accounts(Vec<AccountId>),
}

/// A rule's memory, per effect and per path. Real figures are in plan-start
/// dollars, deflated by the path's inflation on the day they were decided.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpendingRuleState {
    /// `W₀ / P₀`, set the first year.
    pub initial_rate: f64,
    /// `W₀` in plan-start dollars.
    pub initial_real: f64,
    /// The last decided `W` in plan-start dollars.
    pub last_real: f64,
    /// The last decided `W`, nominal.
    pub last_nominal: f64,
    /// The year `last_*` was decided for.
    pub last_year: i16,
}

/// What a rule reads when it decides a year's draw.
#[derive(Debug, Clone, Copy)]
pub struct SpendingInputs {
    /// The portfolio's balance now (`P`).
    pub portfolio: f64,
    /// The portfolio's market return over the previous calendar year (`r`),
    /// `None` in the first year or when it held nothing on January 1.
    pub last_return: Option<f64>,
    /// The path's cumulative inflation since the plan started, today.
    pub inflation: f64,
    /// Age in whole years today.
    pub age: u8,
    /// Calendar years between this one and the plan's end.
    pub years_left: i32,
    pub year: i16,
}

/// `r / (1 − (1 + r)^−n)`: the level payment that spends 1 over `n` periods
/// at return `r`. `pmt(0, n) = 1 / n`.
#[must_use]
pub fn pmt(rate: f64, periods: u32) -> f64 {
    let n = f64::from(periods.max(1));
    if rate.abs() < 1e-12 {
        return 1.0 / n;
    }
    rate / (1.0 - (1.0 + rate).powf(-n))
}

impl SpendingRule {
    /// Decide the year's draw (nominal, at least zero) and the rule's state
    /// after it. `prev` is `None` in the rule's first year.
    #[must_use]
    pub fn decide(
        &self,
        prev: Option<&SpendingRuleState>,
        x: &SpendingInputs,
    ) -> (f64, SpendingRuleState) {
        let p = x.portfolio.max(0.0);
        let f = if x.inflation.is_finite() && x.inflation > 0.0 {
            x.inflation
        } else {
            1.0
        };
        let rate_of = |w: f64| {
            if p > 0.0 {
                w / p
            } else if w > 0.0 {
                f64::INFINITY
            } else {
                0.0
            }
        };

        let (w, floor, ceiling) = match self {
            SpendingRule::PercentOfPortfolio {
                rate,
                floor,
                ceiling,
            } => (rate * p, *floor, *ceiling),
            SpendingRule::Guardrails {
                initial,
                band,
                step,
                skip_inflation_after_loss,
                protect_final_years,
                floor,
                ceiling,
            } => {
                let w = match prev {
                    None => match initial {
                        SpendingInitial::Amount(amount) => amount * f,
                        SpendingInitial::Rate(rate) => rate * p,
                    },
                    Some(s) => {
                        // 1. Inflation, unless the year lost money and the
                        //    raise would lift the rate above where it began.
                        let mut w = s.last_real * f;
                        if *skip_inflation_after_loss
                            && x.last_return.is_some_and(|r| r < 0.0)
                            && rate_of(w) > s.initial_rate
                        {
                            w = s.last_nominal;
                        }
                        // 2-3. One guardrail at most.
                        let protected = x.years_left < i32::from(*protect_final_years);
                        if !protected && rate_of(w) > s.initial_rate * (1.0 + band) {
                            w *= 1.0 - step;
                        } else if rate_of(w) < s.initial_rate * (1.0 - band) {
                            w *= 1.0 + step;
                        }
                        w
                    }
                };
                (w, *floor, *ceiling)
            }
            SpendingRule::CappedChange {
                rate,
                max_cut,
                max_raise,
            } => {
                let target = rate * p;
                let w = match prev {
                    None => target,
                    Some(s) => {
                        let lo = s.last_real * (1.0 - max_cut);
                        let hi = s.last_real * (1.0 + max_raise);
                        (target / f).clamp(lo.min(hi), hi.max(lo)) * f
                    }
                };
                (w, None, None)
            }
            SpendingRule::Vpw {
                real_return,
                final_age,
            } => {
                let years_left = u32::from(final_age.saturating_sub(x.age)).max(1);
                (p * pmt(*real_return, years_left), None, None)
            }
        };

        // 4. Clamps, in plan-start dollars.
        let mut w = if w.is_finite() { w.max(0.0) } else { 0.0 };
        if let Some(floor) = floor {
            w = w.max(floor * f);
        }
        if let Some(ceiling) = ceiling {
            w = w.min(ceiling * f);
        }
        let real = w / f;

        let next = match prev {
            None => SpendingRuleState {
                initial_rate: rate_of(w),
                initial_real: real,
                last_real: real,
                last_nominal: w,
                last_year: x.year,
            },
            Some(s) => SpendingRuleState {
                last_real: real,
                last_nominal: w,
                last_year: x.year,
                ..*s
            },
        };
        (w, next)
    }
}

/// How a path's draws went, when the plan has a `Spending` effect. Draws from
/// several effects in one year are summed; figures are plan-start dollars.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpendingDiagnostics {
    /// Year one's draw.
    pub first_real: f64,
    pub lowest_real: f64,
    /// Largest real year-over-year fall, as a fraction of the year before.
    pub largest_cut: f64,
    /// Years whose real draw was below 90% of `first_real`.
    pub years_below_first: u16,
    /// Every draw paid, summed.
    pub lifetime_real: f64,
}

impl SpendingDiagnostics {
    /// Fold a path's yearly decided draws (`(year, real draw)`, ascending)
    /// and paid draws into its diagnostics.
    #[must_use]
    pub fn from_draws(decided: &[(i16, f64)], lifetime_real: f64) -> Option<Self> {
        let (_, first_real) = *decided.first()?;
        let lowest_real = decided
            .iter()
            .map(|(_, w)| *w)
            .fold(f64::INFINITY, f64::min);
        let largest_cut = decided
            .windows(2)
            .filter(|pair| pair[0].1 > 0.0)
            .map(|pair| (pair[0].1 - pair[1].1) / pair[0].1)
            .fold(0.0_f64, f64::max);
        let years_below_first = decided
            .iter()
            .filter(|(_, w)| *w < 0.9 * first_real)
            .count() as u16;
        Some(Self {
            first_real,
            lowest_real,
            largest_cut,
            years_below_first,
            lifetime_real,
        })
    }
}

/// Spending outcomes across paths, from each path's [`SpendingDiagnostics`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendingStats {
    /// Median of `lifetime_real`.
    pub median_lifetime_spending: f64,
    /// 10th percentile of `lowest_real`: "1 in 10 markets, your draw dips to".
    pub p10_lowest_draw: f64,
    /// Share of paths whose `largest_cut` exceeds 10%.
    pub cut_rate: f64,
    /// Median of `years_below_first`.
    pub median_years_below_first: f64,
    /// Median of `first_real`.
    #[serde(default)]
    pub median_first_draw: f64,
}

/// Mergeable per-batch state behind [`SpendingStats`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpendingAccumulator {
    first: Vec<f64>,
    lifetime: Vec<f64>,
    lowest: Vec<f64>,
    largest_cut: Vec<f64>,
    years_below_first: Vec<f64>,
}

impl SpendingAccumulator {
    pub fn add(&mut self, diagnostics: &SpendingDiagnostics) {
        self.first.push(diagnostics.first_real);
        self.lifetime.push(diagnostics.lifetime_real);
        self.lowest.push(diagnostics.lowest_real);
        self.largest_cut.push(diagnostics.largest_cut);
        self.years_below_first
            .push(f64::from(diagnostics.years_below_first));
    }

    pub fn merge(&mut self, other: Self) {
        self.first.extend(other.first);
        self.lifetime.extend(other.lifetime);
        self.lowest.extend(other.lowest);
        self.largest_cut.extend(other.largest_cut);
        self.years_below_first.extend(other.years_below_first);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lifetime.is_empty()
    }

    /// `None` when no path drew anything.
    #[must_use]
    pub fn finish(mut self) -> Option<SpendingStats> {
        if self.lifetime.is_empty() {
            return None;
        }
        let cuts = self.largest_cut.iter().filter(|c| **c > 0.10).count();
        Some(SpendingStats {
            median_lifetime_spending: sorted_quantile(&mut self.lifetime, 0.5),
            p10_lowest_draw: sorted_quantile(&mut self.lowest, 0.10),
            cut_rate: cuts as f64 / self.largest_cut.len() as f64,
            median_years_below_first: sorted_quantile(&mut self.years_below_first, 0.5),
            median_first_draw: sorted_quantile(&mut self.first, 0.5),
        })
    }
}

/// Type-7 quantile of `values` (sorted in place).
fn sorted_quantile(values: &mut [f64], p: f64) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    let h = (values.len() - 1) as f64 * p;
    let (lo, hi) = (values[h.floor() as usize], values[h.ceil() as usize]);
    if lo == hi {
        return lo;
    }
    (lo + (hi - lo) * h.fract()).clamp(lo, hi)
}

/// Real total spending by year across paths: the same quantiles as real net
/// worth, one point per calendar year of the plan, dated where net worth's
/// grid closes that year (December 31, or the plan's end date for a final
/// part year).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealSpendingSummary {
    pub num_iterations: usize,
    pub points: Vec<super::RealQuantilePoint>,
}
