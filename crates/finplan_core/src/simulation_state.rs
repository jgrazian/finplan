use crate::config::SimulationConfig;
use crate::error::{EngineError, Result};
use crate::model::{
    Account, AccountFlavor, AccountId, AssetCoord, AssetId, Event, EventId, LedgerEntry, Market,
    ReturnProfileId, RmdTable, StateEvent, TaxConfig, TaxSummary, WealthSnapshot,
};
use jiff::ToSpan;
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
    // === Contribution Tracking ===
    /// YTD contributions per account (for yearly limits)
    pub contributions_ytd: HashMap<AccountId, f64>,
    /// Current month contributions per account (for monthly limits)
    pub contributions_mtd: HashMap<AccountId, f64>,
    // === Net Worth Tracking ===
    pub wealth_snapshots: Vec<WealthSnapshot>,
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
    /// Immutable ledger of all state changes
    pub ledger: Vec<LedgerEntry>,
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

        // Extract assets from accounts and map to return profiles
        // Use configured asset_prices if available, otherwise default to $1.00 per unit
        let mut assets: HashMap<AssetId, (f64, ReturnProfileId)> = HashMap::new();
        for account in &params.accounts {
            match &account.flavor {
                AccountFlavor::Investment(inv) => {
                    for lot in &inv.positions {
                        // Use the asset_returns mapping to find the return profile
                        if let Some(&return_profile_id) = params.asset_returns.get(&lot.asset_id) {
                            // Use configured price or default to $1.00 per unit
                            let price = params
                                .asset_prices
                                .get(&lot.asset_id)
                                .copied()
                                .unwrap_or(1.0);
                            assets
                                .entry(lot.asset_id)
                                .or_insert((price, return_profile_id));
                        }
                    }
                }
                AccountFlavor::Property(fixed_assets) => {
                    for asset in fixed_assets {
                        if let Some(&return_profile_id) = params.asset_returns.get(&asset.asset_id)
                        {
                            // Property uses asset.value as the initial price
                            assets
                                .entry(asset.asset_id)
                                .or_insert((asset.value, return_profile_id));
                        }
                    }
                }
                _ => {}
            }
        }

        // Also register any assets defined in asset_returns or asset_prices
        // that may not have initial positions (for purchasing later)
        for (&asset_id, &return_profile_id) in &params.asset_returns {
            assets.entry(asset_id).or_insert_with(|| {
                // Use configured price or default to $1.00 per unit
                let price = params.asset_prices.get(&asset_id).copied().unwrap_or(1.0);
                (price, return_profile_id)
            });
        }

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
                contributions_ytd: HashMap::new(),
                contributions_mtd: HashMap::new(),
                wealth_snapshots: Vec::new(),
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
            history: SimHistory { ledger: Vec::new() },
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

    /// Calculate total income from CashCredit events in the current year
    /// This sums all CashCredit ledger entries for the current calendar year
    pub fn calculate_total_income(&self) -> f64 {
        self.history
            .ledger
            .iter()
            .filter(|entry| entry.date.year() == self.timeline.current_date.year())
            .map(|entry| {
                if let StateEvent::CashCredit { amount, .. } = &entry.event {
                    *amount
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
    pub fn snapshot_wealth(&mut self) {
        // Build snapshots from current state accounts
        let account_snapshots = self
            .portfolio
            .accounts
            .values()
            .map(|account| {
                account.snapshot(
                    &self.portfolio.market,
                    self.timeline.start_date,
                    self.timeline.current_date,
                )
            })
            .collect();

        self.portfolio.wealth_snapshots.push(WealthSnapshot {
            date: self.timeline.current_date,
            accounts: account_snapshots,
        })
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

    // === Contribution Limit Helper Functions ===

    /// Check remaining contribution room for an account
    /// Returns None if account has no contribution limit
    pub fn contribution_room(&self, account_id: AccountId) -> Result<Option<f64>> {
        use crate::model::ContributionLimitPeriod;

        let account = self
            .portfolio
            .accounts
            .get(&account_id)
            .ok_or(EngineError::AccountNotFound(account_id))?;

        if let AccountFlavor::Investment(inv) = &account.flavor
            && let Some(limit) = &inv.contribution_limit
        {
            let contributed = match limit.period {
                ContributionLimitPeriod::Monthly => self
                    .portfolio
                    .contributions_mtd
                    .get(&account_id)
                    .copied()
                    .unwrap_or(0.0),
                ContributionLimitPeriod::Yearly => self
                    .portfolio
                    .contributions_ytd
                    .get(&account_id)
                    .copied()
                    .unwrap_or(0.0),
            };
            let room = (limit.amount - contributed).max(0.0);
            return Ok(Some(room));
        }

        Ok(None) // No limit configured
    }

    /// Record a contribution and check against limits
    /// Returns the amount that can actually be contributed (may be less than requested)
    pub fn record_contribution(&mut self, account_id: AccountId, amount: f64) -> Result<f64> {
        use crate::model::ContributionLimitPeriod;

        let account = self
            .portfolio
            .accounts
            .get(&account_id)
            .ok_or(EngineError::AccountNotFound(account_id))?;

        if let AccountFlavor::Investment(inv) = &account.flavor
            && let Some(limit) = &inv.contribution_limit
        {
            // Check how much room is left
            let room = self.contribution_room(account_id)?.unwrap_or(f64::INFINITY);
            let allowed_amount = amount.min(room);

            // Record the contribution
            match limit.period {
                ContributionLimitPeriod::Monthly => {
                    *self
                        .portfolio
                        .contributions_mtd
                        .entry(account_id)
                        .or_insert(0.0) += allowed_amount;
                }
                ContributionLimitPeriod::Yearly => {
                    *self
                        .portfolio
                        .contributions_ytd
                        .entry(account_id)
                        .or_insert(0.0) += allowed_amount;
                }
            }

            return Ok(allowed_amount);
        }

        // No limit - allow full amount
        Ok(amount)
    }

    /// Reset monthly contribution trackers (call on month boundary)
    pub fn reset_monthly_contributions(&mut self) {
        self.portfolio.contributions_mtd.clear();
    }
}
