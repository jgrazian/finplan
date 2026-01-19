use std::collections::HashMap;

use finplan_core::{
    config::SimulationConfig,
    model::{
        Account, AccountFlavor, AccountId, AmountMode, AssetCoord, AssetId, AssetLot,
        BalanceThreshold, Cash, Event, EventEffect, EventId, EventTrigger, FixedAsset, IncomeType,
        InvestmentContainer, LoanDetail, LotMethod, MonteCarloResult, RepeatInterval,
        ReturnProfileId, TaxStatus, TransferAmount, TriggerOffset, WithdrawalOrder,
        WithdrawalSources,
    },
};
use jiff::civil::Date;

use crate::state::{MonteCarloStats, MonteCarloStoredResult};

use super::{
    app_data::SimulationData,
    events_data::{
        AccountTag, AmountData, EffectData, EventTag, IntervalData, LotMethodData, OffsetData,
        SpecialAmount, ThresholdData, TriggerData, WithdrawalStrategyData,
    },
    parameters_data::ParametersData,
    portfolio_data::{AccountData, AccountType, AssetTag},
};

#[derive(Debug, Clone)]
pub enum ConvertError {
    InvalidDate(String),
    AccountNotFound(String),
    AssetNotFound(String, String),
    EventNotFound(String),
    ProfileNotFound(String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::InvalidDate(s) => write!(f, "Invalid date format: {}", s),
            ConvertError::AccountNotFound(s) => write!(f, "Account not found: {}", s),
            ConvertError::AssetNotFound(acc, asset) => {
                write!(f, "Asset '{}' not found in account '{}'", asset, acc)
            }
            ConvertError::EventNotFound(s) => write!(f, "Event not found: {}", s),
            ConvertError::ProfileNotFound(s) => write!(f, "Return profile not found: {}", s),
        }
    }
}

impl std::error::Error for ConvertError {}

/// Context for resolving string references to IDs
struct ResolveContext {
    account_ids: HashMap<String, AccountId>,
    asset_ids: HashMap<(String, String), (AccountId, AssetId)>, // (account_name, asset_name) -> (AccountId, AssetId)
    event_ids: HashMap<String, EventId>,
    profile_ids: HashMap<String, ReturnProfileId>,
    /// Property/Collectible assets with their return profiles: account_name -> (AssetId, ReturnProfileTag)
    property_assets: HashMap<String, (AssetId, Option<String>)>,
}

/// Convert SimulationData (human-readable YAML) to SimulationConfig (engine format)
pub fn to_simulation_config(data: &SimulationData) -> Result<SimulationConfig, ConvertError> {
    let mut config = SimulationConfig::new();

    // Build ID maps
    let ctx = build_resolve_context(data);

    // Convert parameters
    convert_parameters(&data.parameters, &mut config)?;

    // Convert return profiles
    convert_profiles(data, &ctx, &mut config)?;

    // Convert accounts
    convert_accounts(data, &ctx, &mut config)?;

    // Build asset_returns and asset_prices maps
    build_asset_mappings(data, &ctx, &mut config)?;

    // Convert events
    convert_events(data, &ctx, &mut config)?;

    Ok(config)
}

fn build_resolve_context(data: &SimulationData) -> ResolveContext {
    let mut account_ids = HashMap::new();
    let mut asset_ids = HashMap::new();
    let mut event_ids = HashMap::new();
    let mut profile_ids = HashMap::new();
    let mut property_assets = HashMap::new();

    // Track the next available asset ID (start high to avoid collision with investment assets)
    let mut next_property_asset_id: u16 = 1000;

    // Assign account IDs
    for (idx, account) in data.portfolios.accounts.iter().enumerate() {
        let id = AccountId((idx + 1) as u16);
        account_ids.insert(account.name.clone(), id);

        // Track assets within accounts
        match &account.account_type {
            AccountType::Brokerage(inv)
            | AccountType::Traditional401k(inv)
            | AccountType::Roth401k(inv)
            | AccountType::TraditionalIRA(inv)
            | AccountType::RothIRA(inv) => {
                for (asset_idx, asset_val) in inv.assets.iter().enumerate() {
                    let asset_id = AssetId((asset_idx + 1) as u16);
                    asset_ids.insert(
                        (account.name.clone(), asset_val.asset.0.clone()),
                        (id, asset_id),
                    );
                }
            }
            // Track Property/Collectible assets with their return profiles
            AccountType::Property(prop) | AccountType::Collectible(prop) => {
                let asset_id = AssetId(next_property_asset_id);
                next_property_asset_id += 1;
                property_assets.insert(
                    account.name.clone(),
                    (asset_id, prop.return_profile.as_ref().map(|rp| rp.0.clone())),
                );
            }
            _ => {}
        }
    }

    // Assign profile IDs
    for (idx, profile) in data.profiles.iter().enumerate() {
        let id = ReturnProfileId((idx + 1) as u16);
        profile_ids.insert(profile.name.0.clone(), id);
    }

    // Assign event IDs
    for (idx, event) in data.events.iter().enumerate() {
        let id = EventId((idx + 1) as u16);
        event_ids.insert(event.name.0.clone(), id);
    }

    ResolveContext {
        account_ids,
        asset_ids,
        event_ids,
        profile_ids,
        property_assets,
    }
}

fn convert_parameters(
    params: &ParametersData,
    config: &mut SimulationConfig,
) -> Result<(), ConvertError> {
    config.birth_date = Some(parse_date(&params.birth_date)?);
    config.start_date = Some(parse_date(&params.start_date)?);
    config.duration_years = params.duration_years;
    config.inflation_profile = params.inflation.to_inflation_profile();
    config.tax_config = params.tax_config.to_tax_config();
    Ok(())
}

fn convert_profiles(
    data: &SimulationData,
    ctx: &ResolveContext,
    config: &mut SimulationConfig,
) -> Result<(), ConvertError> {
    for profile_data in &data.profiles {
        if let Some(&id) = ctx.profile_ids.get(&profile_data.name.0) {
            config
                .return_profiles
                .insert(id, profile_data.profile.to_return_profile());
        }
    }
    Ok(())
}

fn convert_accounts(
    data: &SimulationData,
    ctx: &ResolveContext,
    config: &mut SimulationConfig,
) -> Result<(), ConvertError> {
    // We need a default return profile for cash
    let default_cash_profile = ReturnProfileId(0);

    for account_data in &data.portfolios.accounts {
        let account_id = *ctx
            .account_ids
            .get(&account_data.name)
            .ok_or_else(|| ConvertError::AccountNotFound(account_data.name.clone()))?;

        let flavor = convert_account_flavor(account_data, ctx, default_cash_profile)?;

        config.accounts.push(Account { account_id, flavor });
    }
    Ok(())
}

fn convert_account_flavor(
    account_data: &AccountData,
    ctx: &ResolveContext,
    default_cash_profile: ReturnProfileId,
) -> Result<AccountFlavor, ConvertError> {
    match &account_data.account_type {
        AccountType::Checking(prop) | AccountType::Savings(prop) | AccountType::HSA(prop) => {
            let return_profile_id = prop
                .return_profile
                .as_ref()
                .and_then(|rp| ctx.profile_ids.get(&rp.0).copied())
                .unwrap_or(default_cash_profile);

            Ok(AccountFlavor::Bank(Cash {
                value: prop.value,
                return_profile_id,
            }))
        }

        AccountType::Property(prop) | AccountType::Collectible(prop) => {
            // Use the AssetId assigned in build_resolve_context
            let asset_id = ctx
                .property_assets
                .get(&account_data.name)
                .map(|(id, _)| *id)
                .unwrap_or(AssetId(0));
            Ok(AccountFlavor::Property(vec![FixedAsset {
                asset_id,
                value: prop.value,
            }]))
        }

        AccountType::Mortgage(debt)
        | AccountType::LoanDebt(debt)
        | AccountType::StudentLoanDebt(debt) => Ok(AccountFlavor::Liability(LoanDetail {
            principal: debt.balance,
            interest_rate: debt.interest_rate,
        })),

        AccountType::Brokerage(inv) => Ok(AccountFlavor::Investment(convert_investment_container(
            inv,
            &account_data.name,
            ctx,
            TaxStatus::Taxable,
            default_cash_profile,
        )?)),

        AccountType::Traditional401k(inv) | AccountType::TraditionalIRA(inv) => {
            Ok(AccountFlavor::Investment(convert_investment_container(
                inv,
                &account_data.name,
                ctx,
                TaxStatus::TaxDeferred,
                default_cash_profile,
            )?))
        }

        AccountType::Roth401k(inv) | AccountType::RothIRA(inv) => {
            Ok(AccountFlavor::Investment(convert_investment_container(
                inv,
                &account_data.name,
                ctx,
                TaxStatus::TaxFree,
                default_cash_profile,
            )?))
        }
    }
}

fn convert_investment_container(
    inv: &super::portfolio_data::AssetAccount,
    account_name: &str,
    ctx: &ResolveContext,
    tax_status: TaxStatus,
    default_cash_profile: ReturnProfileId,
) -> Result<InvestmentContainer, ConvertError> {
    let positions: Vec<AssetLot> = inv
        .assets
        .iter()
        .filter_map(|av| {
            ctx.asset_ids
                .get(&(account_name.to_string(), av.asset.0.clone()))
                .map(|(_, asset_id)| AssetLot {
                    asset_id: *asset_id,
                    purchase_date: Date::constant(2020, 1, 1), // Default purchase date
                    units: av.value / 100.0, // Assume $100 per unit as placeholder
                    cost_basis: av.value,
                })
        })
        .collect();

    Ok(InvestmentContainer {
        tax_status,
        cash: Cash {
            value: 0.0,
            return_profile_id: default_cash_profile,
        },
        positions,
        contribution_limit: None,
    })
}

fn build_asset_mappings(
    data: &SimulationData,
    ctx: &ResolveContext,
    config: &mut SimulationConfig,
) -> Result<(), ConvertError> {
    // Build asset_returns map from the assets HashMap in SimulationData
    for (asset_tag, profile_tag) in &data.assets {
        if let Some(&profile_id) = ctx.profile_ids.get(&profile_tag.0) {
            // Find all instances of this asset across accounts
            for ((_, asset_name), (_, asset_id)) in &ctx.asset_ids {
                if asset_name == &asset_tag.0 {
                    config.asset_returns.insert(*asset_id, profile_id);
                    // Set a default price
                    config.asset_prices.insert(*asset_id, 100.0);
                }
            }
        }
    }

    // Register Property/Collectible assets with their return profiles
    for account in &data.portfolios.accounts {
        if let AccountType::Property(prop) | AccountType::Collectible(prop) = &account.account_type
        {
            if let Some((asset_id, Some(profile_name))) = ctx.property_assets.get(&account.name) {
                if let Some(&profile_id) = ctx.profile_ids.get(profile_name) {
                    config.asset_returns.insert(*asset_id, profile_id);
                    // Use the property's value as the initial price
                    config.asset_prices.insert(*asset_id, prop.value);
                }
            }
        }
    }

    Ok(())
}

fn convert_events(
    data: &SimulationData,
    ctx: &ResolveContext,
    config: &mut SimulationConfig,
) -> Result<(), ConvertError> {
    for event_data in &data.events {
        let event_id = *ctx
            .event_ids
            .get(&event_data.name.0)
            .ok_or_else(|| ConvertError::EventNotFound(event_data.name.0.clone()))?;

        let trigger = convert_trigger(&event_data.trigger, ctx)?;
        let effects: Result<Vec<EventEffect>, ConvertError> = event_data
            .effects
            .iter()
            .map(|e| convert_effect(e, ctx))
            .collect();

        config.events.push(Event {
            event_id,
            trigger,
            effects: effects?,
            once: event_data.once,
        });
    }
    Ok(())
}

fn convert_trigger(
    trigger: &TriggerData,
    ctx: &ResolveContext,
) -> Result<EventTrigger, ConvertError> {
    match trigger {
        TriggerData::Date { date } => Ok(EventTrigger::Date(parse_date(date)?)),

        TriggerData::Age { years, months } => Ok(EventTrigger::Age {
            years: *years,
            months: *months,
        }),

        TriggerData::RelativeToEvent { event, offset } => {
            let event_id = resolve_event(event, ctx)?;
            Ok(EventTrigger::RelativeToEvent {
                event_id,
                offset: convert_offset(offset),
            })
        }

        TriggerData::AccountBalance { account, threshold } => {
            let account_id = resolve_account(account, ctx)?;
            Ok(EventTrigger::AccountBalance {
                account_id,
                threshold: convert_threshold(threshold),
            })
        }

        TriggerData::AssetBalance {
            account,
            asset,
            threshold,
        } => {
            let asset_coord = resolve_asset(account, asset, ctx)?;
            Ok(EventTrigger::AssetBalance {
                asset_coord,
                threshold: convert_threshold(threshold),
            })
        }

        TriggerData::NetWorth { threshold } => Ok(EventTrigger::NetWorth {
            threshold: convert_threshold(threshold),
        }),

        TriggerData::And { conditions } => {
            let triggers: Result<Vec<EventTrigger>, ConvertError> =
                conditions.iter().map(|t| convert_trigger(t, ctx)).collect();
            Ok(EventTrigger::And(triggers?))
        }

        TriggerData::Or { conditions } => {
            let triggers: Result<Vec<EventTrigger>, ConvertError> =
                conditions.iter().map(|t| convert_trigger(t, ctx)).collect();
            Ok(EventTrigger::Or(triggers?))
        }

        TriggerData::Repeating {
            interval,
            start,
            end,
        } => {
            let start_condition = match start {
                Some(t) => Some(Box::new(convert_trigger(t, ctx)?)),
                None => None,
            };
            let end_condition = match end {
                Some(t) => Some(Box::new(convert_trigger(t, ctx)?)),
                None => None,
            };

            Ok(EventTrigger::Repeating {
                interval: convert_interval(interval),
                start_condition,
                end_condition,
            })
        }

        TriggerData::Manual => Ok(EventTrigger::Manual),
    }
}

fn convert_effect(effect: &EffectData, ctx: &ResolveContext) -> Result<EventEffect, ConvertError> {
    match effect {
        EffectData::Income {
            to,
            amount,
            gross,
            taxable,
        } => {
            let to_id = resolve_account(to, ctx)?;
            Ok(EventEffect::Income {
                to: to_id,
                amount: convert_amount(amount),
                amount_mode: if *gross {
                    AmountMode::Gross
                } else {
                    AmountMode::Net
                },
                income_type: if *taxable {
                    IncomeType::Taxable
                } else {
                    IncomeType::TaxFree
                },
            })
        }

        EffectData::Expense { from, amount } => {
            let from_id = resolve_account(from, ctx)?;
            Ok(EventEffect::Expense {
                from: from_id,
                amount: convert_amount(amount),
            })
        }

        EffectData::AssetPurchase {
            from,
            to_account,
            asset,
            amount,
        } => {
            let from_id = resolve_account(from, ctx)?;
            let to_coord = resolve_asset(to_account, asset, ctx)?;
            Ok(EventEffect::AssetPurchase {
                from: from_id,
                to: to_coord,
                amount: convert_amount(amount),
            })
        }

        EffectData::AssetSale {
            from,
            asset,
            amount,
            gross,
            lot_method,
        } => {
            let from_id = resolve_account(from, ctx)?;
            let asset_id = asset.as_ref().map(|a| {
                ctx.asset_ids
                    .get(&(from.0.clone(), a.0.clone()))
                    .map(|(_, id)| *id)
                    .unwrap_or(AssetId(0))
            });

            Ok(EventEffect::AssetSale {
                from: from_id,
                asset_id,
                amount: convert_amount(amount),
                amount_mode: if *gross {
                    AmountMode::Gross
                } else {
                    AmountMode::Net
                },
                lot_method: convert_lot_method(lot_method),
            })
        }

        EffectData::Sweep {
            to,
            amount,
            strategy,
            gross,
            taxable,
            lot_method,
            exclude_accounts,
        } => {
            let to_id = resolve_account(to, ctx)?;
            let exclude: Vec<AccountId> = exclude_accounts
                .iter()
                .filter_map(|a| ctx.account_ids.get(&a.0).copied())
                .collect();

            Ok(EventEffect::Sweep {
                sources: WithdrawalSources::Strategy {
                    order: convert_withdrawal_strategy(strategy),
                    exclude_accounts: exclude,
                },
                to: to_id,
                amount: convert_amount(amount),
                amount_mode: if *gross {
                    AmountMode::Gross
                } else {
                    AmountMode::Net
                },
                lot_method: convert_lot_method(lot_method),
                income_type: if *taxable {
                    IncomeType::Taxable
                } else {
                    IncomeType::TaxFree
                },
            })
        }

        EffectData::TriggerEvent { event } => {
            let event_id = resolve_event(event, ctx)?;
            Ok(EventEffect::TriggerEvent(event_id))
        }

        EffectData::PauseEvent { event } => {
            let event_id = resolve_event(event, ctx)?;
            Ok(EventEffect::PauseEvent(event_id))
        }

        EffectData::ResumeEvent { event } => {
            let event_id = resolve_event(event, ctx)?;
            Ok(EventEffect::ResumeEvent(event_id))
        }

        EffectData::TerminateEvent { event } => {
            let event_id = resolve_event(event, ctx)?;
            Ok(EventEffect::TerminateEvent(event_id))
        }

        EffectData::ApplyRmd {
            destination,
            lot_method,
        } => {
            let dest_id = resolve_account(destination, ctx)?;
            Ok(EventEffect::ApplyRmd {
                destination: dest_id,
                lot_method: convert_lot_method(lot_method),
            })
        }
    }
}

// Helper functions

fn parse_date(s: &str) -> Result<Date, ConvertError> {
    s.parse::<Date>()
        .map_err(|_| ConvertError::InvalidDate(s.to_string()))
}

fn resolve_account(tag: &AccountTag, ctx: &ResolveContext) -> Result<AccountId, ConvertError> {
    ctx.account_ids
        .get(&tag.0)
        .copied()
        .ok_or_else(|| ConvertError::AccountNotFound(tag.0.clone()))
}

fn resolve_asset(
    account: &AccountTag,
    asset: &AssetTag,
    ctx: &ResolveContext,
) -> Result<AssetCoord, ConvertError> {
    ctx.asset_ids
        .get(&(account.0.clone(), asset.0.clone()))
        .map(|(acc_id, asset_id)| AssetCoord {
            account_id: *acc_id,
            asset_id: *asset_id,
        })
        .ok_or_else(|| ConvertError::AssetNotFound(account.0.clone(), asset.0.clone()))
}

fn resolve_event(tag: &EventTag, ctx: &ResolveContext) -> Result<EventId, ConvertError> {
    ctx.event_ids
        .get(&tag.0)
        .copied()
        .ok_or_else(|| ConvertError::EventNotFound(tag.0.clone()))
}

fn convert_amount(amount: &AmountData) -> TransferAmount {
    match amount {
        AmountData::Fixed(v) => TransferAmount::Fixed(*v),
        AmountData::Special(special) => match special {
            SpecialAmount::SourceBalance => TransferAmount::SourceBalance,
            SpecialAmount::ZeroTargetBalance => TransferAmount::ZeroTargetBalance,
            SpecialAmount::TargetToBalance { target } => TransferAmount::TargetToBalance(*target),
            SpecialAmount::AccountBalance { account: _ } => TransferAmount::AccountTotalBalance {
                account_id: AccountId(0), // Would need proper resolution
            },
            SpecialAmount::AccountCashBalance { account: _ } => {
                TransferAmount::AccountCashBalance {
                    account_id: AccountId(0), // Would need proper resolution
                }
            }
        },
    }
}

fn convert_offset(offset: &OffsetData) -> TriggerOffset {
    match offset {
        OffsetData::Days { value } => TriggerOffset::Days(*value),
        OffsetData::Months { value } => TriggerOffset::Months(*value),
        OffsetData::Years { value } => TriggerOffset::Years(*value),
    }
}

fn convert_threshold(threshold: &ThresholdData) -> BalanceThreshold {
    match threshold {
        ThresholdData::GreaterThanOrEqual { value } => BalanceThreshold::GreaterThanOrEqual(*value),
        ThresholdData::LessThanOrEqual { value } => BalanceThreshold::LessThanOrEqual(*value),
    }
}

fn convert_interval(interval: &IntervalData) -> RepeatInterval {
    match interval {
        IntervalData::Never => RepeatInterval::Never,
        IntervalData::Weekly => RepeatInterval::Weekly,
        IntervalData::BiWeekly => RepeatInterval::BiWeekly,
        IntervalData::Monthly => RepeatInterval::Monthly,
        IntervalData::Quarterly => RepeatInterval::Quarterly,
        IntervalData::Yearly => RepeatInterval::Yearly,
    }
}

fn convert_lot_method(method: &LotMethodData) -> LotMethod {
    match method {
        LotMethodData::Fifo => LotMethod::Fifo,
        LotMethodData::Lifo => LotMethod::Lifo,
        LotMethodData::HighestCost => LotMethod::HighestCost,
        LotMethodData::LowestCost => LotMethod::LowestCost,
        LotMethodData::AverageCost => LotMethod::AverageCost,
    }
}

fn convert_withdrawal_strategy(strategy: &WithdrawalStrategyData) -> WithdrawalOrder {
    match strategy {
        WithdrawalStrategyData::TaxEfficient => WithdrawalOrder::TaxEfficientEarly,
        WithdrawalStrategyData::TaxDeferredFirst => WithdrawalOrder::TaxDeferredFirst,
        WithdrawalStrategyData::TaxFreeFirst => WithdrawalOrder::TaxFreeFirst,
        WithdrawalStrategyData::ProRata => WithdrawalOrder::ProRata,
    }
}

/// Convert core SimulationResult to TUI SimulationResult
pub fn to_tui_result(
    core_result: &finplan_core::model::SimulationResult,
    birth_date: &str,
    start_date: &str,
) -> Result<crate::state::SimulationResult, ConvertError> {
    use finplan_core::model::StateEvent;
    use std::collections::BTreeMap;

    let birth = parse_date(birth_date)?;
    let _start = parse_date(start_date)?;

    // Group ledger entries by year to compute yearly totals
    let mut yearly_income: BTreeMap<i32, f64> = BTreeMap::new();
    let mut yearly_expenses: BTreeMap<i32, f64> = BTreeMap::new();

    for entry in &core_result.ledger {
        let year = entry.date.year() as i32;

        match &entry.event {
            StateEvent::CashCredit { amount, .. } => {
                *yearly_income.entry(year).or_default() += amount;
            }
            StateEvent::CashDebit { amount, .. } => {
                *yearly_expenses.entry(year).or_default() += amount;
            }
            _ => {}
        }
    }

    // Get yearly taxes
    let yearly_taxes: BTreeMap<i32, f64> = core_result
        .yearly_taxes
        .iter()
        .map(|t| (t.year as i32, t.total_tax))
        .collect();

    // Build yearly net worth from wealth snapshots
    // Take the last snapshot of each year (snapshots are in chronological order)
    let mut yearly_net_worth: BTreeMap<i32, f64> = BTreeMap::new();
    let mut all_years_set: std::collections::HashSet<i32> = std::collections::HashSet::new();

    for snapshot in &core_result.wealth_snapshots {
        let year = snapshot.date.year() as i32;
        let total: f64 = snapshot.accounts.iter().map(|acc| acc.total_value()).sum();
        yearly_net_worth.insert(year, total); // Last value for each year wins
        all_years_set.insert(year);
    }

    let mut all_years: Vec<i32> = all_years_set.into_iter().collect();
    all_years.sort();

    // Calculate final net worth
    let final_net_worth: f64 = core_result.wealth_snapshots.last().map_or(0.0, |snap| {
        snap.accounts.iter().map(|acc| acc.total_value()).sum()
    });

    // Build year results
    let mut years = Vec::new();

    for year in &all_years {
        // Calculate age at this year
        let years_since_birth = year - birth.year() as i32;
        let age = years_since_birth.max(0) as u8;

        // Use actual year-end net worth from simulation results
        let net_worth = yearly_net_worth
            .get(year)
            .copied()
            .unwrap_or(final_net_worth);

        years.push(crate::state::YearResult {
            year: *year,
            age,
            net_worth,
            income: *yearly_income.get(year).unwrap_or(&0.0),
            expenses: *yearly_expenses.get(year).unwrap_or(&0.0),
            taxes: *yearly_taxes.get(year).unwrap_or(&0.0),
        });
    }

    Ok(crate::state::SimulationResult {
        final_net_worth,
        years,
    })
}

/// Process Monte Carlo results and extract the 4 representative runs + stats
/// P5, P50, P95 are actual simulation runs; Mean is a synthetic result with
/// averaged values at each year across all iterations.
pub fn process_monte_carlo_results(
    mc_result: &MonteCarloResult,
    birth_date: &str,
    start_date: &str,
) -> Result<MonteCarloStoredResult, ConvertError> {
    use finplan_core::model::{AccountSnapshot, AccountSnapshotFlavor, WealthSnapshot};

    let num_iterations = mc_result.iterations.len();
    if num_iterations == 0 {
        return Err(ConvertError::InvalidDate(
            "No iterations in Monte Carlo result".to_string(),
        ));
    }

    // Convert all iterations to TUI format for averaging
    let tui_results: Vec<crate::state::SimulationResult> = mc_result
        .iterations
        .iter()
        .map(|core| to_tui_result(core, birth_date, start_date))
        .collect::<Result<Vec<_>, _>>()?;

    // Calculate final net worth for each iteration and sort
    let mut indexed_results: Vec<(usize, f64)> = tui_results
        .iter()
        .enumerate()
        .map(|(idx, result)| (idx, result.final_net_worth))
        .collect();

    indexed_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Calculate statistics
    let final_values: Vec<f64> = indexed_results.iter().map(|(_, v)| *v).collect();
    let mean_final_net_worth: f64 = final_values.iter().sum::<f64>() / num_iterations as f64;

    let variance: f64 = final_values
        .iter()
        .map(|v| (v - mean_final_net_worth).powi(2))
        .sum::<f64>()
        / num_iterations as f64;
    let std_dev_final_net_worth = variance.sqrt();

    let min_final_net_worth = final_values.first().copied().unwrap_or(0.0);
    let max_final_net_worth = final_values.last().copied().unwrap_or(0.0);

    // Calculate percentile indices
    let p5_idx = ((num_iterations as f64 * 0.05).floor() as usize).min(num_iterations - 1);
    let p50_idx = ((num_iterations as f64 * 0.50).floor() as usize).min(num_iterations - 1);
    let p95_idx = ((num_iterations as f64 * 0.95).floor() as usize).min(num_iterations - 1);

    // Get the original indices for percentile runs
    let p5_original_idx = indexed_results[p5_idx].0;
    let p50_original_idx = indexed_results[p50_idx].0;
    let p95_original_idx = indexed_results[p95_idx].0;

    // Calculate success rate (iterations with positive final net worth)
    let success_count = final_values.iter().filter(|v| **v > 0.0).count();
    let success_rate = success_count as f64 / num_iterations as f64;

    // Get percentile values
    let p5_final_net_worth = indexed_results[p5_idx].1;
    let p50_final_net_worth = indexed_results[p50_idx].1;
    let p95_final_net_worth = indexed_results[p95_idx].1;

    // Clone the representative core results for P5, P50, P95
    let p5_core = mc_result.iterations[p5_original_idx].clone();
    let p50_core = mc_result.iterations[p50_original_idx].clone();
    let p95_core = mc_result.iterations[p95_original_idx].clone();

    // Get TUI results for percentile runs
    let p5_result = tui_results[p5_original_idx].clone();
    let p50_result = tui_results[p50_original_idx].clone();
    let p95_result = tui_results[p95_original_idx].clone();

    // === Compute TRUE MEAN: average values at each year across all iterations ===

    // Use the first result as a template for year structure
    let template = &tui_results[0];
    let num_years = template.years.len();

    // Compute per-year averages
    let mut mean_years = Vec::with_capacity(num_years);
    for year_idx in 0..num_years {
        let year = template.years[year_idx].year;
        let age = template.years[year_idx].age;

        // Sum values across all iterations for this year
        let mut sum_net_worth = 0.0;
        let mut sum_income = 0.0;
        let mut sum_expenses = 0.0;
        let mut sum_taxes = 0.0;
        let mut count = 0;

        for tui_result in &tui_results {
            if let Some(yr) = tui_result.years.get(year_idx) {
                sum_net_worth += yr.net_worth;
                sum_income += yr.income;
                sum_expenses += yr.expenses;
                sum_taxes += yr.taxes;
                count += 1;
            }
        }

        let n = count as f64;
        mean_years.push(crate::state::YearResult {
            year,
            age,
            net_worth: sum_net_worth / n,
            income: sum_income / n,
            expenses: sum_expenses / n,
            taxes: sum_taxes / n,
        });
    }

    let mean_result = crate::state::SimulationResult {
        final_net_worth: mean_final_net_worth,
        years: mean_years,
    };

    // === Create synthetic core result for mean view ===
    // Use P50's structure as template, but with averaged account values

    let p50_core_ref = &mc_result.iterations[p50_original_idx];

    // Average wealth snapshots: for each snapshot index, average account values
    let mut mean_wealth_snapshots = Vec::new();
    let num_snapshots = p50_core_ref.wealth_snapshots.len();

    for snap_idx in 0..num_snapshots {
        let template_snap = &p50_core_ref.wealth_snapshots[snap_idx];

        // Average each account's value across all iterations
        let mut mean_accounts = Vec::new();
        for (acc_idx, template_acc) in template_snap.accounts.iter().enumerate() {
            let mut sum_value = 0.0;
            let mut count = 0;

            for iteration in &mc_result.iterations {
                if let Some(snap) = iteration.wealth_snapshots.get(snap_idx) {
                    if let Some(acc) = snap.accounts.get(acc_idx) {
                        sum_value += acc.total_value();
                        count += 1;
                    }
                }
            }

            let avg_value = if count > 0 {
                sum_value / count as f64
            } else {
                0.0
            };

            // Create averaged account snapshot (simplified to Bank flavor for display)
            // This preserves the account_id but stores averaged total value
            let mean_flavor = match &template_acc.flavor {
                AccountSnapshotFlavor::Bank(_) => AccountSnapshotFlavor::Bank(avg_value),
                AccountSnapshotFlavor::Investment { .. } => {
                    // Store as bank with total value for simplicity in mean view
                    AccountSnapshotFlavor::Bank(avg_value)
                }
                AccountSnapshotFlavor::Property(_) => AccountSnapshotFlavor::Property(avg_value),
                AccountSnapshotFlavor::Liability(_) => AccountSnapshotFlavor::Liability(avg_value),
            };

            mean_accounts.push(AccountSnapshot {
                account_id: template_acc.account_id,
                flavor: mean_flavor,
            });
        }

        mean_wealth_snapshots.push(WealthSnapshot {
            date: template_snap.date,
            accounts: mean_accounts,
        });
    }

    // Average yearly taxes
    let mut mean_yearly_taxes = Vec::new();
    let num_tax_years = p50_core_ref.yearly_taxes.len();

    for tax_idx in 0..num_tax_years {
        let template_tax = &p50_core_ref.yearly_taxes[tax_idx];

        let mut sum_ordinary = 0.0;
        let mut sum_cap_gains = 0.0;
        let mut sum_tax_free = 0.0;
        let mut sum_federal = 0.0;
        let mut sum_state = 0.0;
        let mut sum_total = 0.0;
        let mut count = 0;

        for iteration in &mc_result.iterations {
            if let Some(tax) = iteration.yearly_taxes.get(tax_idx) {
                sum_ordinary += tax.ordinary_income;
                sum_cap_gains += tax.capital_gains;
                sum_tax_free += tax.tax_free_withdrawals;
                sum_federal += tax.federal_tax;
                sum_state += tax.state_tax;
                sum_total += tax.total_tax;
                count += 1;
            }
        }

        let n = count as f64;
        mean_yearly_taxes.push(finplan_core::model::TaxSummary {
            year: template_tax.year,
            ordinary_income: sum_ordinary / n,
            capital_gains: sum_cap_gains / n,
            tax_free_withdrawals: sum_tax_free / n,
            federal_tax: sum_federal / n,
            state_tax: sum_state / n,
            total_tax: sum_total / n,
        });
    }

    // Create synthetic mean core result (empty ledger - doesn't make sense for averages)
    let mean_core = finplan_core::model::SimulationResult {
        wealth_snapshots: mean_wealth_snapshots,
        yearly_taxes: mean_yearly_taxes,
        ledger: Vec::new(), // No meaningful ledger for averaged results
    };

    let stats = MonteCarloStats {
        num_iterations,
        success_rate,
        mean_final_net_worth,
        std_dev_final_net_worth,
        min_final_net_worth,
        max_final_net_worth,
        p5_final_net_worth,
        p50_final_net_worth,
        p95_final_net_worth,
    };

    Ok(MonteCarloStoredResult {
        stats,
        p5_result,
        p50_result,
        p95_result,
        mean_result,
        p5_core,
        p50_core,
        p95_core,
        mean_core,
    })
}
