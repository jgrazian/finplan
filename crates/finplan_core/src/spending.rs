//! The run-time side of `EventEffect::Spending` (spec 22).
//!
//! A [`SpendingTracker`] exists only on a path whose plan has a `Spending`
//! effect, so a plan without one runs exactly as before. It holds:
//!
//! - each effect's [`SpendingRuleState`], keyed by `(event, effect index)`;
//! - each liquid account's balance on January 1 and its market growth, per
//!   year, for last year's portfolio return `r`;
//! - the income and expenses through each pass-through account since its
//!   effect last spent;
//! - the path's decided draws, and its total spending per year, for the
//!   results.

use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::error::StateEventError;
use crate::evaluate::EvalEvent;
use crate::model::{
    AccountFlavor, AccountId, EventEffect, EventId, EventTrigger, SpendingDiagnostics,
    SpendingInputs, SpendingPortfolio, SpendingRuleState,
};
use crate::simulation_state::SimulationState;

/// An effect's identity on the path: its event and its index in the event.
pub type SpendingKey = (EventId, u16);

/// Income less expenses through a pass-through account since its effect
/// last spent, with the part that arrived on `date` kept apart: the effect's
/// first occurrence spends only what came in that day, not what built up
/// before it started.
#[derive(Debug, Clone, Copy)]
struct PassThrough {
    date: jiff::civil::Date,
    since_last: f64,
    on_date: f64,
}

#[derive(Debug, Clone)]
pub struct SpendingTracker {
    rules: FxHashMap<SpendingKey, SpendingRuleState>,
    /// Effects that have spent at least once.
    started: FxHashSet<SpendingKey>,
    pass_through: FxHashMap<AccountId, PassThrough>,
    /// Liquid balances when each year opened (the plan's start, or the
    /// previous December 31 after its events), per account.
    year_open: BTreeMap<i16, FxHashMap<AccountId, f64>>,
    /// Market growth (price moves and interest) per account, per year.
    growth: BTreeMap<i16, FxHashMap<AccountId, f64>>,
    /// The draws decided each year, real, summed over effects.
    decided: BTreeMap<i16, f64>,
    /// The draws paid, real.
    lifetime_real: f64,
    start_year: i16,
    /// Expense debits (`Expense`, `Spending`, loan payments) per year from
    /// `start_year`, nominal.
    yearly_spending: Vec<f64>,
}

/// Whether a plan's events include a `Spending` effect (at the top level,
/// the only place one runs).
#[must_use]
pub fn has_spending(events: &[crate::model::Event]) -> bool {
    events.iter().any(|event| {
        event
            .effects
            .iter()
            .any(|effect| matches!(effect, EventEffect::Spending { .. }))
    })
}

impl SpendingTracker {
    #[must_use]
    pub fn new(state: &SimulationState) -> Self {
        let start_year = state.timeline.start_date.year();
        let mut tracker = Self {
            rules: FxHashMap::default(),
            started: FxHashSet::default(),
            pass_through: FxHashMap::default(),
            year_open: BTreeMap::new(),
            growth: BTreeMap::new(),
            decided: BTreeMap::new(),
            lifetime_real: 0.0,
            start_year,
            yearly_spending: Vec::new(),
        };
        for event in state.event_state.iter_events() {
            for effect in &event.effects {
                if let EventEffect::Spending {
                    from,
                    pass_through: true,
                    ..
                } = effect
                {
                    tracker.pass_through.insert(
                        *from,
                        PassThrough {
                            date: state.timeline.start_date,
                            since_last: 0.0,
                            on_date: 0.0,
                        },
                    );
                }
            }
        }
        tracker.open_year(start_year, liquid_balances(state));
        tracker
    }

    /// Record the balances a year opens with, and forget years no rule can
    /// read any more.
    pub fn open_year(&mut self, year: i16, balances: FxHashMap<AccountId, f64>) {
        self.year_open.insert(year, balances);
        self.year_open.retain(|y, _| *y >= year - 1);
        self.growth.retain(|y, _| *y >= year - 1);
    }

    /// Market growth on `account` during `year`.
    pub fn add_growth(&mut self, year: i16, account: AccountId, amount: f64) {
        if amount != 0.0 {
            *self
                .growth
                .entry(year)
                .or_default()
                .entry(account)
                .or_default() += amount;
        }
    }

    /// An income credit (positive) or an expense debit (negative) through
    /// `account` from anything but a `Spending` effect.
    pub fn observe_flow(&mut self, account: AccountId, amount: f64, date: jiff::civil::Date) {
        if let Some(flow) = self.pass_through.get_mut(&account) {
            if flow.date != date {
                flow.date = date;
                flow.on_date = 0.0;
            }
            flow.since_last += amount;
            flow.on_date += amount;
        }
    }

    /// Money spent in `year` (an expense, a draw, a loan payment), nominal.
    pub fn add_spending(&mut self, year: i16, amount: f64) {
        let index = (year - self.start_year).max(0) as usize;
        if self.yearly_spending.len() <= index {
            self.yearly_spending.resize(index + 1, 0.0);
        }
        self.yearly_spending[index] += amount;
    }

    /// The net flow through `account` a pass-through occurrence spends.
    fn pass_through_net(
        &self,
        key: SpendingKey,
        account: AccountId,
        today: jiff::civil::Date,
    ) -> f64 {
        let Some(flow) = self.pass_through.get(&account) else {
            return 0.0;
        };
        if self.started.contains(&key) {
            flow.since_last
        } else if flow.date == today {
            flow.on_date
        } else {
            0.0
        }
    }

    /// Last year's market return on `accounts` (`None`: the first year, or
    /// nothing held on January 1).
    fn last_return(&self, year: i16, accounts: &[AccountId]) -> Option<f64> {
        let open = self.year_open.get(&(year - 1))?;
        let growth = self.growth.get(&(year - 1));
        let base: f64 = accounts.iter().filter_map(|a| open.get(a)).sum();
        if base <= 0.0 {
            return None;
        }
        let grew: f64 = accounts
            .iter()
            .filter_map(|a| growth.and_then(|g| g.get(a)))
            .sum();
        Some(grew / base)
    }

    /// Apply a spend that has been evaluated.
    fn record(&mut self, spend: &Spend, today: jiff::civil::Date) {
        if let Some(decision) = spend.decision {
            *self.decided.entry(decision.last_year).or_default() += decision.last_real;
            self.rules.insert(spend.key, decision);
        }
        self.lifetime_real += spend.draw_real;
        self.started.insert(spend.key);
        if spend.pass_through
            && let Some(flow) = self.pass_through.get_mut(&spend.from)
        {
            flow.since_last = 0.0;
            flow.on_date = 0.0;
            flow.date = today;
        }
        self.add_spending(today.year(), spend.amount);
    }

    /// The path's spending diagnostics (`None` when it never drew), and its
    /// total spending per year through `last_year`, deflated by
    /// `cumulative_inflation` the way Results deflates a year.
    #[must_use]
    pub fn finish(
        &self,
        last_year: i16,
        cumulative_inflation: &[f64],
    ) -> (Option<SpendingDiagnostics>, Vec<f64>) {
        let decided: Vec<(i16, f64)> = self.decided.iter().map(|(y, w)| (*y, *w)).collect();
        let diagnostics = SpendingDiagnostics::from_draws(&decided, self.lifetime_real);
        let years = (last_year - self.start_year + 1).max(0) as usize;
        let real = (0..years)
            .map(|index| {
                let factor = cumulative_inflation
                    .get(index)
                    .or(cumulative_inflation.last())
                    .copied()
                    .unwrap_or(1.0);
                self.yearly_spending.get(index).copied().unwrap_or(0.0) / factor
            })
            .collect();
        (diagnostics, real)
    }
}

/// Every liquid account's value now.
pub fn liquid_balances(state: &SimulationState) -> FxHashMap<AccountId, f64> {
    state
        .portfolio
        .accounts
        .iter()
        .filter(|(_, account)| account.is_liquid())
        .map(|(id, account)| {
            (
                *id,
                account.total_value(
                    &state.portfolio.market,
                    state.timeline.start_date,
                    state.timeline.current_date,
                ),
            )
        })
        .collect()
}

/// One occurrence's spend, evaluated: applied by [`apply_spend`].
#[derive(Debug, Clone)]
pub struct Spend {
    pub key: SpendingKey,
    pub from: AccountId,
    /// Debited from `from`: the draw, plus the net pass-through flow.
    pub amount: f64,
    /// The rule's state after this year's decision, when this occurrence
    /// made it.
    pub decision: Option<SpendingRuleState>,
    /// This occurrence's share of the year's draw, real.
    pub draw_real: f64,
    pub pass_through: bool,
}

fn rejected(message: &'static str) -> StateEventError {
    StateEventError::Unsupported(message)
}

/// Evaluate the `Spending` effect at `effect_index` of `event_id`.
pub fn evaluate(
    event_id: EventId,
    effect_index: usize,
    state: &SimulationState,
    out: &mut Vec<EvalEvent>,
) -> Result<(), StateEventError> {
    let event = state
        .event_state
        .get_event(event_id)
        .ok_or(rejected("a Spending effect's event is missing"))?;
    let Some(EventEffect::Spending {
        from,
        rule,
        portfolio,
        pass_through,
    }) = event.effects.get(effect_index)
    else {
        return Err(rejected("not a Spending effect"));
    };
    let EventTrigger::Repeating { interval, .. } = &event.trigger else {
        return Err(rejected(
            "a Spending effect belongs to a repeating event (monthly, quarterly or yearly)",
        ));
    };
    let per_year = interval.per_year().ok_or(rejected(
        "a Spending effect repeats monthly, quarterly or yearly",
    ))?;
    let tracker = state
        .spending
        .as_ref()
        .ok_or(rejected("spending is not tracked on this path"))?;

    let key = (event_id, effect_index as u16);
    let today = state.timeline.current_date;
    let year = today.year();
    let previous = tracker.rules.get(&key);
    let (real_draw, decision) = match previous {
        Some(rule_state) if rule_state.last_year == year => (rule_state.last_real, None),
        _ => {
            let accounts = portfolio_accounts(state, portfolio);
            let balance: f64 = accounts
                .iter()
                .filter_map(|id| state.account_balance(*id).ok())
                .sum();
            let inflation = state
                .portfolio
                .market
                .get_inflation_adjusted_value(state.timeline.start_date, today, 1.0)
                .unwrap_or(1.0);
            let inputs = SpendingInputs {
                portfolio: balance,
                last_return: tracker.last_return(year, &accounts),
                inflation,
                age: state.current_age().0,
                years_left: i32::from(state.timeline.end_date.year() - year),
                year,
            };
            let (_, next) = rule.decide(previous, &inputs);
            (next.last_real, Some(next))
        }
    };
    let nominal_draw = decision
        .or(previous.copied())
        .map_or(0.0, |s| s.last_nominal)
        / f64::from(per_year);
    let amount = if *pass_through {
        (nominal_draw + tracker.pass_through_net(key, *from, today)).max(0.0)
    } else {
        nominal_draw
    };

    out.push(EvalEvent::Spending(Spend {
        key,
        from: *from,
        amount,
        decision,
        draw_real: real_draw / f64::from(per_year),
        pass_through: *pass_through,
    }));
    Ok(())
}

/// The accounts a rule's portfolio holds now, in id order.
fn portfolio_accounts(state: &SimulationState, portfolio: &SpendingPortfolio) -> Vec<AccountId> {
    let mut ids: Vec<AccountId> = match portfolio {
        SpendingPortfolio::Liquid => state
            .portfolio
            .accounts
            .iter()
            .filter(|(_, account)| {
                matches!(
                    account.flavor,
                    AccountFlavor::Bank(_) | AccountFlavor::Investment(_)
                )
            })
            .map(|(id, _)| *id)
            .collect(),
        SpendingPortfolio::Accounts(ids) => ids
            .iter()
            .filter(|id| state.portfolio.accounts.contains_key(id))
            .copied()
            .collect(),
    };
    ids.sort_unstable();
    ids
}

/// Debit an evaluated spend from its account and record it.
pub fn apply_spend(
    state: &mut SimulationState,
    spend: &Spend,
) -> Result<f64, crate::error::ApplyError> {
    use crate::error::{AccountTypeError, ApplyError, LookupError};
    let account = state
        .portfolio
        .accounts
        .get_mut(&spend.from)
        .ok_or(ApplyError::Lookup(LookupError::AccountNotFound(spend.from)))?;
    match &mut account.flavor {
        AccountFlavor::Bank(cash) => cash.value -= spend.amount,
        AccountFlavor::Investment(inv) => inv.cash.value -= spend.amount,
        _ => {
            return Err(ApplyError::AccountType(AccountTypeError::NotACashAccount(
                spend.from,
            )));
        }
    }
    let today = state.timeline.current_date;
    if let Some(tracker) = state.spending.as_mut() {
        tracker.record(spend, today);
    }
    Ok(spend.amount)
}

/// The yearly return after inflation a `portfolio` can expect under the
/// plan's own assumptions: each holding's (and cash balance's) long-run
/// compound return, weighted by its opening value, less expected inflation.
/// What a VPW rule spends at when it names no return of its own. Zero for a
/// portfolio that holds nothing at the start.
#[must_use]
pub fn expected_real_return(
    config: &crate::config::SimulationConfig,
    portfolio: &SpendingPortfolio,
) -> f64 {
    let profile_return = |id: &crate::model::ReturnProfileId| {
        config
            .return_profiles
            .get(id)
            .map_or(0.0, crate::model::ReturnProfile::expected_geometric_return)
    };
    let included = |id: AccountId| match portfolio {
        SpendingPortfolio::Liquid => true,
        SpendingPortfolio::Accounts(ids) => ids.contains(&id),
    };
    let (mut weight, mut weighted) = (0.0, 0.0);
    let mut add = |value: f64, rate: f64| {
        if value > 0.0 {
            weight += value;
            weighted += value * rate;
        }
    };
    for account in config.accounts.iter().filter(|a| included(a.account_id)) {
        match &account.flavor {
            AccountFlavor::Bank(cash) => add(cash.value, profile_return(&cash.return_profile_id)),
            AccountFlavor::Investment(inv) => {
                add(inv.cash.value, profile_return(&inv.cash.return_profile_id));
                for lot in &inv.positions {
                    let price = config
                        .asset_prices
                        .get(&lot.asset_id)
                        .copied()
                        .unwrap_or(1.0);
                    let rate = config
                        .asset_returns
                        .get(&lot.asset_id)
                        .map_or(0.0, profile_return);
                    add(lot.units * price, rate);
                }
            }
            AccountFlavor::Property(_) | AccountFlavor::Liability(_) => {}
        }
    }
    if weight <= 0.0 {
        return 0.0;
    }
    let nominal = weighted / weight;
    let inflation = config.inflation_profile.expected_rate();
    (1.0 + nominal) / (1.0 + inflation) - 1.0
}
