use crate::models::*;
use jiff::ToSpan;
use rand::{RngCore, SeedableRng};
use std::collections::HashMap;

pub fn n_day_rate(yearly_rate: f64, n_days: f64) -> f64 {
    (1.0 + yearly_rate).powf(n_days / 365.0) - 1.0
}

/// Internal state for a running cash flow
struct ActiveCashFlowState {
    cash_flow_index: usize,
    next_date: Option<jiff::civil::Date>,
    period_accumulated: f64,
    last_period_key: i16,
}

struct SimulationState {
    triggered_events: HashMap<String, jiff::civil::Date>,
    histories: Vec<AccountHistory>,
    active_cash_flows: Vec<ActiveCashFlowState>,
    cumulative_inflation: Vec<f64>,
    inflation_rates: Vec<f64>,
    current_date: jiff::civil::Date,
    start_date: jiff::civil::Date,
    end_date: jiff::civil::Date,
}

impl SimulationState {
    fn new(params: &SimulationParameters, seed: u64) -> Self {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let start_date = params
            .start_date
            .unwrap_or_else(|| jiff::Zoned::now().date());
        let end_date = start_date.saturating_add((params.duration_years as i64).years());

        let histories: Vec<AccountHistory> = params
            .accounts
            .iter()
            .map(|a| {
                let asset_histories: Vec<AssetHistory> = a
                    .assets
                    .iter()
                    .map(|asset| AssetHistory {
                        name: asset.name.clone(),
                        yearly_returns: (0..params.duration_years)
                            .map(|_| asset.return_profile.sample(&mut rng))
                            .collect(),
                        values: vec![asset.value],
                    })
                    .collect();

                AccountHistory {
                    account_id: a.account_id,
                    assets: asset_histories,
                    dates: vec![start_date],
                }
            })
            .collect();

        let inflation_rates = (0..params.duration_years)
            .map(|_| params.inflation_profile.sample(&mut rng))
            .collect::<Vec<_>>();

        let mut cumulative_inflation = Vec::with_capacity(params.duration_years + 1);
        cumulative_inflation.push(1.0);
        for r in &inflation_rates {
            cumulative_inflation.push(cumulative_inflation.last().unwrap() * (1.0 + r));
        }

        Self {
            triggered_events: HashMap::new(),
            histories,
            active_cash_flows: Vec::new(),
            cumulative_inflation,
            inflation_rates,
            current_date: start_date,
            start_date,
            end_date,
        }
    }

    fn resolve_start(
        start_date: jiff::civil::Date,
        triggered_events: &HashMap<String, jiff::civil::Date>,
        tp: &Timepoint,
    ) -> Option<jiff::civil::Date> {
        match tp {
            Timepoint::Immediate => Some(start_date),
            Timepoint::Date(d) => Some(*d),
            Timepoint::Event(name) => triggered_events.get(name).copied(),
            Timepoint::Never => None,
        }
    }

    fn init_cash_flows(&mut self, params: &SimulationParameters) {
        for (cf_idx, cf) in params.cash_flows.iter().enumerate() {
            let start = Self::resolve_start(self.start_date, &self.triggered_events, &cf.start);
            // If start depends on an event not yet triggered, it will be None.
            // We still track it.
            self.active_cash_flows.push(ActiveCashFlowState {
                cash_flow_index: cf_idx,
                next_date: start,
                period_accumulated: 0.0,
                last_period_key: self.start_date.year(),
            });
        }
    }

    fn apply_cash_flows(&mut self, params: &SimulationParameters) -> bool {
        let mut something_happened = false;
        for acf in &mut self.active_cash_flows {
            let Some(date) = acf.next_date else {
                continue;
            };

            if date > self.current_date {
                continue;
            }

            let cf = &params.cash_flows[acf.cash_flow_index];

            // Check end date
            let end_date_opt =
                Self::resolve_start(self.start_date, &self.triggered_events, &cf.end);
            let has_ended = if let Some(end) = end_date_opt {
                self.current_date >= end
            } else {
                false
            };

            if has_ended {
                acf.next_date = None;
                continue;
            }

            // Event has not ended, apply cash flow
            let mut amount = cf.amount;
            if cf.adjust_for_inflation {
                let years_passed = (self.current_date - self.start_date).get_days() as f64 / 365.0;
                let year_idx = (years_passed.floor() as usize).min(params.duration_years - 1);
                let fraction = years_passed - (year_idx as f64);
                let inflation_multiplier = self.cumulative_inflation[year_idx]
                    * (1.0 + self.inflation_rates[year_idx]).powf(fraction);
                amount *= inflation_multiplier;
            }

            if let Some(limits) = &cf.cash_flow_limits {
                let current_year = self.current_date.year();
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

            // Apply source (subtract from source account/asset)
            if let CashFlowEndpoint::Asset {
                account_id,
                ref asset_name,
            } = cf.source
                && let Some(history) = self
                    .histories
                    .iter_mut()
                    .find(|h| h.account_id == account_id)
                && let Some(asset) = history.assets.iter_mut().find(|a| &a.name == asset_name)
            {
                let last_val = asset
                    .values
                    .last_mut()
                    .expect("asset must have at least 1 value");
                *last_val -= amount;
            }

            // Apply target (add to target account/asset)
            if let CashFlowEndpoint::Asset {
                account_id,
                ref asset_name,
            } = cf.target
                && let Some(history) = self
                    .histories
                    .iter_mut()
                    .find(|h| h.account_id == account_id)
                && let Some(asset) = history.assets.iter_mut().find(|a| &a.name == asset_name)
            {
                let last_val = asset
                    .values
                    .last_mut()
                    .expect("asset must have at least 1 value");
                *last_val += amount;
            }

            match &cf.repeats {
                RepeatInterval::Never => acf.next_date = None,
                interval => {
                    let next = date.saturating_add(interval.span());
                    acf.next_date = Some(next);
                }
            }
            something_happened = true;
        }

        something_happened
    }

    fn check_triggers(&mut self, params: &SimulationParameters) -> bool {
        let mut new_triggers = Vec::new();
        for event in &params.events {
            if self.triggered_events.contains_key(&event.name) {
                continue;
            }

            let triggered = match &event.trigger {
                EventTrigger::Date(d) => self.current_date >= *d,
                EventTrigger::TotalAccountBalance {
                    account_id,
                    threshold,
                    above,
                } => {
                    if let Some(acc) = self.histories.iter().find(|a| a.account_id == *account_id) {
                        if *above {
                            acc.current_balance() >= *threshold
                        } else {
                            acc.current_balance() <= *threshold
                        }
                    } else {
                        false
                    }
                }
                EventTrigger::AssetBalance {
                    account_id,
                    asset_name,
                    threshold,
                    above,
                } => {
                    if let Some(acc) = self.histories.iter().find(|a| a.account_id == *account_id) {
                        if let Some(asset) = acc.assets.iter().find(|a| &a.name == asset_name) {
                            let balance = *asset.values.last().unwrap_or(&0.0);
                            if *above {
                                balance >= *threshold
                            } else {
                                balance <= *threshold
                            }
                        } else {
                            false
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

        if !new_triggers.is_empty() {
            for name in new_triggers {
                self.triggered_events
                    .insert(name.clone(), self.current_date);

                // Wake up cashflows waiting for this event
                for acf in &mut self.active_cash_flows {
                    let cf = &params.cash_flows[acf.cash_flow_index];
                    if let Timepoint::Event(ref event_name) = cf.start
                        && event_name == &name
                    {
                        acf.next_date = Some(self.current_date);
                    }
                }
            }
            return true;
        }
        false
    }

    fn advance_time(&mut self, params: &SimulationParameters) {
        // Find next checkpoint
        let mut next_checkpoint = self.end_date;

        for acf in &self.active_cash_flows {
            if let Some(d) = acf.next_date
                && d > self.current_date
                && d < next_checkpoint
            {
                next_checkpoint = d;
            }
        }

        for event in &params.events {
            if !self.triggered_events.contains_key(&event.name)
                && let EventTrigger::Date(d) = event.trigger
                && d > self.current_date
                && d < next_checkpoint
            {
                next_checkpoint = d;
            }
        }

        let heartbeat = self.current_date.saturating_add(1.month());
        if heartbeat < next_checkpoint {
            next_checkpoint = heartbeat;
        }

        // Apply interest
        let days_passed = (next_checkpoint - self.current_date).get_days();
        if days_passed > 0 {
            let years_passed = (self.current_date - self.start_date).get_days() as f64 / 365.0;
            let year_idx = (years_passed.floor() as usize).min(params.duration_years - 1);

            for acc in &mut self.histories {
                for asset in &mut acc.assets {
                    let rate = asset.yearly_returns[year_idx];
                    let start_value = *asset.values.last().unwrap();
                    let new_value = start_value * (1.0 + n_day_rate(rate, days_passed as f64));
                    asset.values.push(new_value);
                }

                acc.dates.push(next_checkpoint);
            }
        }
        self.current_date = next_checkpoint;
    }
}

pub fn simulate(params: &SimulationParameters, seed: u64) -> SimulationResult {
    let mut state = SimulationState::new(params, seed);
    state.init_cash_flows(params);

    while state.current_date < state.end_date {
        let mut something_happened = true;
        while something_happened {
            something_happened = false;
            if state.apply_cash_flows(params) {
                something_happened = true;
            }
            if state.check_triggers(params) {
                something_happened = true;
            }
        }
        state.advance_time(params);
    }

    SimulationResult {
        yearly_inflation: state.inflation_rates,
        account_histories: state.histories,
    }
}

pub fn monte_carlo_simulate(
    params: &SimulationParameters,
    num_iterations: usize,
) -> MonteCarloResult {
    let mut rng = rand::rng();
    let iterations = (0..num_iterations)
        .map(|_| {
            let seed = rng.next_u64();
            simulate(params, seed)
        })
        .collect();

    MonteCarloResult { iterations }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::*;

    #[test]
    fn test_simulation() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 30,
            inflation_profile: InflationProfile::Fixed(0.02),
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                name: "Savings".to_string(),
                assets: vec![Asset {
                    name: "Cash".to_string(),
                    value: 10_000.0,
                    return_profile: ReturnProfile::Fixed(0.05),
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 100.0,
                description: Some("Monthly contribution".to_string()),
                start: Timepoint::Immediate,
                end: Timepoint::Never,
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_name: "Cash".to_string(),
                },
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
                account_id: AccountId(1),
                name: "Savings".to_string(),
                assets: vec![Asset {
                    name: "Cash".to_string(),
                    value: 10_000.0,
                    return_profile: ReturnProfile::None,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
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
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_name: "Cash".to_string(),
                },
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

        let final_balance = result.account_histories[0].current_balance();
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
                account_id: AccountId(1),
                name: "Savings".to_string(),
                assets: vec![Asset {
                    name: "Cash".to_string(),
                    value: 10_000.0,
                    return_profile: ReturnProfile::None,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![],
        };

        let result = simulate(&params, 42);

        // Check that the first snapshot date matches the start date
        assert_eq!(result.account_histories[0].dates[0], start_date);
    }

    #[test]
    fn test_inflation_adjustment() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 2,
            inflation_profile: InflationProfile::Fixed(0.10), // 10% inflation
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                name: "Checking".to_string(),
                assets: vec![Asset {
                    name: "Cash".to_string(),
                    value: 0.0,
                    return_profile: ReturnProfile::None,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 100.0,
                description: Some("Yearly income".to_string()),
                start: Timepoint::Immediate,
                end: Timepoint::Never,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: true,
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_name: "Cash".to_string(),
                },
            }],
        };

        let result = simulate(&params, 42);
        let history = &result.account_histories[0];

        // Year 0: 100.0
        // Year 1: 100.0 * 1.10 = 110.0
        // Total: 210.0

        let final_balance = history.current_balance();
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
                account_id: AccountId(1),
                name: "Savings".to_string(),
                assets: vec![Asset {
                    name: "Cash".to_string(),
                    value: 0.0,
                    return_profile: ReturnProfile::None,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
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
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_name: "Cash".to_string(),
                },
            }],
        };

        let result = simulate(&params, 42);
        let final_balance = result.account_histories[0].current_balance();
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
                trigger: EventTrigger::TotalAccountBalance {
                    account_id: AccountId(1),
                    threshold: 5000.0,
                    above: true,
                },
            }],
            accounts: vec![Account {
                account_id: AccountId(1),
                name: "Savings".to_string(),
                assets: vec![Asset {
                    name: "Cash".to_string(),
                    value: 0.0,
                    return_profile: ReturnProfile::None,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![
                // Base income: 2000/year
                CashFlow {
                    cash_flow_id: CashFlowId(1),
                    amount: 2000.0,
                    description: None,
                    start: Timepoint::Immediate,
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    source: CashFlowEndpoint::External,
                    target: CashFlowEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_name: "Cash".to_string(),
                    },
                },
                // Bonus starts when RichEnough
                CashFlow {
                    cash_flow_id: CashFlowId(2),
                    amount: 10000.0,
                    description: None,
                    start: Timepoint::Event("RichEnough".to_string()),
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Never, // One time bonus
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    source: CashFlowEndpoint::External,
                    target: CashFlowEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_name: "Cash".to_string(),
                    },
                },
            ],
        };

        let result = simulate(&params, 42);
        let final_balance = result.account_histories[0].current_balance();

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
                account_id: AccountId(1),
                name: "Invest".to_string(),
                assets: vec![Asset {
                    name: "Stock".to_string(),
                    value: 0.0,
                    return_profile: ReturnProfile::Fixed(0.10),
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1000.0,
                description: None,
                start: Timepoint::Immediate,
                end: Timepoint::Never,
                repeats: RepeatInterval::Never,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_name: "Stock".to_string(),
                },
            }],
        };

        let result = simulate(&params, 42);
        let final_balance = result.account_histories[0].current_balance();

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
                trigger: EventTrigger::TotalAccountBalance {
                    account_id: AccountId(1), // Debt account
                    threshold: 0.0,
                    above: true, // When balance >= 0 (debt paid off)
                },
            }],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    name: "Debt".to_string(),
                    assets: vec![Asset {
                        name: "Loan".to_string(),
                        value: -2000.0,
                        return_profile: ReturnProfile::None,
                        asset_class: AssetClass::Liability,
                    }],
                    account_type: AccountType::Illiquid,
                },
                Account {
                    account_id: AccountId(2),
                    name: "Savings".to_string(),
                    assets: vec![Asset {
                        name: "Cash".to_string(),
                        value: 0.0,
                        return_profile: ReturnProfile::None,
                        asset_class: AssetClass::Investable,
                    }],
                    account_type: AccountType::Taxable,
                },
            ],
            cash_flows: vec![
                CashFlow {
                    cash_flow_id: CashFlowId(1),
                    amount: 1000.0,
                    description: Some("Debt Payment".to_string()),
                    start: Timepoint::Immediate,
                    end: Timepoint::Event("DebtPaid".to_string()),
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    source: CashFlowEndpoint::External,
                    target: CashFlowEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_name: "Loan".to_string(),
                    },
                },
                CashFlow {
                    cash_flow_id: CashFlowId(2),
                    amount: 1000.0,
                    description: Some("Savings after debt".to_string()),
                    start: Timepoint::Event("DebtPaid".to_string()),
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    source: CashFlowEndpoint::External,
                    target: CashFlowEndpoint::Asset {
                        account_id: AccountId(2),
                        asset_name: "Cash".to_string(),
                    },
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

        let final_debt = debt_history.current_balance();
        let final_savings = savings_history.current_balance();

        assert_eq!(final_debt, 0.0, "Debt should be paid off");
        assert_eq!(
            final_savings, 4000.0,
            "Savings should accumulate after debt is paid"
        );
    }

    #[test]
    fn test_house_mortgage_scenario() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 10,
            inflation_profile: InflationProfile::Fixed(0.03),
            events: vec![Event {
                name: "MortgagePaidOff".to_string(),
                trigger: EventTrigger::AssetBalance {
                    account_id: AccountId(1),
                    asset_name: "Home Mortgage".to_string(),
                    threshold: -1.0,
                    above: true,
                },
            }],
            accounts: vec![Account {
                account_id: AccountId(1),
                name: "House".to_string(),
                assets: vec![
                    Asset {
                        name: "Home Mortgage".to_string(),
                        value: -300_000.0,
                        return_profile: ReturnProfile::Fixed(0.06),
                        asset_class: AssetClass::Liability,
                    },
                    Asset {
                        name: "Home Value".to_string(),
                        value: 300_000.0,
                        return_profile: ReturnProfile::Normal {
                            mean: 0.03,
                            std_dev: 0.02,
                        },
                        asset_class: AssetClass::RealEstate,
                    },
                ],
                account_type: AccountType::Illiquid,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 3_500.0, // Monthly payment
                description: Some("Mortgage Payment".to_string()),
                start: Timepoint::Immediate,
                end: Timepoint::Event("MortgagePaidOff".to_string()),
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: None,
                adjust_for_inflation: true,
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_name: "Home Mortgage".to_string(),
                },
            }],
        };

        let result = simulate(&params, 42);
        dbg!(&result);
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
                account_id: AccountId(1),
                name: "LimitedSavings".to_string(),
                assets: vec![Asset {
                    name: "Cash".to_string(),
                    value: 0.0,
                    return_profile: ReturnProfile::None,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
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
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_name: "Cash".to_string(),
                },
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

        let final_balance = history.current_balance();
        assert_eq!(final_balance, 15000.0);
    }

    #[test]
    fn test_monte_carlo_simulation() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 30,
            inflation_profile: InflationProfile::Fixed(0.02),
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                name: "Savings".to_string(),
                assets: vec![Asset {
                    name: "Stock".to_string(),
                    value: 10_000.0,
                    return_profile: ReturnProfile::Normal {
                        mean: 0.07,
                        std_dev: 0.15,
                    },
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![],
        };

        let result = monte_carlo_simulate(&params, 100);
        assert_eq!(result.iterations.len(), 100);

        // Check that results are different (due to random seed)
        let first_final = result.iterations[0].account_histories[0].current_balance();
        let second_final = result.iterations[1].account_histories[0].current_balance();

        assert_ne!(first_final, second_final);
    }
}
