use crate::config::SimulationConfig;
use crate::error::{EngineError, Result};
use crate::model::{
    Account, AccountFlavor, AccountId, AccountSnapshot, AssetCoord, AssetId, AssetSnapshot, Event,
    EventId, Market, Record, RecordKind, ReturnProfileId, RmdTable, TaxConfig, TaxSummary,
};
use jiff::ToSpan;
use jiff::civil::Date;
use rand::SeedableRng;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct AssetPrice {
    pub price: f64,
    pub inflation_profile_id: ReturnProfileId,
}

/// Runtime state for the simulation, mutated as events trigger
#[derive(Debug, Clone)]
pub struct SimulationState {
    pub timeline: SimTimeline,
    pub portfolio: SimPortfolio,
    pub event_state: SimEventState,
    pub taxes: SimTaxState,
    pub history: SimHistory,

    /// Events pending immediate triggering (from TriggerEvent effect)
    pub pending_triggers: Vec<EventId>,
}

#[derive(Debug, Clone)]
pub struct SimTimeline {
    pub start_date: jiff::civil::Date,
    pub end_date: jiff::civil::Date,
    pub birth_date: jiff::civil::Date,
    pub current_date: jiff::civil::Date,
}

#[derive(Debug, Clone)]
pub struct SimPortfolio {
    /// All accounts (keyed for fast lookup)
    pub accounts: HashMap<AccountId, Account>,
    /// Market containing asset prices and returns
    pub market: Market,
    // === RMD Tracking ===
    /// Year-end account balances for RMD calculation (year -> account_id -> balance)
    pub year_end_balances: HashMap<i16, HashMap<AccountId, f64>>,
    /// Active RMD accounts (account_id -> starting_age)
    pub active_rmd_accounts: HashMap<AccountId, u8>,
}

#[derive(Debug, Clone)]
pub struct SimEventState {
    /// Events that have already triggered (for `once: true` checks)
    pub events: HashMap<EventId, Event>,
    pub triggered_events: HashMap<EventId, jiff::civil::Date>,
    pub event_next_date: HashMap<EventId, jiff::civil::Date>,

    /// Whether repeating events have been activated (start_condition met)
    pub repeating_event_active: HashMap<EventId, bool>,

    /// Accumulated values for event flow limits (EventId -> accumulated amount)
    pub event_flow_ytd: HashMap<EventId, f64>,
    pub event_flow_lifetime: HashMap<EventId, f64>,
    pub event_flow_last_period_key: HashMap<EventId, i16>,
}

#[derive(Debug, Clone)]
pub struct SimTaxState {
    /// Year-to-date tax tracking
    pub ytd_tax: TaxSummary,
    /// Yearly tax summaries
    pub yearly_taxes: Vec<TaxSummary>,
    /// Tax configuration
    pub config: TaxConfig,
}

#[derive(Debug, Clone)]
pub struct SimHistory {
    /// Transaction records
    pub records: Vec<Record>,
    /// Dates tracked in the simulation
    pub dates: Vec<Date>,
}

impl SimulationState {
    pub fn from_parameters(params: &SimulationConfig, seed: u64) -> Self {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let start_date = params
            .start_date
            .unwrap_or_else(|| jiff::Zoned::now().date());
        let end_date = start_date.saturating_add((params.duration_years as i64).years());

        // Build return profiles HashMap from Vec
        let return_profiles = params.return_profiles.clone();

        // Extract assets from accounts
        let assets: HashMap<AssetId, (f64, ReturnProfileId)> = HashMap::new();

        // Build Market from sampled returns using from_profiles
        let market = Market::from_profiles(
            &mut rng,
            params.duration_years,
            &params.inflation_profile,
            &return_profiles,
            &assets,
        );

        let mut accounts = HashMap::new();
        // Load initial accounts
        for account in &params.accounts {
            accounts.insert(account.account_id, account.clone());
        }

        let mut events = HashMap::new();
        // Load events
        for event in &params.events {
            events.insert(event.event_id, event.clone());
        }

        Self {
            timeline: SimTimeline {
                current_date: start_date,
                start_date,
                end_date,
                birth_date: params.birth_date.unwrap_or(jiff::civil::date(1970, 1, 1)),
            },
            portfolio: SimPortfolio {
                accounts,
                market,
                year_end_balances: HashMap::new(),
                active_rmd_accounts: HashMap::new(),
            },
            event_state: SimEventState {
                events,
                triggered_events: HashMap::new(),
                event_next_date: HashMap::new(),
                repeating_event_active: HashMap::new(),
                event_flow_ytd: HashMap::new(),
                event_flow_lifetime: HashMap::new(),
                event_flow_last_period_key: HashMap::new(),
            },
            taxes: SimTaxState {
                ytd_tax: TaxSummary {
                    year: start_date.year(),
                    ..Default::default()
                },
                yearly_taxes: Vec::new(),
                config: params.tax_config.clone(),
            },
            history: SimHistory {
                records: Vec::new(),
                dates: vec![start_date],
            },
            pending_triggers: Vec::new(),
        }
    }

    /// Calculate total net worth across all accounts
    pub fn net_worth(&self) -> f64 {
        let market = &self.portfolio.market;

        self.portfolio
            .accounts
            .values()
            .map(|acc| {
                acc.total_value(market, self.timeline.start_date, self.timeline.current_date)
            })
            .sum()
    }

    /// Calculate account balance
    pub fn account_balance(&self, account_id: AccountId) -> Result<f64> {
        let market = &self.portfolio.market;

        self.portfolio
            .accounts
            .get(&account_id)
            .map(|acc| {
                acc.total_value(market, self.timeline.start_date, self.timeline.current_date)
            })
            .ok_or(EngineError::AccountNotFound(account_id))
    }

    pub fn account_cash_balance(&self, account_id: AccountId) -> Result<f64> {
        self.portfolio
            .accounts
            .get(&account_id)
            .and_then(|acc| acc.cash_balance())
            .ok_or(EngineError::AccountNotFound(account_id))
    }

    /// Get current balance of a specific asset
    /// Uses Market prices to calculate current value from lot units
    pub fn asset_balance(&self, asset_coord: AssetCoord) -> Result<f64> {
        let account = self
            .portfolio
            .accounts
            .get(&asset_coord.account_id)
            .ok_or(EngineError::AccountNotFound(asset_coord.account_id))?;

        match &account.flavor {
            AccountFlavor::Investment(inv) => {
                // Get current price from Market
                let current_price = self
                    .portfolio
                    .market
                    .get_asset_value(
                        self.timeline.start_date,
                        self.timeline.current_date,
                        asset_coord.asset_id,
                    )
                    .unwrap_or(0.0);

                // Sum up units for this asset across all lots
                let total_units: f64 = inv
                    .positions
                    .iter()
                    .filter(|lot| lot.asset_id == asset_coord.asset_id)
                    .map(|lot| lot.units)
                    .sum();

                Ok(total_units * current_price)
            }
            AccountFlavor::Property(assets) => {
                let value = assets
                    .iter()
                    .find(|a| a.asset_id == asset_coord.asset_id)
                    .and_then(|a| {
                        self.portfolio.market.get_asset_value(
                            self.timeline.start_date,
                            self.timeline.current_date,
                            a.asset_id,
                        )
                    })
                    .unwrap_or(0.0);
                Ok(value)
            }
            _ => Err(EngineError::AssetNotFound(asset_coord)),
        }
    }

    pub fn current_asset_price(&self, asset_coord: AssetCoord) -> Result<f64> {
        self.portfolio
            .market
            .get_asset_value(
                self.timeline.start_date,
                self.timeline.current_date,
                asset_coord.asset_id,
            )
            .ok_or(EngineError::AssetNotFound(asset_coord))
    }

    /// Calculate total income from Transfer events in the current year
    /// This sums all Income records (External -> Asset transfers) for the current calendar year
    pub fn calculate_total_income(&self) -> f64 {
        self.history
            .records
            .iter()
            .filter(|r| r.date.year() == self.timeline.current_date.year())
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
    pub fn current_age(&self) -> (u8, u8) {
        // Calculate age manually since jiff::Span from until() is in days only
        let mut years = self.timeline.current_date.year() - self.timeline.birth_date.year();
        let mut months =
            self.timeline.current_date.month() as i32 - self.timeline.birth_date.month() as i32;

        // Adjust for birthday not yet reached in current year
        if self.timeline.current_date.month() < self.timeline.birth_date.month()
            || (self.timeline.current_date.month() == self.timeline.birth_date.month()
                && self.timeline.current_date.day() < self.timeline.birth_date.day())
        {
            years -= 1;
            months += 12;
        }

        // Normalize months (should be 0-11)
        if months < 0 {
            months += 12;
        }

        (years as u8, months as u8)
    }

    /// Finalize YTD taxes when year changes or simulation ends
    pub fn finalize_year_taxes(&mut self) {
        if self.taxes.ytd_tax.ordinary_income > 0.0
            || self.taxes.ytd_tax.capital_gains > 0.0
            || self.taxes.ytd_tax.tax_free_withdrawals > 0.0
        {
            let tax_summary = TaxSummary {
                year: self.taxes.ytd_tax.year,
                ordinary_income: self.taxes.ytd_tax.ordinary_income,
                capital_gains: self.taxes.ytd_tax.capital_gains,
                tax_free_withdrawals: self.taxes.ytd_tax.tax_free_withdrawals,
                federal_tax: self.taxes.ytd_tax.federal_tax,
                state_tax: self.taxes.ytd_tax.state_tax,
                total_tax: self.taxes.ytd_tax.federal_tax + self.taxes.ytd_tax.state_tax,
            };
            self.taxes.yearly_taxes.push(tax_summary);
        }
    }

    /// Check if we've crossed into a new year and finalize previous year's taxes
    pub fn maybe_rollover_year(&mut self) {
        let current_year = self.timeline.current_date.year();
        if current_year != self.taxes.ytd_tax.year {
            self.finalize_year_taxes();
            self.taxes.ytd_tax = TaxSummary {
                year: current_year,
                ..Default::default()
            };

            // Reset YTD flow accumulators (for flow limits)
            for (event_id, last_period) in self.event_state.event_flow_last_period_key.iter_mut() {
                if *last_period != current_year {
                    self.event_state.event_flow_ytd.insert(*event_id, 0.0);
                    *last_period = current_year;
                }
            }
        }
    }

    /// Build account snapshots with starting values from SimulationParameters
    pub fn build_account_snapshots(&self, _params: &SimulationConfig) -> Vec<AccountSnapshot> {
        // Build snapshots from current state accounts
        self.portfolio
            .accounts
            .values()
            .map(|account| {
                let assets = match &account.flavor {
                    AccountFlavor::Investment(inv) => {
                        // Get unique asset_ids from positions
                        let mut asset_ids: std::collections::HashSet<AssetId> =
                            std::collections::HashSet::new();
                        for lot in &inv.positions {
                            asset_ids.insert(lot.asset_id);
                        }

                        asset_ids
                            .iter()
                            .map(|&asset_id| {
                                // Sum up cost basis as starting value
                                let starting_value: f64 = inv
                                    .positions
                                    .iter()
                                    .filter(|lot| lot.asset_id == asset_id)
                                    .map(|lot| lot.cost_basis)
                                    .sum();

                                AssetSnapshot {
                                    asset_id,
                                    return_profile_index: 0, // TODO: lookup from Market
                                    starting_value,
                                }
                            })
                            .collect()
                    }
                    AccountFlavor::Property(assets) => assets
                        .iter()
                        .map(|asset| AssetSnapshot {
                            asset_id: asset.asset_id,
                            return_profile_index: 0,
                            starting_value: asset.value,
                        })
                        .collect(),
                    _ => Vec::new(),
                };

                AccountSnapshot {
                    account_id: account.account_id,
                    flavor: account.flavor.clone(),
                    assets,
                }
            })
            .collect()
    }

    // === RMD Helper Functions ===

    /// Get prior year-end balance for an account
    pub fn prior_year_end_balance(&self, account_id: AccountId) -> Option<f64> {
        let prior_year = self.timeline.current_date.year() - 1;
        self.portfolio
            .year_end_balances
            .get(&prior_year)?
            .get(&account_id)
            .copied()
    }

    /// Get IRS divisor for current age
    pub fn current_rmd_divisor(&self, rmd_table: &RmdTable) -> Option<f64> {
        let (years, _months) = self.current_age();
        rmd_table.divisor_for_age(years)
    }

    /// Calculate RMD amount for an account
    pub fn calculate_rmd_amount(&self, account_id: AccountId, rmd_table: &RmdTable) -> Option<f64> {
        let balance = self.prior_year_end_balance(account_id)?;
        let divisor = self.current_rmd_divisor(rmd_table)?;
        Some(balance / divisor)
    }
}
