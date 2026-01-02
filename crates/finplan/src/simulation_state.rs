use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountId, AccountSnapshot, AssetId, AssetSnapshot, Event, EventId, Record,
    RecordKind, RmdTable, TaxConfig, TaxSummary,
};
use jiff::ToSpan;
use rand::SeedableRng;
use std::collections::HashMap;

/// A single purchase lot for cost basis tracking
#[derive(Debug, Clone)]
pub struct AssetLot {
    /// Date of purchase
    pub purchase_date: jiff::civil::Date,
    /// Number of shares/units (or dollar amount for non-share assets)
    pub units: f64,
    /// Total cost basis for this lot
    pub cost_basis: f64,
}

/// Runtime state for the simulation, mutated as events trigger
#[derive(Debug, Clone)]
pub struct SimulationState {
    /// Current simulation date
    pub current_date: jiff::civil::Date,
    pub start_date: jiff::civil::Date,
    pub end_date: jiff::civil::Date,

    /// All accounts (keyed for fast lookup)
    pub accounts: HashMap<AccountId, Account>,

    /// Current balances per asset (separate from initial_value for mutation)
    pub asset_balances: HashMap<(AccountId, AssetId), f64>,

    /// Events that have already triggered (for `once: true` checks)
    pub triggered_events: HashMap<EventId, jiff::civil::Date>,

    /// All events
    pub events: HashMap<EventId, Event>,

    /// Next scheduled date for repeating events
    pub event_next_date: HashMap<EventId, jiff::civil::Date>,

    /// Whether repeating events have been activated (start_condition met)
    pub repeating_event_active: HashMap<EventId, bool>,

    /// Birth date for age calculations (from SimulationParameters)
    pub birth_date: Option<jiff::civil::Date>,

    /// Cost basis tracking for taxable accounts (account_id, asset_id) -> lots
    pub asset_lots: HashMap<(AccountId, AssetId), Vec<AssetLot>>,

    /// Accumulated values for event flow limits (EventId -> accumulated amount)
    pub event_flow_ytd: HashMap<EventId, f64>,
    pub event_flow_lifetime: HashMap<EventId, f64>,
    pub event_flow_last_period_key: HashMap<EventId, i16>,

    /// Sampled returns per return profile per year
    pub return_profile_returns: Vec<Vec<f64>>,

    /// Sampled inflation rates per year
    pub inflation_rates: Vec<f64>,

    /// Cumulative inflation multipliers
    pub cumulative_inflation: Vec<f64>,

    /// Year-to-date tax tracking
    pub ytd_tax: YtdTaxState,

    /// Yearly tax summaries
    pub yearly_taxes: Vec<TaxSummary>,

    /// Tax configuration
    pub tax_config: TaxConfig,

    // === Transaction Log ===
    /// Unified record of all transactions in chronological order
    pub records: Vec<Record>,

    /// Recorded dates for history
    pub dates: Vec<jiff::civil::Date>,

    // === RMD Tracking ===
    /// Year-end account balances for RMD calculation (year -> account_id -> balance)
    pub year_end_balances: HashMap<i16, HashMap<AccountId, f64>>,
    /// Active RMD accounts (account_id -> starting_age)
    pub active_rmd_accounts: HashMap<AccountId, u8>,
}

impl SimulationState {
    /// Update RMD record with actual withdrawal amount after spending target executes
    /// DEPRECATED: RMD tracking needs to be redesigned for new event system
    #[deprecated(note = "RMD tracking needs to be redesigned for new event system")]
    pub fn update_rmd_actual_withdrawn(&mut self, amount: f64) {
        // Search backwards to find most recent RMD record
        for record in self.records.iter_mut().rev() {
            if let RecordKind::Rmd {
                actual_withdrawn, ..
            } = &mut record.kind
            {
                // Track cumulative withdrawals for this RMD
                *actual_withdrawn += amount;
                break;
            }
        }
    }
}

/// Year-to-date tax tracking
#[derive(Debug, Clone, Default)]
pub struct YtdTaxState {
    pub year: i16,
    pub ordinary_income: f64,
    pub capital_gains: f64,
    pub tax_free_withdrawals: f64,
    pub federal_tax: f64,
    pub state_tax: f64,
}

impl SimulationState {
    pub fn from_parameters(params: &SimulationConfig, seed: u64) -> Self {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let start_date = params
            .start_date
            .unwrap_or_else(|| jiff::Zoned::now().date());
        let end_date = start_date.saturating_add((params.duration_years as i64).years());

        // Sample returns once per return profile
        let return_profile_returns: Vec<Vec<f64>> = params
            .return_profiles
            .iter()
            .map(|profile| {
                (0..params.duration_years)
                    .map(|_| profile.sample(&mut rng))
                    .collect()
            })
            .collect();

        // Sample inflation rates
        let inflation_rates: Vec<f64> = (0..params.duration_years)
            .map(|_| params.inflation_profile.sample(&mut rng))
            .collect();

        // Build cumulative inflation
        let mut cumulative_inflation = Vec::with_capacity(params.duration_years + 1);
        cumulative_inflation.push(1.0);
        for r in &inflation_rates {
            cumulative_inflation.push(cumulative_inflation.last().unwrap() * (1.0 + r));
        }

        let mut state = Self {
            current_date: start_date,
            start_date,
            end_date,
            accounts: HashMap::new(),
            asset_balances: HashMap::new(),
            triggered_events: HashMap::new(),
            events: HashMap::new(),
            event_next_date: HashMap::new(),
            repeating_event_active: HashMap::new(),
            birth_date: params.birth_date,
            asset_lots: HashMap::new(),
            event_flow_ytd: HashMap::new(),
            event_flow_lifetime: HashMap::new(),
            event_flow_last_period_key: HashMap::new(),
            return_profile_returns,
            inflation_rates,
            cumulative_inflation,
            ytd_tax: YtdTaxState {
                year: start_date.year(),
                ..Default::default()
            },
            yearly_taxes: Vec::new(),
            tax_config: params.tax_config.clone(),
            records: Vec::new(),
            dates: vec![start_date],
            year_end_balances: HashMap::new(),
            active_rmd_accounts: HashMap::new(),
        };

        // Load initial accounts
        for account in &params.accounts {
            for asset in &account.assets {
                state
                    .asset_balances
                    .insert((account.account_id, asset.asset_id), asset.initial_value);

                // Initialize cost basis lots for taxable accounts
                if matches!(account.account_type, crate::model::AccountType::Taxable) {
                    let cost_basis = asset.initial_cost_basis.unwrap_or(asset.initial_value);
                    let lot = AssetLot {
                        purchase_date: start_date,
                        units: asset.initial_value,
                        cost_basis,
                    };
                    state
                        .asset_lots
                        .insert((account.account_id, asset.asset_id), vec![lot]);
                }
            }
            state.accounts.insert(account.account_id, account.clone());
        }

        // Load events
        for event in &params.events {
            state.events.insert(event.event_id, event.clone());
        }

        state
    }

    /// Calculate total net worth across all accounts
    pub fn net_worth(&self) -> f64 {
        self.asset_balances.values().sum()
    }

    /// Calculate account balance
    pub fn account_balance(&self, account_id: AccountId) -> f64 {
        self.asset_balances
            .iter()
            .filter(|((acc_id, _), _)| *acc_id == account_id)
            .map(|(_, balance)| balance)
            .sum()
    }

    /// Get current balance of a specific asset
    pub fn asset_balance(&self, account_id: AccountId, asset_id: AssetId) -> f64 {
        self.asset_balances
            .get(&(account_id, asset_id))
            .copied()
            .unwrap_or(0.0)
    }

    /// Calculate total income from Transfer events in the current year
    /// This sums all Income records (External -> Asset transfers) for the current calendar year
    pub fn calculate_total_income(&self) -> f64 {
        self.records
            .iter()
            .filter(|r| r.date.year() == self.current_date.year())
            .map(|r| {
                if let RecordKind::Income { amount, .. } = r.kind {
                    amount
                } else {
                    0.0
                }
            })
            .sum()
    }

    /// Get current age in years and months
    pub fn current_age(&self) -> Option<(u8, u8)> {
        let birth = self.birth_date?;

        // Calculate age manually since jiff::Span from until() is in days only
        let mut years = self.current_date.year() - birth.year();
        let mut months = self.current_date.month() as i32 - birth.month() as i32;

        // Adjust for birthday not yet reached in current year
        if self.current_date.month() < birth.month()
            || (self.current_date.month() == birth.month() && self.current_date.day() < birth.day())
        {
            years -= 1;
            months += 12;
        }

        // Normalize months (should be 0-11)
        if months < 0 {
            months += 12;
        }

        Some((years as u8, months as u8))
    }

    /// Calculate inflation-adjusted amount
    pub fn inflation_adjusted_amount(
        &self,
        base_amount: f64,
        adjust_for_inflation: bool,
        duration_years: usize,
    ) -> f64 {
        if !adjust_for_inflation {
            return base_amount;
        }

        let years_passed = (self.current_date - self.start_date).get_days() as f64 / 365.0;
        let year_idx = (years_passed.floor() as usize).min(duration_years.saturating_sub(1));
        let fraction = years_passed - (year_idx as f64);

        if year_idx < self.inflation_rates.len() {
            let inflation_multiplier = self.cumulative_inflation[year_idx]
                * (1.0 + self.inflation_rates[year_idx]).powf(fraction);
            base_amount * inflation_multiplier
        } else {
            base_amount
        }
    }

    /// Finalize YTD taxes when year changes or simulation ends
    pub fn finalize_year_taxes(&mut self) {
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
    pub fn maybe_rollover_year(&mut self) {
        let current_year = self.current_date.year();
        if current_year != self.ytd_tax.year {
            self.finalize_year_taxes();
            self.ytd_tax = YtdTaxState {
                year: current_year,
                ..Default::default()
            };

            // Reset YTD flow accumulators (for flow limits)
            for (event_id, last_period) in self.event_flow_last_period_key.iter_mut() {
                if *last_period != current_year {
                    self.event_flow_ytd.insert(*event_id, 0.0);
                    *last_period = current_year;
                }
            }
        }
    }

    /// Build account snapshots with starting values from SimulationParameters
    pub fn build_account_snapshots(&self, params: &SimulationConfig) -> Vec<AccountSnapshot> {
        params
            .accounts
            .iter()
            .map(|account| {
                let assets = account
                    .assets
                    .iter()
                    .map(|asset| AssetSnapshot {
                        asset_id: asset.asset_id,
                        return_profile_index: asset.return_profile_index,
                        starting_value: asset.initial_value,
                    })
                    .collect();

                AccountSnapshot {
                    account_id: account.account_id,
                    account_type: account.account_type,
                    assets,
                }
            })
            .collect()
    }

    // === RMD Helper Functions ===

    /// Get prior year-end balance for an account
    pub fn prior_year_end_balance(&self, account_id: AccountId) -> Option<f64> {
        let prior_year = self.current_date.year() - 1;
        self.year_end_balances
            .get(&prior_year)?
            .get(&account_id)
            .copied()
    }

    /// Get IRS divisor for current age
    pub fn current_rmd_divisor(&self, rmd_table: &RmdTable) -> Option<f64> {
        let (years, _months) = self.current_age()?;
        rmd_table.divisor_for_age(years)
    }

    /// Calculate RMD amount for an account
    pub fn calculate_rmd_amount(&self, account_id: AccountId, rmd_table: &RmdTable) -> Option<f64> {
        let balance = self.prior_year_end_balance(account_id)?;
        let divisor = self.current_rmd_divisor(rmd_table)?;
        Some(balance / divisor)
    }
}
