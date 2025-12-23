use std::fmt::Debug;

use jiff::ToSpan;
use rand::{SeedableRng, distr::Distribution};

fn main() {
    println!("Hello, world!");
}

pub enum AccountType {
    Taxable,
    TaxDeferred, // 401k, Traditional IRA
    TaxFree,     // Roth IRA
    Liability,   // Mortgages, loans
}

pub struct Account {
    pub name: String,
    pub balance: f64,
    pub account_type: AccountType,
    pub cash_flows: Vec<CashFlow>,
}

pub enum RepeatInterval {
    Never,
    Weekly,
    BiWeekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl RepeatInterval {
    pub fn span(&self) -> jiff::Span {
        match self {
            RepeatInterval::Never => 0.days(),
            RepeatInterval::Weekly => 1.week(),
            RepeatInterval::BiWeekly => 2.weeks(),
            RepeatInterval::Monthly => 1.month(),
            RepeatInterval::Quarterly => 3.months(),
            RepeatInterval::Yearly => 1.year(),
        }
    }
}

pub struct CashFlow {
    pub description: String,
    pub amount: f64,
    pub start: jiff::civil::Date,
    pub end: Option<jiff::civil::Date>, // None = continues indefinitely
    pub repeats: RepeatInterval,
    pub adjust_for_inflation: bool,
}

#[derive(Debug)]
pub struct CashFlowEvent {
    pub amount: f64,
    pub date: jiff::civil::Date,
}

pub struct Event {
    pub description: String,
    pub date: jiff::civil::Date,
}

pub enum InflationProfile {
    None,
    Fixed(f64),
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
}

impl InflationProfile {
    pub const US_HISTORICAL_FIXED: InflationProfile = InflationProfile::Fixed(0.035432);
    pub const US_HISTORICAL_NORMAL: InflationProfile = InflationProfile::Normal {
        mean: 0.035432,
        std_dev: 0.027807,
    };
    pub const US_HISTORICAL_LOG_NORMAL: InflationProfile = InflationProfile::LogNormal {
        mean: 0.035432,
        std_dev: 0.026317,
    };

    pub fn sample(&self, rng: &mut rand::rngs::SmallRng) -> f64 {
        match self {
            InflationProfile::None => 0.0,
            InflationProfile::Fixed(rate) => *rate,
            InflationProfile::Normal { mean, std_dev } => rand_distr::Normal::new(*mean, *std_dev)
                .unwrap()
                .sample(rng),
            InflationProfile::LogNormal { mean, std_dev } => {
                rand_distr::LogNormal::new(*mean, *std_dev)
                    .unwrap()
                    .sample(rng)
            }
        }
    }
}

pub enum ReturnProfile {
    None,
    Fixed(f64),
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
}

impl ReturnProfile {
    pub const SP_500_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.095668);
    pub const SP_500_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.095668,
        std_dev: 0.165234,
    };
    pub const SP_500_HISTORICAL_LOG_NORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.079088,
        std_dev: 0.161832,
    };

    pub fn sample(&self, rng: &mut rand::rngs::SmallRng) -> f64 {
        match self {
            ReturnProfile::None => 0.0,
            ReturnProfile::Fixed(rate) => *rate,
            ReturnProfile::Normal { mean, std_dev } => rand_distr::Normal::new(*mean, *std_dev)
                .unwrap()
                .sample(rng),
            ReturnProfile::LogNormal { mean, std_dev } => {
                rand_distr::LogNormal::new(*mean, *std_dev)
                    .unwrap()
                    .sample(rng)
            }
        }
    }
}

pub struct SimulationParameters {
    pub duration_years: usize,
    pub inflation_profile: InflationProfile,
    pub return_profile: ReturnProfile,
    pub events: Vec<Event>,
    pub accounts: Vec<Account>,
}

pub struct AccountResult {
    pub name: String,
    pub balance: Vec<f64>,
}

pub struct SimulationResult {
    pub inflation: Vec<f64>,
    pub returns: Vec<f64>,
    pub accounts: Vec<f64>,
}

pub fn n_day_return_rate(yearly_return_rate: f64, n_days: f64) -> f64 {
    (1.0 + yearly_return_rate).powf(n_days / 365.0) - 1.0
}

pub fn simulate(params: SimulationParameters, seed: u64) -> SimulationResult {
    let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);

    let mut inflation = Vec::with_capacity(params.duration_years);
    let mut returns = Vec::with_capacity(params.duration_years);
    let mut accounts = Vec::with_capacity(params.accounts.len());

    let today = jiff::Zoned::now().date();

    for _ in 0..params.duration_years {
        inflation.push(params.inflation_profile.sample(&mut rng));
        returns.push(params.return_profile.sample(&mut rng));
    }

    for account in params.accounts {
        let mut balance = account.balance;

        let mut all_cash_flow_events = Vec::new();

        for cash_flow in account.cash_flows {
            let cash_flow_amount = cash_flow.amount;
            let cash_flow_start_date = cash_flow.start;
            let cash_flow_end_date = cash_flow
                .end
                .unwrap_or_else(|| today.saturating_add((params.duration_years as i64).years()));

            let cash_flow_events = cash_flow_start_date
                .series(cash_flow.repeats.span())
                .take_while(|&d| d <= cash_flow_end_date)
                .map(|date| CashFlowEvent {
                    amount: cash_flow_amount,
                    date,
                });
            all_cash_flow_events.extend(cash_flow_events);
        }
        // Sort cash flow events by date so we can apply them in order
        all_cash_flow_events.sort_unstable_by(|a, b| a.date.cmp(&b.date));
        let mut cash_flow_events_iter = all_cash_flow_events.iter().peekable();

        let mut current_date = today;
        for (year, return_rate) in today
            .series(1.year())
            .skip(1)
            .take(params.duration_years)
            .zip(returns.iter())
        {
            while let Some(event) = cash_flow_events_iter.peek() {
                if event.date > year {
                    break;
                }

                if event.date == current_date {
                    balance += &event.amount;
                    cash_flow_events_iter.next();
                    continue;
                }

                let last_date = current_date;
                current_date = event.date;
                let date_diff = (current_date - last_date).get_days();
                let daily_return_rate = n_day_return_rate(*return_rate, date_diff as f64);
                balance += &balance * daily_return_rate;

                balance += &event.amount;
                cash_flow_events_iter.next();
            }

            let last_date = current_date;
            current_date = year;
            let date_diff = (current_date - last_date).get_days();
            let daily_return_rate = n_day_return_rate(*return_rate, date_diff as f64);
            balance += &balance * daily_return_rate;
        }
        accounts.push(balance);
    }

    SimulationResult {
        inflation,
        returns,
        accounts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation() {
        let params = SimulationParameters {
            duration_years: 30,
            inflation_profile: InflationProfile::Fixed(0.02),
            return_profile: ReturnProfile::SP_500_HISTORICAL_NORMAL,
            events: vec![],
            accounts: vec![Account {
                name: "Savings".to_string(),
                balance: 10_000.0,
                account_type: AccountType::Taxable,
                cash_flows: vec![CashFlow {
                    amount: 100.0,
                    description: "Monthly contribution".to_string(),
                    start: jiff::Zoned::now().date(),
                    end: None,
                    repeats: RepeatInterval::Monthly,
                }],
            }],
        };

        let result = simulate(params);

        dbg!(&result.accounts);
    }
}
