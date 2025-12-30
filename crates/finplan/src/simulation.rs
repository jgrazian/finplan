use crate::models::*;
use crate::taxes::{calculate_withdrawal_tax, gross_up_for_net_target};
use jiff::ToSpan;
use rand::{RngCore, SeedableRng};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
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

/// Internal state for a running spending target
struct ActiveSpendingTargetState {
    spending_target_index: usize,
    next_date: Option<jiff::civil::Date>,
}

/// Year-to-date tax tracking
#[derive(Default)]
struct YtdTaxState {
    year: i16,
    ordinary_income: f64,
    capital_gains: f64,
    tax_free_withdrawals: f64,
    federal_tax: f64,
    state_tax: f64,
}

struct SimulationState {
    triggered_events: HashMap<EventId, jiff::civil::Date>,
    histories: Vec<AccountHistory>,
    dates: Vec<jiff::civil::Date>,
    return_profile_returns: Vec<Vec<f64>>,
    active_cash_flows: Vec<ActiveCashFlowState>,
    active_spending_targets: Vec<ActiveSpendingTargetState>,
    cumulative_inflation: Vec<f64>,
    inflation_rates: Vec<f64>,
    current_date: jiff::civil::Date,
    start_date: jiff::civil::Date,
    end_date: jiff::civil::Date,
    // Tax tracking
    ytd_tax: YtdTaxState,
    yearly_taxes: Vec<TaxSummary>,
    withdrawal_history: Vec<WithdrawalRecord>,
}

impl SimulationState {
    fn new(params: &SimulationParameters, seed: u64) -> Self {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let start_date = params
            .start_date
            .unwrap_or_else(|| jiff::Zoned::now().date());
        let end_date = start_date.saturating_add((params.duration_years as i64).years());

        // Sample returns once per return profile, not per asset
        let sampled_returns: Vec<Vec<f64>> = params
            .return_profiles
            .iter()
            .map(|profile| {
                (0..params.duration_years)
                    .map(|_| profile.sample(&mut rng))
                    .collect()
            })
            .collect();

        let histories: Vec<AccountHistory> = params
            .accounts
            .iter()
            .map(|a| {
                let asset_histories: Vec<AssetHistory> = a
                    .assets
                    .iter()
                    .map(|asset| AssetHistory {
                        asset_id: asset.asset_id,
                        return_profile_index: asset.return_profile_index,
                        values: vec![asset.initial_value],
                    })
                    .collect();

                AccountHistory {
                    account_id: a.account_id,
                    assets: asset_histories,
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
            dates: vec![start_date],
            return_profile_returns: sampled_returns,
            active_cash_flows: Vec::new(),
            active_spending_targets: Vec::new(),
            cumulative_inflation,
            inflation_rates,
            current_date: start_date,
            start_date,
            end_date,
            ytd_tax: YtdTaxState {
                year: start_date.year(),
                ..Default::default()
            },
            yearly_taxes: Vec::new(),
            withdrawal_history: Vec::new(),
        }
    }

    fn resolve_start(
        start_date: jiff::civil::Date,
        triggered_events: &HashMap<EventId, jiff::civil::Date>,
        tp: &Timepoint,
    ) -> Option<jiff::civil::Date> {
        match tp {
            Timepoint::Immediate => Some(start_date),
            Timepoint::Date(d) => Some(*d),
            Timepoint::Event(event_id) => triggered_events.get(event_id).copied(),
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

    fn init_spending_targets(&mut self, params: &SimulationParameters) {
        for (st_idx, st) in params.spending_targets.iter().enumerate() {
            let start = Self::resolve_start(self.start_date, &self.triggered_events, &st.start);
            self.active_spending_targets
                .push(ActiveSpendingTargetState {
                    spending_target_index: st_idx,
                    next_date: start,
                });
        }
    }

    /// Finalize YTD taxes when year changes or simulation ends
    fn finalize_year_taxes(&mut self) {
        if self.ytd_tax.ordinary_income > 0.0
            || self.ytd_tax.capital_gains > 0.0
            || self.ytd_tax.tax_free_withdrawals > 0.0
        {
            self.yearly_taxes.push(TaxSummary {
                year: self.ytd_tax.year,
                ordinary_income: self.ytd_tax.ordinary_income,
                capital_gains: self.ytd_tax.capital_gains,
                tax_free_withdrawals: self.ytd_tax.tax_free_withdrawals,
                federal_tax: self.ytd_tax.federal_tax,
                state_tax: self.ytd_tax.state_tax,
                total_tax: self.ytd_tax.federal_tax + self.ytd_tax.state_tax,
            });
        }
    }

    /// Check if we've crossed into a new year and finalize previous year's taxes
    fn maybe_rollover_year(&mut self) {
        let current_year = self.current_date.year();
        if current_year != self.ytd_tax.year {
            self.finalize_year_taxes();
            self.ytd_tax = YtdTaxState {
                year: current_year,
                ..Default::default()
            };
        }
    }

    fn apply_spending_targets(&mut self, params: &SimulationParameters) -> bool {
        self.maybe_rollover_year();

        let mut something_happened = false;

        for ast_idx in 0..self.active_spending_targets.len() {
            let ast = &self.active_spending_targets[ast_idx];
            let Some(date) = ast.next_date else {
                continue;
            };

            if date > self.current_date {
                continue;
            }

            let st = &params.spending_targets[ast.spending_target_index];

            // Check end date
            let end_date_opt =
                Self::resolve_start(self.start_date, &self.triggered_events, &st.end);
            let has_ended = if let Some(end) = end_date_opt {
                self.current_date >= end
            } else {
                false
            };

            if has_ended {
                self.active_spending_targets[ast_idx].next_date = None;
                continue;
            }

            // Calculate target amount (with inflation adjustment)
            let mut target_amount = st.amount;
            if st.adjust_for_inflation {
                let years_passed = (self.current_date - self.start_date).get_days() as f64 / 365.0;
                let year_idx = (years_passed.floor() as usize).min(params.duration_years - 1);
                let fraction = years_passed - (year_idx as f64);
                let inflation_multiplier = self.cumulative_inflation[year_idx]
                    * (1.0 + self.inflation_rates[year_idx]).powf(fraction);
                target_amount *= inflation_multiplier;
            }

            // Execute withdrawal strategy
            let withdrawal_order = self.get_withdrawal_order(params, st);
            let mut remaining_target = target_amount;

            for (account_id, asset_id) in withdrawal_order {
                if remaining_target <= 0.0 {
                    break;
                }

                // Find account type
                let account_type = params
                    .accounts
                    .iter()
                    .find(|a| a.account_id == account_id)
                    .map(|a| &a.account_type)
                    .unwrap_or(&AccountType::Taxable);

                // Get available balance
                let available = self
                    .histories
                    .iter()
                    .find(|h| h.account_id == account_id)
                    .and_then(|h| h.assets.iter().find(|a| a.asset_id == asset_id))
                    .and_then(|a| a.values.last().copied())
                    .unwrap_or(0.0)
                    .max(0.0);

                if available <= 0.0 {
                    continue;
                }

                // Calculate how much to withdraw
                let gross_withdrawal = if st.net_amount_mode {
                    // Need to gross up for taxes
                    let gross = gross_up_for_net_target(
                        remaining_target,
                        account_type,
                        &params.tax_config,
                        self.ytd_tax.ordinary_income,
                    )
                    .unwrap_or(remaining_target);
                    gross.min(available)
                } else {
                    remaining_target.min(available)
                };

                // Calculate taxes on this withdrawal
                let tax_result = calculate_withdrawal_tax(
                    gross_withdrawal,
                    account_type,
                    &params.tax_config,
                    self.ytd_tax.ordinary_income,
                );

                // Apply the withdrawal to the account
                if let Some(history) = self
                    .histories
                    .iter_mut()
                    .find(|h| h.account_id == account_id)
                    && let Some(asset) = history.assets.iter_mut().find(|a| a.asset_id == asset_id)
                    && let Some(last_val) = asset.values.last_mut()
                {
                    *last_val -= gross_withdrawal;
                }

                // Track taxes
                match account_type {
                    AccountType::TaxDeferred => {
                        self.ytd_tax.ordinary_income += gross_withdrawal;
                    }
                    AccountType::Taxable => {
                        self.ytd_tax.capital_gains +=
                            gross_withdrawal * params.tax_config.taxable_gains_percentage;
                    }
                    AccountType::TaxFree => {
                        self.ytd_tax.tax_free_withdrawals += gross_withdrawal;
                    }
                    AccountType::Illiquid => {}
                }
                self.ytd_tax.federal_tax += tax_result.federal_tax;
                self.ytd_tax.state_tax += tax_result.state_tax + tax_result.capital_gains_tax;

                // Record the withdrawal
                self.withdrawal_history.push(WithdrawalRecord {
                    date: self.current_date,
                    spending_target_id: st.spending_target_id,
                    account_id,
                    asset_id,
                    gross_amount: gross_withdrawal,
                    tax_amount: tax_result.total_tax,
                    net_amount: tax_result.net_amount,
                });

                // Update remaining target
                if st.net_amount_mode {
                    remaining_target -= tax_result.net_amount;
                } else {
                    remaining_target -= gross_withdrawal;
                }

                something_happened = true;
            }

            // Schedule next occurrence
            let next = match &st.repeats {
                RepeatInterval::Never => None,
                interval => Some(date.saturating_add(interval.span())),
            };
            self.active_spending_targets[ast_idx].next_date = next;
        }

        something_happened
    }

    /// Get the order of (account_id, asset_id) pairs to withdraw from based on strategy
    fn get_withdrawal_order(
        &self,
        params: &SimulationParameters,
        target: &SpendingTarget,
    ) -> Vec<(AccountId, AssetId)> {
        // Build list of all liquid accounts/assets with their balances
        let mut candidates: Vec<(AccountId, AssetId, &AccountType, f64)> = Vec::new();

        for account in &params.accounts {
            // Skip illiquid and excluded accounts
            if matches!(account.account_type, AccountType::Illiquid) {
                continue;
            }
            if target.exclude_accounts.contains(&account.account_id) {
                continue;
            }

            // Get current balances from history
            if let Some(history) = self
                .histories
                .iter()
                .find(|h| h.account_id == account.account_id)
            {
                for asset in &account.assets {
                    let balance = history
                        .assets
                        .iter()
                        .find(|a| a.asset_id == asset.asset_id)
                        .and_then(|a| a.values.last().copied())
                        .unwrap_or(0.0);

                    if balance > 0.0 {
                        candidates.push((
                            account.account_id,
                            asset.asset_id,
                            &account.account_type,
                            balance,
                        ));
                    }
                }
            }
        }

        match &target.withdrawal_strategy {
            WithdrawalStrategy::Sequential { order } => {
                // Sort by the provided order
                candidates.sort_by(|a, b| {
                    let a_idx = order.iter().position(|id| *id == a.0).unwrap_or(usize::MAX);
                    let b_idx = order.iter().position(|id| *id == b.0).unwrap_or(usize::MAX);
                    a_idx.cmp(&b_idx)
                });
            }
            WithdrawalStrategy::ProRata => {
                // For pro-rata, we'll handle proportional withdrawal in the main loop
                // For now, just sort by balance descending
                candidates
                    .sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
            }
            WithdrawalStrategy::TaxOptimized => {
                // Tax-optimized order: Taxable first (cap gains), then TaxDeferred, then TaxFree
                candidates.sort_by(|a, b| {
                    let type_order = |t: &AccountType| match t {
                        AccountType::Taxable => 0,
                        AccountType::TaxDeferred => 1,
                        AccountType::TaxFree => 2,
                        AccountType::Illiquid => 3,
                    };
                    type_order(a.2).cmp(&type_order(b.2))
                });
            }
        }

        candidates.into_iter().map(|(a, b, _, _)| (a, b)).collect()
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
                asset_id,
            } = cf.source
                && let Some(history) = self
                    .histories
                    .iter_mut()
                    .find(|h| h.account_id == account_id)
                && let Some(asset) = history.assets.iter_mut().find(|a| a.asset_id == asset_id)
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
                asset_id,
            } = cf.target
                && let Some(history) = self
                    .histories
                    .iter_mut()
                    .find(|h| h.account_id == account_id)
                && let Some(asset) = history.assets.iter_mut().find(|a| a.asset_id == asset_id)
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
            if self.triggered_events.contains_key(&event.event_id) {
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
                    asset_id,
                    threshold,
                    above,
                } => {
                    if let Some(acc) = self.histories.iter().find(|a| a.account_id == *account_id) {
                        if let Some(asset) = acc.assets.iter().find(|a| a.asset_id == *asset_id) {
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
                new_triggers.push(event.event_id);
            }
        }

        if !new_triggers.is_empty() {
            for event_id in new_triggers {
                self.triggered_events.insert(event_id, self.current_date);

                // Wake up cashflows waiting for this event
                for acf in &mut self.active_cash_flows {
                    let cf = &params.cash_flows[acf.cash_flow_index];
                    if let Timepoint::Event(event_id_) = cf.start
                        && event_id_ == event_id
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

        for ast in &self.active_spending_targets {
            if let Some(d) = ast.next_date
                && d > self.current_date
                && d < next_checkpoint
            {
                next_checkpoint = d;
            }
        }

        for event in &params.events {
            if !self.triggered_events.contains_key(&event.event_id)
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
                    let rate = self.return_profile_returns[asset.return_profile_index][year_idx];
                    let start_value = *asset.values.last().unwrap();
                    let new_value = start_value * (1.0 + n_day_rate(rate, days_passed as f64));
                    asset.values.push(new_value);
                }
            }

            self.dates.push(next_checkpoint);
        }
        self.current_date = next_checkpoint;
    }
}

pub fn simulate(params: &SimulationParameters, seed: u64) -> SimulationResult {
    let mut state = SimulationState::new(params, seed);
    state.init_cash_flows(params);
    state.init_spending_targets(params);

    while state.current_date < state.end_date {
        let mut something_happened = true;
        while something_happened {
            something_happened = false;
            if state.apply_spending_targets(params) {
                something_happened = true;
            }
            if state.apply_cash_flows(params) {
                something_happened = true;
            }
            if state.check_triggers(params) {
                something_happened = true;
            }
        }
        state.advance_time(params);
    }

    // Finalize last year's taxes
    state.finalize_year_taxes();

    SimulationResult {
        yearly_inflation: state.inflation_rates,
        dates: state.dates,
        return_profile_returns: state.return_profile_returns,
        triggered_events: state.triggered_events,
        account_histories: state.histories,
        yearly_taxes: state.yearly_taxes,
        withdrawal_history: state.withdrawal_history,
    }
}

pub fn monte_carlo_simulate(
    params: &SimulationParameters,
    num_iterations: usize,
) -> MonteCarloResult {
    let iterations = (0..num_iterations)
        .into_par_iter()
        .map_init(rand::rng, |rng, _| {
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
            return_profiles: vec![ReturnProfile::Fixed(0.05)],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 100.0,
                start: Timepoint::Immediate,
                end: Timepoint::Never,
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
            }],
            ..Default::default()
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
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 100.0,
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
                    asset_id: AssetId(1),
                },
            }],
            ..Default::default()
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
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // Check that the first snapshot date matches the start date
        assert_eq!(result.dates[0], start_date);
    }

    #[test]
    fn test_inflation_adjustment() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 2,
            inflation_profile: InflationProfile::Fixed(0.10), // 10% inflation
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 100.0,
                start: Timepoint::Immediate,
                end: Timepoint::Never,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: true,
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
            }],
            ..Default::default()
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
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1000.0,
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
                    asset_id: AssetId(1),
                },
            }],
            ..Default::default()
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
            return_profiles: vec![ReturnProfile::None],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::TotalAccountBalance {
                    account_id: AccountId(1),
                    threshold: 5000.0,
                    above: true,
                },
            }],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![
                // Base income: 2000/year
                CashFlow {
                    cash_flow_id: CashFlowId(1),
                    amount: 2000.0,
                    start: Timepoint::Immediate,
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    source: CashFlowEndpoint::External,
                    target: CashFlowEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                },
                // Bonus starts when RichEnough
                CashFlow {
                    cash_flow_id: CashFlowId(2),
                    amount: 10000.0,
                    start: Timepoint::Event(EventId(1)),
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Never, // One time bonus
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    source: CashFlowEndpoint::External,
                    target: CashFlowEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                },
            ],
            ..Default::default()
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
            return_profiles: vec![ReturnProfile::Fixed(0.10)],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1000.0,
                start: Timepoint::Immediate,
                end: Timepoint::Never,
                repeats: RepeatInterval::Never,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
            }],
            ..Default::default()
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
            return_profiles: vec![ReturnProfile::None],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::TotalAccountBalance {
                    account_id: AccountId(1), // Debt account
                    threshold: 0.0,
                    above: true, // When balance >= 0 (debt paid off)
                },
            }],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: -2000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Liability,
                    }],
                    account_type: AccountType::Illiquid,
                },
                Account {
                    account_id: AccountId(2),
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 0.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                    account_type: AccountType::Taxable,
                },
            ],
            cash_flows: vec![
                CashFlow {
                    cash_flow_id: CashFlowId(1),
                    amount: 1000.0,
                    start: Timepoint::Immediate,
                    end: Timepoint::Event(EventId(1)),
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    source: CashFlowEndpoint::External,
                    target: CashFlowEndpoint::Asset {
                        account_id: AccountId(1),
                        asset_id: AssetId(1),
                    },
                },
                CashFlow {
                    cash_flow_id: CashFlowId(2),
                    amount: 1000.0,
                    start: Timepoint::Event(EventId(1)),
                    end: Timepoint::Never,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    source: CashFlowEndpoint::External,
                    target: CashFlowEndpoint::Asset {
                        account_id: AccountId(2),
                        asset_id: AssetId(1),
                    },
                },
            ],
            ..Default::default()
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
            return_profiles: vec![
                ReturnProfile::Fixed(0.06),
                ReturnProfile::Normal {
                    mean: 0.03,
                    std_dev: 0.02,
                },
            ],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::AssetBalance {
                    account_id: AccountId(1),
                    asset_id: AssetId(1), // Home Mortgage
                    threshold: -1.0,
                    above: true,
                },
            }],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![
                    Asset {
                        asset_id: AssetId(1),
                        initial_value: -300_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Liability,
                    },
                    Asset {
                        asset_id: AssetId(2),
                        initial_value: 300_000.0,
                        return_profile_index: 1,
                        asset_class: AssetClass::RealEstate,
                    },
                ],
                account_type: AccountType::Illiquid,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 3_500.0, // Monthly payment
                start: Timepoint::Immediate,
                end: Timepoint::Event(EventId(1)),
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: None,
                adjust_for_inflation: true,
                source: CashFlowEndpoint::External,
                target: CashFlowEndpoint::Asset {
                    account_id: AccountId(1),
                    asset_id: AssetId(1),
                },
            }],
            ..Default::default()
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
            return_profiles: vec![ReturnProfile::None],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::Date(start_date.saturating_add(2.years())),
            }],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1000.0, // Monthly
                start: Timepoint::Event(EventId(1)),
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
                    asset_id: AssetId(1),
                },
            }],
            ..Default::default()
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
            return_profiles: vec![ReturnProfile::Normal {
                mean: 0.07,
                std_dev: 0.15,
            }],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![],
            ..Default::default()
        };

        let result = monte_carlo_simulate(&params, 100);
        assert_eq!(result.iterations.len(), 100);

        // Check that results are different (due to random seed)
        let first_final = result.iterations[0].account_histories[0].current_balance();
        let second_final = result.iterations[1].account_histories[0].current_balance();

        assert_ne!(first_final, second_final);
    }

    #[test]
    fn test_shared_return_profile() {
        // Test that assets in different accounts with the same return_profile_index
        // share the same yearly returns
        let params = SimulationParameters {
            start_date: None,
            duration_years: 10,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::Normal {
                mean: 0.07,
                std_dev: 0.15,
            }],
            events: vec![],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 10_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                    account_type: AccountType::Taxable,
                },
                Account {
                    account_id: AccountId(2),
                    assets: vec![Asset {
                        asset_id: AssetId(2),
                        initial_value: 5_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                    account_type: AccountType::TaxDeferred,
                },
            ],
            cash_flows: vec![],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // Both assets should have the same return_profile_index, which means they use
        // the same returns from result.return_profile_returns
        let asset1_profile_idx = result.account_histories[0].assets[0].return_profile_index;
        let asset2_profile_idx = result.account_histories[1].assets[0].return_profile_index;

        assert_eq!(
            asset1_profile_idx, asset2_profile_idx,
            "Both assets should reference the same return profile"
        );
        assert_eq!(
            asset1_profile_idx, 0,
            "Both assets should use return_profile_index 0"
        );
    }

    #[test]
    fn test_spending_target_basic() {
        // Test basic spending target withdrawal from a single account
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2025, 1, 1)),
            duration_years: 5,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                account_type: AccountType::TaxDeferred, // 401k - taxed as ordinary income
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 100_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
            }],
            cash_flows: vec![],
            spending_targets: vec![SpendingTarget {
                spending_target_id: SpendingTargetId(1),
                amount: 10_000.0,
                net_amount_mode: false, // Gross withdrawal
                start: Timepoint::Immediate,
                end: Timepoint::Never,
                repeats: RepeatInterval::Yearly,
                adjust_for_inflation: false,
                withdrawal_strategy: WithdrawalStrategy::Sequential {
                    order: vec![AccountId(1)],
                },
                exclude_accounts: vec![],
            }],
            tax_config: TaxConfig::default(),
        };

        let result = simulate(&params, 42);
        let final_balance = result.account_histories[0].current_balance();

        // Starting: 100,000
        // Yearly withdrawal: 10,000
        // After 5 years: 100,000 - (5 * 10,000) = 50,000
        assert!(
            (final_balance - 50_000.0).abs() < 1.0,
            "Expected ~50,000, got {}",
            final_balance
        );

        // Check that taxes were tracked
        assert!(!result.yearly_taxes.is_empty(), "Should have tax records");

        // Each year should have 10,000 in ordinary income (TaxDeferred withdrawal)
        for tax in &result.yearly_taxes {
            assert!(
                (tax.ordinary_income - 10_000.0).abs() < 1.0,
                "Expected 10,000 ordinary income, got {}",
                tax.ordinary_income
            );
        }
    }

    #[test]
    fn test_spending_target_tax_optimized() {
        // Test tax-optimized withdrawal order: Taxable -> TaxDeferred -> TaxFree
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2025, 1, 1)),
            duration_years: 3,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    account_type: AccountType::TaxFree, // Roth - should be last
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 50_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                },
                Account {
                    account_id: AccountId(2),
                    account_type: AccountType::TaxDeferred, // 401k - should be second
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 50_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                },
                Account {
                    account_id: AccountId(3),
                    account_type: AccountType::Taxable, // Brokerage - should be first
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 30_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                },
            ],
            cash_flows: vec![],
            spending_targets: vec![SpendingTarget {
                spending_target_id: SpendingTargetId(1),
                amount: 40_000.0,
                net_amount_mode: false,
                start: Timepoint::Immediate,
                end: Timepoint::Never,
                repeats: RepeatInterval::Yearly,
                adjust_for_inflation: false,
                withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
                exclude_accounts: vec![],
            }],
            tax_config: TaxConfig::default(),
        };

        let result = simulate(&params, 42);

        // Year 1: Need 40k. Taxable has 30k, so take all 30k from Taxable, then 10k from TaxDeferred
        // Year 2: Taxable empty. Take 40k from TaxDeferred (has 40k left)
        // Year 3: TaxDeferred empty. Take 40k from TaxFree

        // Final balances:
        // Taxable: 0
        // TaxDeferred: 0
        // TaxFree: 50,000 - 40,000 = 10,000

        let taxfree_balance = result.account_histories[0].current_balance();
        let taxdeferred_balance = result.account_histories[1].current_balance();
        let taxable_balance = result.account_histories[2].current_balance();

        assert!(
            taxable_balance.abs() < 1.0,
            "Taxable should be depleted first, got {}",
            taxable_balance
        );
        assert!(
            taxdeferred_balance.abs() < 1.0,
            "TaxDeferred should be depleted second, got {}",
            taxdeferred_balance
        );
        assert!(
            (taxfree_balance - 10_000.0).abs() < 1.0,
            "TaxFree should have ~10,000 left, got {}",
            taxfree_balance
        );
    }

    #[test]
    fn test_spending_target_excludes_illiquid() {
        // Test that Illiquid accounts are automatically skipped
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2025, 1, 1)),
            duration_years: 2,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    account_type: AccountType::Illiquid, // Real estate - cannot withdraw
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 500_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::RealEstate,
                    }],
                },
                Account {
                    account_id: AccountId(2),
                    account_type: AccountType::Taxable,
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 50_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                },
            ],
            cash_flows: vec![],
            spending_targets: vec![SpendingTarget {
                spending_target_id: SpendingTargetId(1),
                amount: 20_000.0,
                net_amount_mode: false,
                start: Timepoint::Immediate,
                end: Timepoint::Never,
                repeats: RepeatInterval::Yearly,
                adjust_for_inflation: false,
                withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
                exclude_accounts: vec![],
            }],
            tax_config: TaxConfig::default(),
        };

        let result = simulate(&params, 42);

        // Illiquid account should be untouched
        let illiquid_balance = result.account_histories[0].current_balance();
        assert_eq!(
            illiquid_balance, 500_000.0,
            "Illiquid account should be untouched"
        );

        // Taxable should have withdrawals
        let taxable_balance = result.account_histories[1].current_balance();
        assert!(
            (taxable_balance - 10_000.0).abs() < 1.0,
            "Taxable should have 10,000 left after 2 years of 20k withdrawals, got {}",
            taxable_balance
        );
    }
}
