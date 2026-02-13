use std::collections::HashMap;

use finplan_core::{
    config::SimulationConfig,
    model::{
        Account, AccountFlavor, AccountId, AmountMode, AssetCoord, AssetId, AssetLot,
        BalanceThreshold, Cash, Event, EventEffect, EventId, EventTrigger, FixedAsset, IncomeType,
        InvestmentContainer, LoanDetail, LotMethod, RepeatInterval, ReturnProfileId, TaxStatus,
        TransferAmount, TriggerOffset, WithdrawalOrder, WithdrawalSources,
    },
};
use jiff::civil::Date;

use super::{
    app_data::SimulationData,
    events_data::{
        AccountTag, AmountData, EffectData, EventTag, IntervalData, LotMethodData, OffsetData,
        ThresholdData, TriggerData, WithdrawalStrategyData,
    },
    parameters_data::{ParametersData, ReturnsMode},
    portfolio_data::{AccountData, AccountType, AssetTag},
    profiles_data::ReturnProfileData,
    ticker_profiles::HISTORICAL_PRESETS,
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
                    (
                        asset_id,
                        prop.return_profile.as_ref().map(|rp| rp.0.clone()),
                    ),
                );
            }
            _ => {}
        }
    }

    // Assign profile IDs based on mode
    match data.parameters.returns_mode {
        ReturnsMode::Historical => {
            // In Historical mode, assign IDs to preset profiles
            for (idx, (_, display_name, _)) in HISTORICAL_PRESETS.iter().enumerate() {
                let id = ReturnProfileId((idx + 1) as u16);
                profile_ids.insert(display_name.to_string(), id);
            }
        }
        ReturnsMode::Parametric => {
            // In Parametric mode, assign IDs to user-defined profiles
            for (idx, profile) in data.profiles.iter().enumerate() {
                let id = ReturnProfileId((idx + 1) as u16);
                profile_ids.insert(profile.name.0.clone(), id);
            }
        }
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
    use finplan_core::model::InflationProfile;

    config.birth_date = Some(parse_date(&params.birth_date)?);
    config.start_date = Some(parse_date(&params.start_date)?);
    config.duration_years = params.duration_years;

    // Log the returns mode being used for debugging
    tracing::info!(
        returns_mode = ?params.returns_mode,
        historical_block_size = ?params.historical_block_size,
        duration_years = params.duration_years,
        "Converting simulation parameters"
    );

    // In Historical mode, always use historical bootstrap inflation with the same block size
    config.inflation_profile = match params.returns_mode {
        ReturnsMode::Historical => {
            InflationProfile::us_historical_bootstrap(params.historical_block_size)
        }
        ReturnsMode::Parametric => params.inflation.to_inflation_profile(),
    };

    config.tax_config = params.tax_config.to_tax_config();
    Ok(())
}

fn convert_profiles(
    data: &SimulationData,
    ctx: &ResolveContext,
    config: &mut SimulationConfig,
) -> Result<(), ConvertError> {
    match data.parameters.returns_mode {
        ReturnsMode::Historical => {
            // Generate historical profiles at runtime
            let block_size = data.parameters.historical_block_size;
            for (preset_key, display_name, _) in HISTORICAL_PRESETS {
                if let Some(&id) = ctx.profile_ids.get(*display_name) {
                    let profile_data = ReturnProfileData::Bootstrap {
                        preset: preset_key.to_string(),
                    };
                    config.return_profiles.insert(
                        id,
                        profile_data.to_return_profile_with_block_size(block_size),
                    );
                }
            }
        }
        ReturnsMode::Parametric => {
            // Use user-defined profiles
            for profile_data in &data.profiles {
                if let Some(&id) = ctx.profile_ids.get(&profile_data.name.0) {
                    config
                        .return_profiles
                        .insert(id, profile_data.profile.to_return_profile());
                }
            }
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

        let flavor =
            convert_account_flavor(account_data, ctx, default_cash_profile, &data.asset_prices)?;

        config.accounts.push(Account { account_id, flavor });
    }
    Ok(())
}

fn convert_account_flavor(
    account_data: &AccountData,
    ctx: &ResolveContext,
    default_cash_profile: ReturnProfileId,
    asset_prices: &HashMap<AssetTag, f64>,
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
            Ok(AccountFlavor::Property(FixedAsset {
                asset_id,
                value: prop.value,
            }))
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
            asset_prices,
        )?)),

        AccountType::Traditional401k(inv) | AccountType::TraditionalIRA(inv) => {
            Ok(AccountFlavor::Investment(convert_investment_container(
                inv,
                &account_data.name,
                ctx,
                TaxStatus::TaxDeferred,
                default_cash_profile,
                asset_prices,
            )?))
        }

        AccountType::Roth401k(inv) | AccountType::RothIRA(inv) => {
            Ok(AccountFlavor::Investment(convert_investment_container(
                inv,
                &account_data.name,
                ctx,
                TaxStatus::TaxFree,
                default_cash_profile,
                asset_prices,
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
    asset_prices: &HashMap<AssetTag, f64>,
) -> Result<InvestmentContainer, ConvertError> {
    let positions: Vec<AssetLot> = inv
        .assets
        .iter()
        .filter_map(|av| {
            let price = asset_prices.get(&av.asset).copied().unwrap_or(100.0);
            ctx.asset_ids
                .get(&(account_name.to_string(), av.asset.0.clone()))
                .map(|(_, asset_id)| AssetLot {
                    asset_id: *asset_id,
                    purchase_date: Date::constant(2020, 1, 1), // Default purchase date
                    units: av.value / price,
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
    // Select the appropriate asset mappings based on mode
    let asset_mappings = match data.parameters.returns_mode {
        ReturnsMode::Historical => &data.historical_assets,
        ReturnsMode::Parametric => &data.assets,
    };

    // Build asset_returns map from the selected assets HashMap
    // Sort asset_mappings for deterministic iteration order
    let mut sorted_mappings: Vec<_> = asset_mappings.iter().collect();
    sorted_mappings.sort_by(|a, b| a.0.0.cmp(&b.0.0));

    for (asset_tag, profile_tag) in sorted_mappings {
        if let Some(&profile_id) = ctx.profile_ids.get(&profile_tag.0) {
            // Find all instances of this asset across accounts
            // Sort asset_ids for deterministic iteration order
            let mut sorted_asset_ids: Vec<_> = ctx.asset_ids.iter().collect();
            sorted_asset_ids.sort_by(|a, b| a.0.cmp(b.0));

            for ((_, asset_name), (_, asset_id)) in sorted_asset_ids {
                if asset_name == &asset_tag.0 {
                    config.asset_returns.insert(*asset_id, profile_id);
                    let price = data.asset_prices.get(asset_tag).copied().unwrap_or(100.0);
                    config.asset_prices.insert(*asset_id, price);
                    if let Some(&te) = data.asset_tracking_errors.get(asset_tag) {
                        config.asset_tracking_errors.insert(*asset_id, te);
                    }
                }
            }
        }
    }

    // Register Property/Collectible assets with their return profiles
    for account in &data.portfolios.accounts {
        if let AccountType::Property(prop) | AccountType::Collectible(prop) = &account.account_type
            && let Some((asset_id, Some(profile_name))) = ctx.property_assets.get(&account.name)
            && let Some(&profile_id) = ctx.profile_ids.get(profile_name)
        {
            config.asset_returns.insert(*asset_id, profile_id);
            // Use the property's value as the initial price
            config.asset_prices.insert(*asset_id, prop.value);
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
        // Skip disabled events
        if !event_data.enabled {
            continue;
        }

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
            max_occurrences,
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
                max_occurrences: *max_occurrences,
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
                amount: convert_amount(amount, ctx),
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
                amount: convert_amount(amount, ctx),
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
                amount: convert_amount(amount, ctx),
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
                amount: convert_amount(amount, ctx),
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
                amount: convert_amount(amount, ctx),
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

        EffectData::AdjustBalance { account, amount } => {
            let account_id = resolve_account(account, ctx)?;
            Ok(EventEffect::AdjustBalance {
                account: account_id,
                amount: convert_amount(amount, ctx),
            })
        }

        EffectData::CashTransfer { from, to, amount } => {
            let from_id = resolve_account(from, ctx)?;
            let to_id = resolve_account(to, ctx)?;
            Ok(EventEffect::CashTransfer {
                from: from_id,
                to: to_id,
                amount: convert_amount(amount, ctx),
            })
        }

        EffectData::Random {
            probability,
            on_true,
            on_false,
        } => {
            let on_true_id = resolve_event(on_true, ctx)?;
            let on_false_effect = match on_false {
                Some(event_tag) => {
                    let event_id = resolve_event(event_tag, ctx)?;
                    Some(Box::new(EventEffect::TriggerEvent(event_id)))
                }
                None => None,
            };
            Ok(EventEffect::Random {
                probability: *probability,
                on_true: Box::new(EventEffect::TriggerEvent(on_true_id)),
                on_false: on_false_effect,
            })
        }

        EffectData::RsuVesting {
            to,
            asset,
            units,
            sell_to_cover,
            lot_method,
        } => {
            let to_id = resolve_account(to, ctx)?;
            let asset_coord = resolve_asset(to, asset, ctx)?;
            Ok(EventEffect::RsuVesting {
                to: to_id,
                asset: asset_coord,
                units: *units,
                sell_to_cover: *sell_to_cover,
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

fn convert_amount(amount: &AmountData, ctx: &ResolveContext) -> TransferAmount {
    match amount {
        AmountData::Fixed { value } => TransferAmount::Fixed(*value),
        AmountData::InflationAdjusted { inner } => {
            TransferAmount::InflationAdjusted(Box::new(convert_amount(inner, ctx)))
        }
        AmountData::Scale { multiplier, inner } => {
            TransferAmount::Scale(*multiplier, Box::new(convert_amount(inner, ctx)))
        }
        AmountData::SourceBalance => TransferAmount::SourceBalance,
        AmountData::ZeroTargetBalance => TransferAmount::ZeroTargetBalance,
        AmountData::TargetToBalance { target } => TransferAmount::TargetToBalance(*target),
        AmountData::AccountBalance { account } => {
            let account_id = ctx
                .account_ids
                .get(&account.0)
                .copied()
                .unwrap_or(AccountId(0));
            TransferAmount::AccountTotalBalance { account_id }
        }
        AmountData::AccountCashBalance { account } => {
            let account_id = ctx
                .account_ids
                .get(&account.0)
                .copied()
                .unwrap_or(AccountId(0));
            TransferAmount::AccountCashBalance { account_id }
        }
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
        WithdrawalStrategyData::PenaltyAware => WithdrawalOrder::PenaltyAware,
    }
}

/// Convert core SimulationResult to TUI SimulationResult
///
/// Uses the pre-computed yearly_cash_flows from the core simulation which
/// properly categorizes income, expenses, and withdrawals using CashFlowKind.
///
/// Also computes inflation-adjusted (real) values for display in "today's dollars".
pub fn to_tui_result(
    core_result: &finplan_core::model::SimulationResult,
    birth_date: &str,
    start_date: &str,
) -> Result<crate::state::SimulationResult, ConvertError> {
    use std::collections::BTreeMap;

    let birth = parse_date(birth_date)?;
    let start = parse_date(start_date)?;

    // Use pre-computed yearly cash flows from core simulation
    let yearly_income: BTreeMap<i32, f64> = core_result
        .yearly_cash_flows
        .iter()
        .map(|cf| (cf.year as i32, cf.income))
        .collect();

    let yearly_expenses: BTreeMap<i32, f64> = core_result
        .yearly_cash_flows
        .iter()
        .map(|cf| (cf.year as i32, cf.expenses))
        .collect();

    let yearly_withdrawals: BTreeMap<i32, f64> = core_result
        .yearly_cash_flows
        .iter()
        .map(|cf| (cf.year as i32, cf.withdrawals))
        .collect();

    let yearly_contributions: BTreeMap<i32, f64> = core_result
        .yearly_cash_flows
        .iter()
        .map(|cf| (cf.year as i32, cf.contributions))
        .collect();

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

    // Get cumulative inflation factors for real value calculations
    let inflation_factors = &core_result.cumulative_inflation;
    let start_year = start.year() as i32;

    // Helper to get inflation factor for a given year
    let get_inflation_factor = |year: i32| -> f64 {
        let year_index = (year - start_year).max(0) as usize;
        inflation_factors
            .get(year_index)
            .copied()
            .unwrap_or_else(|| {
                // If beyond available data, use the last factor
                inflation_factors.last().copied().unwrap_or(1.0)
            })
    };

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

        let income = *yearly_income.get(year).unwrap_or(&0.0);
        let expenses = *yearly_expenses.get(year).unwrap_or(&0.0);

        // Calculate inflation factor for this year
        let inflation_factor = get_inflation_factor(*year);

        // Calculate real (inflation-adjusted) values
        let real_net_worth = if inflation_factor > 0.0 {
            net_worth / inflation_factor
        } else {
            net_worth
        };
        let real_income = if inflation_factor > 0.0 {
            income / inflation_factor
        } else {
            income
        };
        let real_expenses = if inflation_factor > 0.0 {
            expenses / inflation_factor
        } else {
            expenses
        };

        years.push(crate::state::YearResult {
            year: *year,
            age,
            net_worth,
            income,
            expenses,
            withdrawals: *yearly_withdrawals.get(year).unwrap_or(&0.0),
            contributions: *yearly_contributions.get(year).unwrap_or(&0.0),
            taxes: *yearly_taxes.get(year).unwrap_or(&0.0),
            real_net_worth,
            real_income,
            real_expenses,
        });
    }

    // Calculate final real net worth
    let final_year = all_years.last().copied().unwrap_or(start_year);
    let final_inflation_factor = get_inflation_factor(final_year);
    let final_real_net_worth = if final_inflation_factor > 0.0 {
        final_net_worth / final_inflation_factor
    } else {
        final_net_worth
    };

    Ok(crate::state::SimulationResult {
        final_net_worth,
        final_real_net_worth,
        years,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::app_data::SimulationData;
    use crate::data::parameters_data::ReturnsMode;
    use crate::data::portfolio_data::{
        AccountData, AccountType, AssetAccount, AssetTag, AssetValue, Property,
    };
    use crate::data::profiles_data::ReturnProfileTag;
    use rand::RngCore;
    use std::collections::HashMap;

    /// Test that to_simulation_config produces deterministic results regardless of
    /// HashMap insertion order in historical_assets. This guards against non-determinism
    /// from HashMap iteration order which varies across process invocations.
    #[test]
    fn test_asset_mapping_determinism() {
        // Create a minimal simulation data structure
        let mut data = SimulationData::default();

        // Set up historical mode
        data.parameters.returns_mode = ReturnsMode::Historical;
        data.parameters.historical_block_size = Some(5);
        data.parameters.birth_date = "1980-01-01".to_string();
        data.parameters.start_date = "2024-01-01".to_string();
        data.parameters.duration_years = 30;

        // Add accounts with various assets
        data.portfolios.accounts = vec![
            AccountData {
                name: "Checking".to_string(),
                description: None,
                account_type: AccountType::Checking(Property {
                    value: 10000.0,
                    return_profile: None,
                }),
            },
            AccountData {
                name: "Brokerage".to_string(),
                description: None,
                account_type: AccountType::Brokerage(AssetAccount {
                    assets: vec![
                        AssetValue {
                            asset: AssetTag("VFIAX".to_string()),
                            value: 100000.0,
                        },
                        AssetValue {
                            asset: AssetTag("VTSAX".to_string()),
                            value: 50000.0,
                        },
                        AssetValue {
                            asset: AssetTag("BND".to_string()),
                            value: 25000.0,
                        },
                    ],
                }),
            },
        ];

        // Test with different HashMap insertion orders for historical_assets
        // Order A: VFIAX, VTSAX, BND
        let mut historical_a = HashMap::new();
        historical_a.insert(
            AssetTag("VFIAX".to_string()),
            ReturnProfileTag("S&P 500".to_string()),
        );
        historical_a.insert(
            AssetTag("VTSAX".to_string()),
            ReturnProfileTag("S&P 500".to_string()),
        );
        historical_a.insert(
            AssetTag("BND".to_string()),
            ReturnProfileTag("US Agg Bonds".to_string()),
        );

        // Order B: BND, VFIAX, VTSAX (different insertion order)
        let mut historical_b = HashMap::new();
        historical_b.insert(
            AssetTag("BND".to_string()),
            ReturnProfileTag("US Agg Bonds".to_string()),
        );
        historical_b.insert(
            AssetTag("VFIAX".to_string()),
            ReturnProfileTag("S&P 500".to_string()),
        );
        historical_b.insert(
            AssetTag("VTSAX".to_string()),
            ReturnProfileTag("S&P 500".to_string()),
        );

        // Order C: VTSAX, BND, VFIAX (yet another order)
        let mut historical_c = HashMap::new();
        historical_c.insert(
            AssetTag("VTSAX".to_string()),
            ReturnProfileTag("S&P 500".to_string()),
        );
        historical_c.insert(
            AssetTag("BND".to_string()),
            ReturnProfileTag("US Agg Bonds".to_string()),
        );
        historical_c.insert(
            AssetTag("VFIAX".to_string()),
            ReturnProfileTag("S&P 500".to_string()),
        );

        // Convert with order A
        let mut data_a = data.clone();
        data_a.historical_assets = historical_a;
        let config_a = to_simulation_config(&data_a).expect("Config A should succeed");

        // Convert with order B
        let mut data_b = data.clone();
        data_b.historical_assets = historical_b;
        let config_b = to_simulation_config(&data_b).expect("Config B should succeed");

        // Convert with order C
        let mut data_c = data;
        data_c.historical_assets = historical_c;
        let config_c = to_simulation_config(&data_c).expect("Config C should succeed");

        // Verify asset_returns mappings are identical
        assert_eq!(
            config_a.asset_returns.len(),
            config_b.asset_returns.len(),
            "asset_returns length differs between A and B"
        );
        assert_eq!(
            config_a.asset_returns.len(),
            config_c.asset_returns.len(),
            "asset_returns length differs between A and C"
        );

        for (asset_id, profile_id_a) in &config_a.asset_returns {
            let profile_id_b = config_b
                .asset_returns
                .get(asset_id)
                .expect("Asset should exist in config B");
            let profile_id_c = config_c
                .asset_returns
                .get(asset_id)
                .expect("Asset should exist in config C");

            assert_eq!(
                profile_id_a, profile_id_b,
                "Profile ID for asset {:?} differs between A and B",
                asset_id
            );
            assert_eq!(
                profile_id_a, profile_id_c,
                "Profile ID for asset {:?} differs between A and C",
                asset_id
            );
        }

        // Verify return_profiles are identical
        assert_eq!(
            config_a.return_profiles.len(),
            config_b.return_profiles.len(),
            "return_profiles length differs between A and B"
        );

        for profile_id in config_a.return_profiles.keys() {
            assert!(
                config_b.return_profiles.contains_key(profile_id),
                "Profile {:?} missing from config B",
                profile_id
            );
            assert!(
                config_c.return_profiles.contains_key(profile_id),
                "Profile {:?} missing from config C",
                profile_id
            );
        }

        // Verify that running simulations with same seed produces identical results
        use finplan_core::simulation_state::SimulationState;
        use rand::SeedableRng;

        let mut rng_a = rand::rngs::SmallRng::seed_from_u64(42);
        let mut rng_b = rand::rngs::SmallRng::seed_from_u64(42);
        let mut rng_c = rand::rngs::SmallRng::seed_from_u64(42);

        let state_a =
            SimulationState::from_parameters(&config_a, rng_a.next_u64()).expect("State A");
        let state_b =
            SimulationState::from_parameters(&config_b, rng_b.next_u64()).expect("State B");
        let state_c =
            SimulationState::from_parameters(&config_c, rng_c.next_u64()).expect("State C");

        // Check that initial portfolio values are identical
        let start_date = state_a.timeline.start_date;
        let current_date = state_a.timeline.current_date;

        let total_a: f64 = state_a
            .portfolio
            .accounts
            .values()
            .map(|acc| acc.total_value(&state_a.portfolio.market, start_date, current_date))
            .sum();
        let total_b: f64 = state_b
            .portfolio
            .accounts
            .values()
            .map(|acc| acc.total_value(&state_b.portfolio.market, start_date, current_date))
            .sum();
        let total_c: f64 = state_c
            .portfolio
            .accounts
            .values()
            .map(|acc| acc.total_value(&state_c.portfolio.market, start_date, current_date))
            .sum();

        assert!(
            (total_a - total_b).abs() < 0.01,
            "Initial portfolio value differs between A ({}) and B ({})",
            total_a,
            total_b
        );
        assert!(
            (total_a - total_c).abs() < 0.01,
            "Initial portfolio value differs between A ({}) and C ({})",
            total_a,
            total_c
        );
    }
}
