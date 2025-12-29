use std::collections::HashMap;
use std::fmt::Debug;

use jiff::ToSpan;
use rand::{SeedableRng, distr::Distribution};

fn main() {
    println!("Hello, world!");
}

#[derive(Debug, Clone)]
pub enum AccountType {
    Taxable,
    TaxDeferred, // 401k, Traditional IRA
    TaxFree,     // Roth IRA
    Liability,   // Mortgages, loans
}

#[derive(Debug, Clone)]
pub struct Account {
    pub account_id: u64,
    pub name: String,
    pub initial_balance: f64,
    pub account_type: AccountType,
    pub return_profile: ReturnProfile,
    pub cash_flows: Vec<CashFlow>,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum Timepoint {
    Immediate,
    /// A specific fixed date (ad-hoc)
    Date(jiff::civil::Date),
    /// Reference to a named event in SimulationParameters
    Event(String),
    Never,
}

#[derive(Debug, Clone)]
pub struct CashFlow {
    pub cash_flow_id: u64,
    pub description: Option<String>,
    pub amount: f64,
    pub start: Timepoint,
    pub end: Timepoint,
    pub repeats: RepeatInterval,
    pub cash_flow_limits: Option<CashFlowLimits>,
    pub adjust_for_inflation: bool,
}

#[derive(Debug)]
pub struct CashFlowEvent {
    pub cash_flow_id: u64,
    pub amount: f64,
    pub date: jiff::civil::Date,
}

#[derive(Debug, Clone)]
pub enum LimitPeriod {
    /// Resets every calendar
    Yearly,
    /// Never resets
    Lifetime,
}

#[derive(Debug, Clone)]
pub struct CashFlowLimits {
    pub limit: f64,
    pub limit_period: LimitPeriod,
}

pub struct Event {
    pub name: String,
    pub trigger: EventTrigger,
}

pub enum EventTrigger {
    Date(jiff::civil::Date),
    AccountBalance {
        account_id: u64,
        threshold: f64,
        above: bool, // true = trigger when balance > threshold, false = balance < threshold
    },
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

#[derive(Debug, Clone)]
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
    pub start_date: Option<jiff::civil::Date>,
    pub duration_years: usize,
    pub inflation_profile: InflationProfile,
    pub events: Vec<Event>,
    pub accounts: Vec<Account>,
}

#[derive(Debug)]
pub struct SimulationResult {
    pub yearly_inflation: Vec<f64>,
    pub account_histories: Vec<AccountHistory>,
}

#[derive(Debug)]
pub struct AccountHistory {
    pub account_id: u64,
    pub yearly_returns: Vec<f64>,
    pub values: Vec<AccountSnapshot>,
}

#[derive(Debug, Clone)]
pub struct AccountSnapshot {
    pub date: jiff::civil::Date,
    pub balance: f64,
}

pub fn n_day_rate(yearly_rate: f64, n_days: f64) -> f64 {
    (1.0 + yearly_rate).powf(n_days / 365.0) - 1.0
}

/// Internal state for a running cash flow
struct ActiveCashFlowState {
    account_index: usize,
    cash_flow_index: usize,
    next_date: Option<jiff::civil::Date>,
    period_accumulated: f64,
    last_period_key: i16,
}

pub fn simulate(params: &SimulationParameters, seed: u64) -> SimulationResult {
    let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
    let start_date = params
        .start_date
        .unwrap_or_else(|| jiff::Zoned::now().date());
    let end_date = start_date.saturating_add((params.duration_years as i64).years());

    // 1. Initialize State
    let mut triggered_events: HashMap<String, jiff::civil::Date> = HashMap::new();

    let mut histories: Vec<AccountHistory> = params
        .accounts
        .iter()
        .map(|a| AccountHistory {
            account_id: a.account_id,
            yearly_returns: (0..params.duration_years)
                .map(|_| a.return_profile.sample(&mut rng))
                .collect(),
            values: vec![AccountSnapshot {
                date: start_date,
                balance: a.initial_balance,
            }],
        })
        .collect();

    // Pre-calculate market conditions
    let inflation_rates = (0..params.duration_years)
        .map(|_| params.inflation_profile.sample(&mut rng))
        .collect::<Vec<_>>();

    let mut cumulative_inflation = Vec::with_capacity(params.duration_years + 1);
    cumulative_inflation.push(1.0);
    for r in &inflation_rates {
        cumulative_inflation.push(cumulative_inflation.last().unwrap() * (1.0 + r));
    }

    // Initialize CashFlow States
    // We need to track when each cashflow happens next.
    let mut active_cash_flows: Vec<ActiveCashFlowState> = Vec::new();

    // Helper to resolve start date
    let resolve_start = |tp: &Timepoint,
                         now: jiff::civil::Date,
                         triggers: &HashMap<String, jiff::civil::Date>|
     -> Option<jiff::civil::Date> {
        match tp {
            Timepoint::Immediate => Some(now),
            Timepoint::Date(d) => Some(*d),
            Timepoint::Event(name) => triggers.get(name).copied(),
            Timepoint::Never => None,
        }
    };

    // Initial population of active cash flows
    for (acc_idx, acc) in params.accounts.iter().enumerate() {
        for (cf_idx, cf) in acc.cash_flows.iter().enumerate() {
            let start = resolve_start(&cf.start, start_date, &triggered_events);
            if let Some(d) = start {
                active_cash_flows.push(ActiveCashFlowState {
                    account_index: acc_idx,
                    cash_flow_index: cf_idx,
                    next_date: Some(d), // First occurrence
                    period_accumulated: 0.0,
                    last_period_key: start_date.year(),
                });
            } else {
                // It depends on an event that hasn't happened yet.
                // We track it, but next_date is None.
                active_cash_flows.push(ActiveCashFlowState {
                    account_index: acc_idx,
                    cash_flow_index: cf_idx,
                    next_date: None,
                    period_accumulated: 0.0,
                    last_period_key: start_date.year(),
                });
            }
        }
    }

    let mut current_date = start_date;

    // EVENT LOOP
    while current_date < end_date {
        // 1. Apply CashFlows & Check Triggers
        // We loop until no new triggers/cashflows happen at this instant
        let mut something_happened = true;
        while something_happened {
            something_happened = false;

            // A. Apply CashFlows
            for acf in &mut active_cash_flows {
                let Some(date) = acf.next_date else {
                    continue;
                };

                if date > current_date {
                    continue;
                }

                let acc = &params.accounts[acf.account_index];
                let cf = &acc.cash_flows[acf.cash_flow_index];
                let histories_acc = &mut histories[acf.account_index];

                // Check if we passed the end date
                let end_date_opt = resolve_start(&cf.end, start_date, &triggered_events);
                let has_ended = if let Some(end) = end_date_opt {
                    current_date >= end
                } else {
                    false
                };

                if !has_ended {
                    // Apply Cash Flow
                    let mut amount = cf.amount;
                    if cf.adjust_for_inflation {
                        let years_passed = (current_date - start_date).get_days() as f64 / 365.0;
                        let year_idx =
                            (years_passed.floor() as usize).min(params.duration_years - 1);
                        let fraction = years_passed - (year_idx as f64);
                        let inflation_multiplier = cumulative_inflation[year_idx]
                            * (1.0 + inflation_rates[year_idx]).powf(fraction);
                        amount *= inflation_multiplier;
                    }

                    if let Some(limits) = &cf.cash_flow_limits {
                        let current_year = current_date.year();
                        let period_key = match limits.limit_period {
                            LimitPeriod::Yearly => current_year,
                            LimitPeriod::Lifetime => 0,
                        };

                        if period_key != acf.last_period_key {
                            acf.period_accumulated = 0.0;
                            acf.last_period_key = period_key;
                        }

                        let magnitude = amount.abs();
                        let remaining = limits.limit - acf.period_accumulated;
                        let allowed_magnitude = magnitude.min(remaining.max(0.0));

                        if allowed_magnitude < magnitude {
                            amount = amount.signum() * allowed_magnitude;
                        }

                        acf.period_accumulated += allowed_magnitude;
                    }

                    // Update account balance
                    let last_balance = histories_acc
                        .values
                        .last_mut()
                        .expect("account must have at least 1 balance.");
                    last_balance.balance += amount;

                    // Schedule next occurrence
                    match &cf.repeats {
                        RepeatInterval::Never => acf.next_date = None,
                        interval => {
                            let next = date.saturating_add(interval.span());
                            acf.next_date = Some(next);
                        }
                    }
                    something_happened = true;
                } else {
                    acf.next_date = None;
                }
            }

            // B. Check Triggers (Balance & Date)
            let mut new_triggers = Vec::new();
            for event in &params.events {
                if triggered_events.contains_key(&event.name) {
                    continue;
                }

                let triggered = match event.trigger {
                    EventTrigger::Date(d) => current_date >= d,
                    EventTrigger::AccountBalance {
                        account_id,
                        threshold,
                        above,
                    } => {
                        if let Some(acc) = histories.iter().find(|a| a.account_id == account_id) {
                            if above {
                                acc.values.last().unwrap().balance >= threshold
                            } else {
                                acc.values.last().unwrap().balance <= threshold
                            }
                        } else {
                            false
                        }
                    }
                };

                if triggered {
                    new_triggers.push(event.name.clone());
                }
            }

            // Activate triggers
            if !new_triggers.is_empty() {
                something_happened = true;
                for name in new_triggers {
                    triggered_events.insert(name.clone(), current_date);

                    // Wake up cashflows waiting for this event
                    for acf in &mut active_cash_flows {
                        let cf =
                            &params.accounts[acf.account_index].cash_flows[acf.cash_flow_index];

                        if let Timepoint::Event(ref event_name) = cf.start
                            && event_name == &name
                        {
                            acf.next_date = Some(current_date);
                        }
                    }
                }
            }
        }

        // 2. Find the next checkpoint date
        let mut next_checkpoint = end_date;

        // Check CashFlows
        for acf in &active_cash_flows {
            if let Some(d) = acf.next_date
                && d > current_date
                && d < next_checkpoint
            {
                next_checkpoint = d;
            }
        }

        // Check Fixed Date Events
        for event in &params.events {
            if !triggered_events.contains_key(&event.name)
                && let EventTrigger::Date(d) = event.trigger
                && d > current_date
                && d < next_checkpoint
            {
                next_checkpoint = d;
            }
        }

        // Heartbeat
        let heartbeat = current_date.saturating_add(1.month());
        if heartbeat < next_checkpoint {
            next_checkpoint = heartbeat;
        }

        // 3. Advance Time (Apply Interest)
        let days_passed = (next_checkpoint - current_date).get_days();
        if days_passed > 0 {
            // Determine rate based on year index
            let years_passed = (current_date - start_date).get_days() as f64 / 365.0;
            let year_idx = (years_passed.floor() as usize).min(params.duration_years - 1);

            for acc in &mut histories {
                let rate = acc.yearly_returns[year_idx];
                let start_balance = acc
                    .values
                    .last()
                    .expect("account must have at least 1 balance.")
                    .balance;
                let new_balance = start_balance * (1.0 + n_day_rate(rate, days_passed as f64));
                acc.values.push(AccountSnapshot {
                    date: next_checkpoint,
                    balance: new_balance,
                });
            }
        }
        current_date = next_checkpoint;
    }

    SimulationResult {
        yearly_inflation: inflation_rates,
        account_histories: histories,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 30,
            inflation_profile: InflationProfile::Fixed(0.02),
            events: vec![],
            accounts: vec![Account {
                account_id: 1,
                name: "Savings".to_string(),
                initial_balance: 10_000.0,
                account_type: AccountType::Taxable,
                return_profile: ReturnProfile::Fixed(0.05),
                cash_flows: vec![CashFlow {
                    cash_flow_id: 1,
                    amount: 100.0,
                    description: Some("Monthly contribution".to_string()),
                    start: Timepoint::Immediate,
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Monthly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                }],
            }],
        };

        let result = simulate(&params, 42);

        dbg!(&result);
    }

    #[test]
    fn test_cashflow_limits() {
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2022, 1, 1)),
            duration_years: 10,
            inflation_profile: InflationProfile::None,
            events: vec![],
            accounts: vec![Account {
                account_id: 1,
                name: "Savings".to_string(),
                initial_balance: 10_000.0,
                account_type: AccountType::Taxable,
                return_profile: ReturnProfile::None,
                cash_flows: vec![CashFlow {
                    cash_flow_id: 1,
                    amount: 100.0,
                    description: Some("Monthly contribution".to_string()),
                    start: Timepoint::Immediate,
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Monthly,
                    cash_flow_limits: Some(CashFlowLimits {
                        limit: 1_000.0,
                        limit_period: LimitPeriod::Yearly,
                    }),
                    adjust_for_inflation: false,
                }],
            }],
        };

        let result = simulate(&params, 42);

        // Initial: 10,000
        // Contribution: 100/month -> 1200/year.
        // Limit: 1000/year.
        // Expected annual contribution: 1000.
        // Duration: 10 years.
        // Total added: 10 * 1000 = 10,000.
        // Final Balance: 10,000 + 10,000 = 20,000.

        let final_balance = result.account_histories[0].values.last().unwrap().balance;
        assert_eq!(
            final_balance, 20_000.0,
            "Balance should be capped by yearly limits"
        );
    }

    #[test]
    fn test_simulation_start_date() {
        let start_date = jiff::civil::date(2020, 1, 1);
        let params = SimulationParameters {
            start_date: Some(start_date),
            duration_years: 1,
            inflation_profile: InflationProfile::None,
            events: vec![],
            accounts: vec![Account {
                account_id: 1,
                name: "Savings".to_string(),
                initial_balance: 10_000.0,
                account_type: AccountType::Taxable,
                return_profile: ReturnProfile::None,
                cash_flows: vec![],
            }],
        };

        let result = simulate(&params, 42);

        // Check that the first snapshot date matches the start date
        assert_eq!(result.account_histories[0].values[0].date, start_date);
    }

    #[test]
    fn test_inflation_adjustment() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 2,
            inflation_profile: InflationProfile::Fixed(0.10), // 10% inflation
            events: vec![],
            accounts: vec![Account {
                account_id: 1,
                name: "Checking".to_string(),
                initial_balance: 0.0,
                account_type: AccountType::Taxable,
                return_profile: ReturnProfile::None,
                cash_flows: vec![CashFlow {
                    cash_flow_id: 1,
                    amount: 100.0,
                    description: Some("Yearly income".to_string()),
                    start: Timepoint::Immediate,
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: true,
                }],
            }],
        };

        let result = simulate(&params, 42);
        let history = &result.account_histories[0];

        // Year 0: 100.0
        // Year 1: 100.0 * 1.10 = 110.0
        // Total: 210.0

        let final_balance = history.values.last().unwrap().balance;
        // Floating point comparison
        assert!(
            (final_balance - 210.0).abs() < 1e-6,
            "Expected 210.0, got {}",
            final_balance
        );
    }

    #[test]
    fn test_lifetime_limit() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 5,
            inflation_profile: InflationProfile::None,
            events: vec![],
            accounts: vec![Account {
                account_id: 1,
                name: "Savings".to_string(),
                initial_balance: 0.0,
                account_type: AccountType::Taxable,
                return_profile: ReturnProfile::None,
                cash_flows: vec![CashFlow {
                    cash_flow_id: 1,
                    amount: 1000.0,
                    description: None,
                    start: Timepoint::Immediate,
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: Some(CashFlowLimits {
                        limit: 2500.0,
                        limit_period: LimitPeriod::Lifetime,
                    }),
                    adjust_for_inflation: false,
                }],
            }],
        };

        let result = simulate(&params, 42);
        let final_balance = result.account_histories[0].values.last().unwrap().balance;
        assert_eq!(final_balance, 2500.0);
    }

    #[test]
    fn test_event_trigger_balance() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 5,
            inflation_profile: InflationProfile::None,
            events: vec![Event {
                name: "RichEnough".to_string(),
                trigger: EventTrigger::AccountBalance {
                    account_id: 1,
                    threshold: 5000.0,
                    above: true,
                },
            }],
            accounts: vec![Account {
                account_id: 1,
                name: "Savings".to_string(),
                initial_balance: 0.0,
                account_type: AccountType::Taxable,
                return_profile: ReturnProfile::None,
                cash_flows: vec![
                    // Base income: 2000/year
                    CashFlow {
                        cash_flow_id: 1,
                        amount: 2000.0,
                        description: None,
                        start: Timepoint::Immediate,
                        end: Timepoint::Never,
                        repeats: RepeatInterval::Yearly,
                        cash_flow_limits: None,
                        adjust_for_inflation: false,
                    },
                    // Bonus starts when RichEnough
                    CashFlow {
                        cash_flow_id: 2,
                        amount: 10000.0,
                        description: None,
                        start: Timepoint::Event("RichEnough".to_string()),
                        end: Timepoint::Never,
                        repeats: RepeatInterval::Never, // One time bonus
                        cash_flow_limits: None,
                        adjust_for_inflation: false,
                    },
                ],
            }],
        };

        let result = simulate(&params, 42);
        let final_balance = result.account_histories[0].values.last().unwrap().balance;

        // Year 0: +2000 -> Bal 2000
        // Year 1: +2000 -> Bal 4000
        // Year 2: +2000 -> Bal 6000. Trigger "RichEnough" (Threshold 5000).
        // Bonus +10000 -> Bal 16000.
        // Year 3: +2000 -> Bal 18000.
        // Year 4: +2000 -> Bal 20000.

        assert_eq!(final_balance, 20000.0);
    }

    #[test]
    fn test_interest_accrual() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 1,
            inflation_profile: InflationProfile::None,
            events: vec![],
            accounts: vec![Account {
                account_id: 1,
                name: "Invest".to_string(),
                initial_balance: 0.0,
                account_type: AccountType::Taxable,
                return_profile: ReturnProfile::Fixed(0.10),
                cash_flows: vec![CashFlow {
                    cash_flow_id: 1,
                    amount: 1000.0,
                    description: None,
                    start: Timepoint::Immediate,
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Never,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                }],
            }],
        };

        let result = simulate(&params, 42);
        let final_balance = result.account_histories[0].values.last().unwrap().balance;

        // 1000 invested immediately. 10% return. 1 year.
        // Should be 1100.
        assert!(
            (final_balance - 1100.0).abs() < 1.0,
            "Expected 1100.0, got {}",
            final_balance
        );
    }

    #[test]
    fn test_cross_account_events() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 5,
            inflation_profile: InflationProfile::None,
            events: vec![Event {
                name: "DebtPaid".to_string(),
                trigger: EventTrigger::AccountBalance {
                    account_id: 1, // Debt account
                    threshold: 0.0,
                    above: true, // When balance >= 0 (debt paid off)
                },
            }],
            accounts: vec![
                Account {
                    account_id: 1,
                    name: "Debt".to_string(),
                    initial_balance: -2000.0,
                    account_type: AccountType::Liability,
                    return_profile: ReturnProfile::None,
                    cash_flows: vec![CashFlow {
                        cash_flow_id: 1,
                        amount: 1000.0,
                        description: Some("Debt Payment".to_string()),
                        start: Timepoint::Immediate,
                        end: Timepoint::Event("DebtPaid".to_string()),
                        repeats: RepeatInterval::Yearly,
                        cash_flow_limits: None,
                        adjust_for_inflation: false,
                    }],
                },
                Account {
                    account_id: 2,
                    name: "Savings".to_string(),
                    initial_balance: 0.0,
                    account_type: AccountType::Taxable,
                    return_profile: ReturnProfile::None,
                    cash_flows: vec![CashFlow {
                        cash_flow_id: 2,
                        amount: 1000.0,
                        description: Some("Savings after debt".to_string()),
                        start: Timepoint::Event("DebtPaid".to_string()),
                        end: Timepoint::Never,
                        repeats: RepeatInterval::Yearly,
                        cash_flow_limits: None,
                        adjust_for_inflation: false,
                    }],
                },
            ],
        };

        let result = simulate(&params, 42);
        let debt_history = &result.account_histories[0];
        let savings_history = &result.account_histories[1];

        // Debt Account:
        // Year 0: -2000 + 1000 = -1000
        // Year 1: -1000 + 1000 = 0. Trigger "DebtPaid".
        // Payment stops (end: DebtPaid).
        // Final Debt Balance: 0.

        // Savings Account:
        // Year 0: 0
        // Year 1: 0 (Event triggered this year, but cashflow starts on event)
        //         Wait, if event triggers at Year 1, does cashflow happen immediately?
        //         The logic says: if start == event, next_date = current_date.
        //         So yes, it should happen in Year 1.
        // Year 1: +1000 -> Bal 1000.
        // Year 2: +1000 -> Bal 2000.
        // Year 3: +1000 -> Bal 3000.
        // Year 4: +1000 -> Bal 4000.

        let final_debt = debt_history.values.last().unwrap().balance;
        let final_savings = savings_history.values.last().unwrap().balance;

        assert_eq!(final_debt, 0.0, "Debt should be paid off");
        assert_eq!(
            final_savings, 4000.0,
            "Savings should accumulate after debt is paid"
        );
    }

    #[test]
    fn test_event_and_limits() {
        let start_date = jiff::civil::date(2025, 1, 1);
        let params = SimulationParameters {
            start_date: Some(start_date),
            duration_years: 5,
            inflation_profile: InflationProfile::None,
            events: vec![Event {
                name: "StartSaving".to_string(),
                trigger: EventTrigger::Date(start_date.saturating_add(2.years())),
            }],
            accounts: vec![Account {
                account_id: 1,
                name: "LimitedSavings".to_string(),
                initial_balance: 0.0,
                account_type: AccountType::Taxable,
                return_profile: ReturnProfile::None,
                cash_flows: vec![CashFlow {
                    cash_flow_id: 1,
                    amount: 1000.0, // Monthly
                    description: None,
                    start: Timepoint::Event("StartSaving".to_string()),
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Monthly,
                    cash_flow_limits: Some(CashFlowLimits {
                        limit: 5000.0,
                        limit_period: LimitPeriod::Yearly,
                    }),
                    adjust_for_inflation: false,
                }],
            }],
        };

        let result = simulate(&params, 42);
        let history = &result.account_histories[0];

        // StartSaving triggers at Year 2 (2027-01-01).
        // Year 0 (2025): 0
        // Year 1 (2026): 0
        // Year 2 (2027): Start. Monthly 1000. Limit 5000/year.
        //         Calendar Year 2027 matches Simulation Year 2.
        //         Should contribute 5000 total in Year 2.
        // Year 3 (2028): 5000.
        // Year 4 (2029): 5000.
        // Total: 15000.

        let final_balance = history.values.last().unwrap().balance;
        assert_eq!(final_balance, 15000.0);
    }
}
