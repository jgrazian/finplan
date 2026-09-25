//! Lower a stored scenario graph into a `finplan_core::config::SimulationConfig`.
//!
//! This is the one place that knows both representations. Everything the engine
//! sees is built here; nothing engine-shaped is ever persisted.

pub mod idmap;
pub mod rows;

use std::collections::{HashMap, HashSet};

use finplan_core::config::SimulationConfig;
use finplan_core::config::SimulationMetadata;
use finplan_core::expression::compile_amount;
use finplan_core::model::{
    Account, AccountFlavor, AmountMode, AssetCoord, AssetLot, BalanceThreshold, CalendarAge, Cash,
    ContributionLimit, ContributionLimitPeriod, Event, EventEffect, EventTrigger, FixedAsset,
    HistoricalInflation, HistoricalReturns, IncomeType, InflationProfile, InvestmentContainer,
    LoanDetail, LotMethod, ParameterValue, RepeatInterval, ReturnProfile, TaxBracket, TaxConfig,
    TaxStatus, TransferAmount, TriggerOffset, WithdrawalOrder, WithdrawalSources,
};
use jiff::civil::Date;

use crate::error::{ApiError, ApiResult};
use idmap::IdMap;
use rows::{DistributionRow, ScenarioGraph};

/// A scenario lowered for the engine, plus the id map needed to read its output.
pub struct CompiledScenario {
    pub config: SimulationConfig,
    pub id_map: IdMap,
    pub metadata: SimulationMetadata,
    /// Display names keyed by database account id, for labelling result series.
    pub account_names: HashMap<i64, String>,
    /// Display names keyed by database event id, for labelling ledger entries.
    pub event_names: HashMap<i64, String>,
}

fn parse_date(text: &str, field: &str) -> ApiResult<Date> {
    text.parse::<Date>()
        .map_err(|e| ApiError::unprocessable(format!("invalid {field} '{text}': {e}")))
}

/// Guard against a cycle in a self-referential table sending the compiler into
/// unbounded recursion. Depth is generous; real expressions nest a handful deep.
const MAX_DEPTH: usize = 64;

/// The id the synthesised flat-zero profile is interned under. Negative, so it
/// can never collide with a `return_profiles` row.
const UNMAPPED_PROFILE: i64 = -1;

fn depth_check(depth: usize, what: &str) -> ApiResult<()> {
    if depth > MAX_DEPTH {
        return Err(ApiError::unprocessable(format!(
            "{what} nests deeper than {MAX_DEPTH} levels, or contains a cycle"
        )));
    }
    Ok(())
}

/// Only the names, IDs, and typed values needed to inspect expressions. This
/// remains available when an unrelated event or account makes a full plan
/// uncompileable, so parameter editing can help repair that plan.
pub fn expression_context(
    graph: &ScenarioGraph,
) -> ApiResult<(
    IdMap,
    SimulationMetadata,
    HashMap<finplan_core::model::ParameterId, ParameterValue>,
)> {
    let mut ids = IdMap::new();
    let mut metadata = SimulationMetadata::new();
    for row in &graph.assets {
        metadata.register_asset(
            ids.intern_asset(row.id)?,
            Some(row.name.clone()),
            row.description.clone(),
        );
    }
    for row in &graph.accounts {
        metadata.register_account(
            ids.intern_account(row.id)?,
            Some(row.name.clone()),
            row.description.clone(),
        );
    }
    let mut parameters = HashMap::new();
    for row in &graph.parameters {
        let id = ids.intern_parameter(row.id)?;
        metadata.register_parameter(id, Some(row.name.clone()), None);
        let invalid =
            || ApiError::unprocessable(format!("parameter '{}' has an invalid value", row.name));
        let value = match row.kind.as_str() {
            "Money" => ParameterValue::Money(row.number_value.ok_or_else(invalid)?),
            "Rate" => ParameterValue::Rate(row.number_value.ok_or_else(invalid)?),
            "Date" => ParameterValue::Date(parse_date(
                row.date_value.as_deref().ok_or_else(invalid)?,
                "parameter date",
            )?),
            "Age" => ParameterValue::Age(CalendarAge::new(
                u8::try_from(row.age_years.ok_or_else(invalid)?).map_err(|_| invalid())?,
                u8::try_from(row.age_months.ok_or_else(invalid)?).map_err(|_| invalid())?,
            )),
            _ => return Err(invalid()),
        };
        if !value.is_valid() {
            return Err(invalid());
        }
        parameters.insert(id, value);
    }
    Ok((ids, metadata, parameters))
}

pub fn compile(graph: &ScenarioGraph) -> ApiResult<CompiledScenario> {
    let mut id_map = IdMap::new();

    let start_date = parse_date(&graph.scenario.start_date, "start_date")?;
    let birth_date = graph
        .scenario
        .birth_date
        .as_deref()
        .map(|d| parse_date(d, "birth_date"))
        .transpose()?;

    // ── Pass 1: intern every entity so later passes can resolve references ──
    // Order is stable (sort_order, id) so the dense indices are deterministic
    // across compiles of an unchanged scenario, which keeps seeded runs
    // reproducible.
    for asset in &graph.assets {
        id_map.intern_asset(asset.id)?;
    }
    for account in &graph.accounts {
        id_map.intern_account(account.id)?;
    }
    for event in &graph.events {
        if event.enabled != 0 {
            id_map.intern_event(event.id)?;
        }
    }
    for parameter in &graph.parameters {
        id_map.intern_parameter(parameter.id)?;
    }

    let mut metadata = SimulationMetadata::new();
    for row in &graph.accounts {
        metadata.register_account(
            id_map.account(row.id)?,
            Some(row.name.clone()),
            row.description.clone(),
        );
    }
    for row in &graph.assets {
        metadata.register_asset(
            id_map.asset(row.id)?,
            Some(row.name.clone()),
            row.description.clone(),
        );
    }
    for row in &graph.parameters {
        metadata.register_parameter(id_map.parameter(row.id)?, Some(row.name.clone()), None);
    }
    let mut parameters = HashMap::new();
    for row in &graph.parameters {
        let invalid =
            || ApiError::unprocessable(format!("parameter '{}' has an invalid value", row.name));
        let value = match row.kind.as_str() {
            "Money" => ParameterValue::Money(row.number_value.ok_or_else(invalid)?),
            "Rate" => ParameterValue::Rate(row.number_value.ok_or_else(invalid)?),
            "Date" => ParameterValue::Date(parse_date(
                row.date_value.as_deref().ok_or_else(invalid)?,
                "parameter date",
            )?),
            "Age" => ParameterValue::Age(CalendarAge::new(
                u8::try_from(row.age_years.ok_or_else(invalid)?).map_err(|_| invalid())?,
                u8::try_from(row.age_months.ok_or_else(invalid)?).map_err(|_| invalid())?,
            )),
            _ => return Err(invalid()),
        };
        if !value.is_valid() {
            return Err(invalid());
        }
        parameters.insert(id_map.parameter(row.id)?, value);
    }

    // ── Return profiles: intern only those the scenario actually references ──
    let mut referenced_profiles: Vec<i64> = Vec::new();
    let mut seen = HashSet::new();
    let note = |id: i64, out: &mut Vec<i64>, seen: &mut HashSet<i64>| {
        if seen.insert(id) {
            out.push(id);
        }
    };

    for asset in &graph.assets {
        if let Some(id) = asset.return_profile_id {
            note(id, &mut referenced_profiles, &mut seen);
        }
    }
    for bank in graph.bank.values() {
        note(bank.return_profile_id, &mut referenced_profiles, &mut seen);
    }
    for inv in graph.investment.values() {
        note(
            inv.cash_return_profile_id,
            &mut referenced_profiles,
            &mut seen,
        );
    }
    referenced_profiles.sort_unstable();

    let mut return_profiles = HashMap::new();
    for db_id in referenced_profiles {
        let profile_row = graph.return_profiles.get(&db_id).ok_or_else(|| {
            ApiError::unprocessable(format!("return profile {db_id} does not exist"))
        })?;
        let dense = id_map.intern_profile(db_id)?;
        let profile = build_return_profile(graph, profile_row.distribution_id, 0)?;
        return_profiles.insert(dense, profile);
    }

    // ── Assets: prices, profile assignment, tracking error ─────────────────
    let mut asset_prices = HashMap::new();
    let mut asset_returns = HashMap::new();
    let mut asset_tracking_errors = HashMap::new();
    for asset in &graph.assets {
        let dense = id_map.asset(asset.id)?;
        asset_prices.insert(dense, asset.initial_price);
        // An unmapped asset has a price but nothing driving it. Rather than
        // refuse the run, every one of them shares a single synthesised profile
        // that returns zero — the asset holds its opening price for the whole
        // simulation. Interned under a sentinel id, since no row can hold one.
        let returns = match asset.return_profile_id {
            Some(profile_id) => id_map.profile(profile_id)?,
            None => {
                let profile = id_map.intern_profile(UNMAPPED_PROFILE)?;
                return_profiles
                    .entry(profile)
                    .or_insert(ReturnProfile::None);
                profile
            }
        };
        asset_returns.insert(dense, returns);
        if let Some(te) = asset.tracking_error
            && te > 0.0
        {
            asset_tracking_errors.insert(dense, te);
        }
    }

    // ── Accounts ────────────────────────────────────────────────────────────
    let mut accounts = Vec::with_capacity(graph.accounts.len());
    let mut account_names = HashMap::new();
    for row in &graph.accounts {
        account_names.insert(row.id, row.name.clone());
        let account_id = id_map.account(row.id)?;

        let flavor = match row.flavor.as_str() {
            "Bank" => {
                let bank = graph.bank.get(&row.id).ok_or_else(|| {
                    ApiError::unprocessable(format!(
                        "account '{}' is a Bank account but has no bank detail row",
                        row.name
                    ))
                })?;
                AccountFlavor::Bank(Cash {
                    value: bank.cash_value,
                    return_profile_id: id_map.profile(bank.return_profile_id)?,
                })
            }
            "Investment" => {
                let inv = graph.investment.get(&row.id).ok_or_else(|| {
                    ApiError::unprocessable(format!(
                        "account '{}' is an Investment account but has no investment detail row",
                        row.name
                    ))
                })?;

                let tax_status = match inv.tax_status.as_str() {
                    "Taxable" => TaxStatus::Taxable,
                    "TaxDeferred" => TaxStatus::TaxDeferred,
                    "TaxFree" => TaxStatus::TaxFree,
                    other => {
                        return Err(ApiError::unprocessable(format!(
                            "unknown tax status '{other}'"
                        )));
                    }
                };

                let contribution_limit = match (inv.contribution_limit, &inv.contribution_period) {
                    (Some(amount), Some(period)) => Some(ContributionLimit {
                        amount,
                        period: match period.as_str() {
                            "Monthly" => ContributionLimitPeriod::Monthly,
                            "Yearly" => ContributionLimitPeriod::Yearly,
                            other => {
                                return Err(ApiError::unprocessable(format!(
                                    "unknown contribution period '{other}'"
                                )));
                            }
                        },
                    }),
                    _ => None,
                };

                let mut positions = Vec::new();
                for lot in graph.positions.get(&row.id).into_iter().flatten() {
                    positions.push(AssetLot {
                        asset_id: id_map.asset(lot.asset_id)?,
                        purchase_date: parse_date(&lot.purchase_date, "purchase_date")?,
                        units: lot.units,
                        cost_basis: lot.cost_basis,
                    });
                }

                AccountFlavor::Investment(InvestmentContainer {
                    tax_status,
                    cash: Cash {
                        value: inv.cash_value,
                        return_profile_id: id_map.profile(inv.cash_return_profile_id)?,
                    },
                    positions,
                    contribution_limit,
                })
            }
            "Property" => {
                let prop = graph.property.get(&row.id).ok_or_else(|| {
                    ApiError::unprocessable(format!(
                        "account '{}' is a Property account but has no property detail row",
                        row.name
                    ))
                })?;
                AccountFlavor::Property(FixedAsset {
                    asset_id: id_map.asset(prop.asset_id)?,
                    value: prop.value,
                })
            }
            "Liability" => {
                let loan = graph.liability.get(&row.id).ok_or_else(|| {
                    ApiError::unprocessable(format!(
                        "account '{}' is a Liability but has no liability detail row",
                        row.name
                    ))
                })?;
                AccountFlavor::Liability(LoanDetail {
                    principal: loan.principal,
                    interest_rate: loan.interest_rate,
                })
            }
            other => {
                return Err(ApiError::unprocessable(format!(
                    "unknown account flavor '{other}'"
                )));
            }
        };

        accounts.push(Account { account_id, flavor });
    }

    // ── Events ──────────────────────────────────────────────────────────────
    let mut events = Vec::new();
    // Names cover disabled events too: an earlier run's ledger can still be on
    // screen after an event has been switched off.
    let mut event_names = HashMap::new();
    for row in &graph.events {
        event_names.insert(row.id, row.name.clone());
        if row.enabled == 0 {
            continue;
        }
        let event_id = id_map.event(row.id)?;

        let trigger_id = graph.event_trigger.get(&row.id).ok_or_else(|| {
            ApiError::unprocessable(format!("event '{}' has no trigger", row.name))
        })?;
        let trigger = build_trigger(graph, &id_map, *trigger_id, 0)?;

        let mut effects = Vec::new();
        for effect_id in graph.event_effects.get(&row.id).into_iter().flatten() {
            effects.push(build_effect(
                graph,
                &id_map,
                &metadata,
                &parameters,
                *effect_id,
                0,
            )?);
        }

        events.push(Event {
            event_id,
            trigger,
            effects,
            once: row.fires_once != 0,
        });
    }

    // ── Inflation and taxes ─────────────────────────────────────────────────
    let inflation_profile = match graph.inflation_distribution_id {
        Some(id) => build_inflation_profile(graph, id)?,
        None => InflationProfile::default(),
    };

    let tax_config = match &graph.tax_config {
        Some(cfg) => {
            let federal_brackets: Vec<TaxBracket> = graph
                .tax_brackets
                .iter()
                .map(|b| TaxBracket {
                    threshold: b.threshold,
                    rate: b.rate,
                })
                .collect();

            // The engine walks brackets assuming ascending thresholds starting
            // at zero; an empty or gapped set would silently under-tax.
            if federal_brackets.is_empty() {
                return Err(ApiError::unprocessable(format!(
                    "tax config '{}' has no federal brackets",
                    cfg.name
                )));
            }
            if federal_brackets[0].threshold != 0.0 {
                return Err(ApiError::unprocessable(format!(
                    "tax config '{}' must have a bracket starting at 0",
                    cfg.name
                )));
            }

            TaxConfig {
                federal_brackets,
                state_rate: cfg.state_rate,
                capital_gains_rate: cfg.capital_gains_rate,
                early_withdrawal_penalty_rate: cfg.early_withdrawal_penalty_rate,
            }
        }
        None => TaxConfig::default(),
    };

    // An Age trigger or an RMD effect without a birth date can never resolve, so
    // reject it here rather than letting the engine emit warnings for 30 years.
    if birth_date.is_none() {
        for event in &events {
            if trigger_needs_birth_date(&event.trigger) {
                return Err(ApiError::unprocessable(
                    "scenario uses an Age trigger but has no birth_date".to_string(),
                ));
            }
            if event
                .effects
                .iter()
                .any(|e| matches!(e, EventEffect::ApplyRmd { .. }))
            {
                return Err(ApiError::unprocessable(
                    "scenario applies RMDs but has no birth_date".to_string(),
                ));
            }
        }
    }

    let config = SimulationConfig {
        return_profiles,
        inflation_profile,
        asset_returns,
        asset_prices,
        asset_tracking_errors,
        parameters,
        tax_config,
        start_date: Some(start_date),
        birth_date,
        accounts,
        duration_years: graph.scenario.duration_years.max(1) as usize,
        events,
        collect_ledger: graph.scenario.collect_ledger != 0,
    };

    Ok(CompiledScenario {
        config,
        id_map,
        metadata,
        account_names,
        event_names,
    })
}

fn trigger_needs_birth_date(trigger: &EventTrigger) -> bool {
    match trigger {
        EventTrigger::Age { .. } | EventTrigger::AgeParameter(_) => true,
        EventTrigger::And(children) | EventTrigger::Or(children) => {
            children.iter().any(trigger_needs_birth_date)
        }
        EventTrigger::Repeating {
            start_condition,
            end_condition,
            ..
        } => {
            start_condition
                .as_deref()
                .is_some_and(trigger_needs_birth_date)
                || end_condition
                    .as_deref()
                    .is_some_and(trigger_needs_birth_date)
        }
        _ => false,
    }
}

// ── distributions ───────────────────────────────────────────────────────────

fn distribution(graph: &ScenarioGraph, id: i64) -> ApiResult<&DistributionRow> {
    graph
        .distributions
        .get(&id)
        .ok_or_else(|| ApiError::unprocessable(format!("distribution {id} does not exist")))
}

/// A required numeric parameter. The schema's CHECK constraints already enforce
/// presence, so a miss means the row was written outside the API.
fn require(value: Option<f64>, kind: &str, field: &str) -> ApiResult<f64> {
    value
        .ok_or_else(|| ApiError::unprocessable(format!("{kind} distribution is missing '{field}'")))
}

fn build_return_profile(
    graph: &ScenarioGraph,
    distribution_id: i64,
    depth: usize,
) -> ApiResult<ReturnProfile> {
    depth_check(depth, "return profile")?;
    let row = distribution(graph, distribution_id)?;

    Ok(match row.kind.as_str() {
        "None" => ReturnProfile::None,
        "Fixed" => ReturnProfile::Fixed(require(row.rate, "Fixed", "rate")?),
        "Normal" => ReturnProfile::Normal {
            mean: require(row.mean, "Normal", "mean")?,
            std_dev: require(row.std_dev, "Normal", "std_dev")?,
        },
        "LogNormal" => ReturnProfile::LogNormal {
            mean: require(row.mean, "LogNormal", "mean")?,
            std_dev: require(row.std_dev, "LogNormal", "std_dev")?,
        },
        "StudentT" => ReturnProfile::StudentT {
            mean: require(row.mean, "StudentT", "mean")?,
            scale: require(row.scale, "StudentT", "scale")?,
            df: require(row.df, "StudentT", "df")?,
        },
        "RegimeSwitching" => {
            let bull_id = row.bull_id.ok_or_else(|| {
                ApiError::unprocessable("RegimeSwitching distribution is missing 'bull_id'")
            })?;
            let bear_id = row.bear_id.ok_or_else(|| {
                ApiError::unprocessable("RegimeSwitching distribution is missing 'bear_id'")
            })?;
            ReturnProfile::RegimeSwitching {
                bull: Box::new(build_return_profile(graph, bull_id, depth + 1)?),
                bear: Box::new(build_return_profile(graph, bear_id, depth + 1)?),
                bull_to_bear_prob: require(
                    row.bull_to_bear_prob,
                    "RegimeSwitching",
                    "bull_to_bear_prob",
                )?,
                bear_to_bull_prob: require(
                    row.bear_to_bull_prob,
                    "RegimeSwitching",
                    "bear_to_bull_prob",
                )?,
            }
        }
        "Bootstrap" => {
            let preset = row.history_preset.as_deref().unwrap_or_default();
            ReturnProfile::Bootstrap {
                history: historical_returns(preset)?,
                block_size: row.block_size.map(|b| b.max(1) as usize),
            }
        }
        other => {
            return Err(ApiError::unprocessable(format!(
                "unknown distribution kind '{other}'"
            )));
        }
    })
}

fn build_inflation_profile(
    graph: &ScenarioGraph,
    distribution_id: i64,
) -> ApiResult<InflationProfile> {
    let row = distribution(graph, distribution_id)?;

    Ok(match row.kind.as_str() {
        "None" => InflationProfile::None,
        "Fixed" => InflationProfile::Fixed(require(row.rate, "Fixed", "rate")?),
        "Normal" => InflationProfile::Normal {
            mean: require(row.mean, "Normal", "mean")?,
            std_dev: require(row.std_dev, "Normal", "std_dev")?,
        },
        "LogNormal" => InflationProfile::LogNormal {
            mean: require(row.mean, "LogNormal", "mean")?,
            std_dev: require(row.std_dev, "LogNormal", "std_dev")?,
        },
        "Bootstrap" => InflationProfile::Bootstrap {
            history: HistoricalInflation::us_cpi(),
            block_size: row.block_size.map(|b| b.max(1) as usize),
        },
        // `InflationProfile` has no StudentT or RegimeSwitching variant.
        other => {
            return Err(ApiError::unprocessable(format!(
                "'{other}' is not a valid inflation distribution; \
                 use None, Fixed, Normal, LogNormal or Bootstrap"
            )));
        }
    })
}

/// The bootstrap history presets the engine ships with.
pub const HISTORY_PRESETS: &[&str] = &[
    "sp500",
    "us_small_cap",
    "us_tbills",
    "us_long_bonds",
    "intl_developed",
    "emerging_markets",
    "reits",
    "gold",
    "us_agg_bonds",
    "us_corporate_bonds",
    "tips",
];

pub(crate) fn historical_returns(preset: &str) -> ApiResult<HistoricalReturns> {
    Ok(match preset {
        "sp500" => HistoricalReturns::sp500(),
        "us_small_cap" => HistoricalReturns::us_small_cap(),
        "us_tbills" => HistoricalReturns::us_tbills(),
        "us_long_bonds" => HistoricalReturns::us_long_bonds(),
        "intl_developed" => HistoricalReturns::intl_developed(),
        "emerging_markets" => HistoricalReturns::emerging_markets(),
        "reits" => HistoricalReturns::reits(),
        "gold" => HistoricalReturns::gold(),
        "us_agg_bonds" => HistoricalReturns::us_agg_bonds(),
        "us_corporate_bonds" => HistoricalReturns::us_corporate_bonds(),
        "tips" => HistoricalReturns::tips(),
        other => {
            return Err(ApiError::unprocessable(format!(
                "unknown history preset '{other}'; expected one of {}",
                HISTORY_PRESETS.join(", ")
            )));
        }
    })
}

// ── triggers ────────────────────────────────────────────────────────────────

fn build_trigger(
    graph: &ScenarioGraph,
    ids: &IdMap,
    trigger_id: i64,
    depth: usize,
) -> ApiResult<EventTrigger> {
    depth_check(depth, "trigger")?;

    let row = graph
        .triggers
        .get(&trigger_id)
        .ok_or_else(|| ApiError::unprocessable(format!("trigger {trigger_id} does not exist")))?;

    let threshold = |cmp: &Option<String>, value: Option<f64>| -> ApiResult<BalanceThreshold> {
        let value = value.ok_or_else(|| {
            ApiError::unprocessable("balance trigger is missing its threshold value")
        })?;
        match cmp.as_deref() {
            Some("GreaterThanOrEqual") => Ok(BalanceThreshold::GreaterThanOrEqual(value)),
            Some("LessThanOrEqual") => Ok(BalanceThreshold::LessThanOrEqual(value)),
            other => Err(ApiError::unprocessable(format!(
                "unknown comparison '{}'",
                other.unwrap_or("<null>")
            ))),
        }
    };

    Ok(match row.kind.as_str() {
        "Date" if row.parameter_id.is_some() => {
            let id = row.parameter_id.unwrap();
            if !matches!(
                graph
                    .parameters
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.kind.as_str()),
                Some("Date")
            ) {
                return Err(ApiError::unprocessable(
                    "Date trigger requires a Date parameter",
                ));
            }
            EventTrigger::DateParameter(ids.parameter(id)?)
        }
        "Age" if row.parameter_id.is_some() => {
            let id = row.parameter_id.unwrap();
            if !matches!(
                graph
                    .parameters
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.kind.as_str()),
                Some("Age")
            ) {
                return Err(ApiError::unprocessable(
                    "Age trigger requires an Age parameter",
                ));
            }
            EventTrigger::AgeParameter(ids.parameter(id)?)
        }
        "Date" => {
            let text = row
                .on_date
                .as_deref()
                .ok_or_else(|| ApiError::unprocessable("Date trigger is missing 'on_date'"))?;
            EventTrigger::Date(parse_date(text, "on_date")?)
        }
        "Age" => EventTrigger::Age {
            years: row
                .age_years
                .ok_or_else(|| ApiError::unprocessable("Age trigger is missing 'age_years'"))?
                as u8,
            months: row.age_months.map(|m| m as u8),
        },
        "RelativeToEvent" => {
            let ref_event = row.ref_event_id.ok_or_else(|| {
                ApiError::unprocessable("RelativeToEvent trigger is missing 'ref_event_id'")
            })?;
            let value = row.offset_value.ok_or_else(|| {
                ApiError::unprocessable("RelativeToEvent trigger is missing 'offset_value'")
            })? as i32;
            let offset = match row.offset_unit.as_deref() {
                Some("Days") => TriggerOffset::Days(value),
                Some("Months") => TriggerOffset::Months(value),
                Some("Years") => TriggerOffset::Years(value),
                other => {
                    return Err(ApiError::unprocessable(format!(
                        "unknown offset unit '{}'",
                        other.unwrap_or("<null>")
                    )));
                }
            };
            EventTrigger::RelativeToEvent {
                event_id: ids.event(ref_event)?,
                offset,
            }
        }
        "AccountBalance" => EventTrigger::AccountBalance {
            account_id: ids.account(row.account_id.ok_or_else(|| {
                ApiError::unprocessable("AccountBalance trigger is missing 'account_id'")
            })?)?,
            threshold: threshold(&row.comparison, row.threshold)?,
        },
        "AssetBalance" => EventTrigger::AssetBalance {
            asset_coord: AssetCoord {
                account_id: ids.account(row.account_id.ok_or_else(|| {
                    ApiError::unprocessable("AssetBalance trigger is missing 'account_id'")
                })?)?,
                asset_id: ids.asset(row.asset_id.ok_or_else(|| {
                    ApiError::unprocessable("AssetBalance trigger is missing 'asset_id'")
                })?)?,
            },
            threshold: threshold(&row.comparison, row.threshold)?,
        },
        "NetWorth" => EventTrigger::NetWorth {
            threshold: threshold(&row.comparison, row.threshold)?,
        },
        "And" | "Or" => {
            let child_ids = graph.trigger_children.get(&trigger_id);
            let mut children = Vec::new();
            for child in child_ids.into_iter().flatten() {
                children.push(build_trigger(graph, ids, *child, depth + 1)?);
            }
            if children.is_empty() {
                return Err(ApiError::unprocessable(format!(
                    "{} trigger has no child conditions",
                    row.kind
                )));
            }
            if row.kind == "And" {
                EventTrigger::And(children)
            } else {
                EventTrigger::Or(children)
            }
        }
        "Repeating" => {
            let interval = match row.interval.as_deref() {
                Some("Never") => RepeatInterval::Never,
                Some("Weekly") => RepeatInterval::Weekly,
                Some("BiWeekly") => RepeatInterval::BiWeekly,
                Some("Monthly") => RepeatInterval::Monthly,
                Some("Quarterly") => RepeatInterval::Quarterly,
                Some("Yearly") => RepeatInterval::Yearly,
                other => {
                    return Err(ApiError::unprocessable(format!(
                        "unknown repeat interval '{}'",
                        other.unwrap_or("<null>")
                    )));
                }
            };
            let start_condition = row
                .start_trigger_id
                .map(|id| build_trigger(graph, ids, id, depth + 1).map(Box::new))
                .transpose()?;
            let end_condition = row
                .end_trigger_id
                .map(|id| build_trigger(graph, ids, id, depth + 1).map(Box::new))
                .transpose()?;

            EventTrigger::Repeating {
                interval,
                start_condition,
                end_condition,
                max_occurrences: row.max_occurrences.map(|m| m as u32),
            }
        }
        "Manual" => EventTrigger::Manual,
        other => {
            return Err(ApiError::unprocessable(format!(
                "unknown trigger kind '{other}'"
            )));
        }
    })
}

// ── transfer amounts ────────────────────────────────────────────────────────

fn build_amount(
    graph: &ScenarioGraph,
    ids: &IdMap,
    amount_id: i64,
    depth: usize,
) -> ApiResult<TransferAmount> {
    depth_check(depth, "transfer amount")?;

    let row = graph.amounts.get(&amount_id).ok_or_else(|| {
        ApiError::unprocessable(format!("transfer amount {amount_id} does not exist"))
    })?;

    let value = || -> ApiResult<f64> {
        row.value.ok_or_else(|| {
            ApiError::unprocessable(format!("{} amount is missing 'value'", row.kind))
        })
    };
    let left = |depth: usize| -> ApiResult<TransferAmount> {
        let id = row.left_id.ok_or_else(|| {
            ApiError::unprocessable(format!("{} amount is missing its operand", row.kind))
        })?;
        if graph
            .amounts
            .get(&id)
            .is_some_and(|r| r.expression_source.is_some())
        {
            return Err(ApiError::unprocessable(
                "an Expression cannot be nested inside a legacy amount",
            ));
        }
        build_amount(graph, ids, id, depth + 1)
    };
    let right = |depth: usize| -> ApiResult<TransferAmount> {
        let id = row.right_id.ok_or_else(|| {
            ApiError::unprocessable(format!("{} amount is missing its right operand", row.kind))
        })?;
        if graph
            .amounts
            .get(&id)
            .is_some_and(|r| r.expression_source.is_some())
        {
            return Err(ApiError::unprocessable(
                "an Expression cannot be nested inside a legacy amount",
            ));
        }
        build_amount(graph, ids, id, depth + 1)
    };

    Ok(match row.kind.as_str() {
        "Fixed" => TransferAmount::fixed(value()?),
        "InflationAdjusted" => left(depth)?.inflated(),
        "SourceBalance" => TransferAmount::source_balance(),
        "ZeroTargetBalance" => TransferAmount::payoff(),
        "TargetToBalance" => TransferAmount::top_up(value()?),
        "AssetBalance" => TransferAmount::holding_balance(AssetCoord {
            account_id: ids.account(row.account_id.ok_or_else(|| {
                ApiError::unprocessable("AssetBalance amount is missing 'account_id'")
            })?)?,
            asset_id: ids.asset(row.asset_id.ok_or_else(|| {
                ApiError::unprocessable("AssetBalance amount is missing 'asset_id'")
            })?)?,
        }),
        "AccountTotalBalance" => {
            TransferAmount::account_balance(ids.account(row.account_id.ok_or_else(|| {
                ApiError::unprocessable("AccountTotalBalance amount is missing 'account_id'")
            })?)?)
        }
        "AccountCashBalance" => {
            TransferAmount::cash_balance(ids.account(row.account_id.ok_or_else(|| {
                ApiError::unprocessable("AccountCashBalance amount is missing 'account_id'")
            })?)?)
        }
        "Min" => left(depth)?.min(right(depth)?),
        "Max" => left(depth)?.max(right(depth)?),
        "Sub" => left(depth)?.minus(right(depth)?),
        "Add" => left(depth)?.plus(right(depth)?),
        "Mul" => {
            let fixed = |id: Option<i64>| {
                id.and_then(|id| graph.amounts.get(&id))
                    .filter(|r| r.kind == "Fixed" && r.expression_source.is_none())
                    .and_then(|r| r.value)
            };
            if let Some(factor) = fixed(row.left_id) {
                TransferAmount::scaled(factor, right(depth)?)
            } else if let Some(factor) = fixed(row.right_id) {
                TransferAmount::scaled(factor, left(depth)?)
            } else {
                return Err(ApiError::unprocessable(
                    "legacy Mul needs a fixed scalar operand",
                ));
            }
        }
        "Scale" => TransferAmount::scaled(value()?, left(depth)?),
        other => {
            return Err(ApiError::unprocessable(format!(
                "unknown transfer amount kind '{other}'"
            )));
        }
    })
}

// ── effects ─────────────────────────────────────────────────────────────────

fn build_effect(
    graph: &ScenarioGraph,
    ids: &IdMap,
    metadata: &SimulationMetadata,
    parameters: &HashMap<finplan_core::model::ParameterId, ParameterValue>,
    effect_id: i64,
    depth: usize,
) -> ApiResult<EventEffect> {
    depth_check(depth, "effect")?;

    let row = graph
        .effects
        .get(&effect_id)
        .ok_or_else(|| ApiError::unprocessable(format!("effect {effect_id} does not exist")))?;

    let amount = |depth: usize| -> ApiResult<TransferAmount> {
        let id = row.amount_id.ok_or_else(|| {
            ApiError::unprocessable(format!("{} effect is missing an amount", row.kind))
        })?;
        build_amount(graph, ids, id, depth)
    };
    let from = || -> ApiResult<i64> {
        row.from_account_id.ok_or_else(|| {
            ApiError::unprocessable(format!("{} effect is missing 'from_account_id'", row.kind))
        })
    };
    let to = || -> ApiResult<i64> {
        row.to_account_id.ok_or_else(|| {
            ApiError::unprocessable(format!("{} effect is missing 'to_account_id'", row.kind))
        })
    };
    let target = || -> ApiResult<i64> {
        row.target_event_id.ok_or_else(|| {
            ApiError::unprocessable(format!("{} effect is missing 'target_event_id'", row.kind))
        })
    };

    let amount_mode = match row.amount_mode.as_deref() {
        Some("Gross") => AmountMode::Gross,
        Some("Net") | None => AmountMode::Net,
        Some(other) => {
            return Err(ApiError::unprocessable(format!(
                "unknown amount mode '{other}'"
            )));
        }
    };

    let income_type = || -> ApiResult<IncomeType> {
        match row.income_type.as_deref() {
            Some("Taxable") => Ok(IncomeType::Taxable),
            Some("TaxFree") => Ok(IncomeType::TaxFree),
            other => Err(ApiError::unprocessable(format!(
                "unknown income type '{}'",
                other.unwrap_or("<null>")
            ))),
        }
    };

    let lot_method = match row.lot_method.as_deref() {
        Some("Fifo") | None => LotMethod::Fifo,
        Some("Lifo") => LotMethod::Lifo,
        Some("HighestCost") => LotMethod::HighestCost,
        Some("LowestCost") => LotMethod::LowestCost,
        Some("AverageCost") => LotMethod::AverageCost,
        Some(other) => {
            return Err(ApiError::unprocessable(format!(
                "unknown lot method '{other}'"
            )));
        }
    };

    let mut effect = match row.kind.as_str() {
        "Income" => EventEffect::Income {
            to: ids.account(to()?)?,
            amount: amount(depth)?,
            amount_mode,
            income_type: income_type()?,
        },
        "Expense" => EventEffect::Expense {
            from: ids.account(from()?)?,
            amount: amount(depth)?,
        },
        "AssetPurchase" => EventEffect::AssetPurchase {
            from: ids.account(from()?)?,
            to: AssetCoord {
                account_id: ids.account(to()?)?,
                asset_id: ids.asset(row.asset_id.ok_or_else(|| {
                    ApiError::unprocessable("AssetPurchase effect is missing 'asset_id'")
                })?)?,
            },
            amount: amount(depth)?,
        },
        "AssetSale" => EventEffect::AssetSale {
            from: ids.account(from()?)?,
            asset_id: row.asset_id.map(|id| ids.asset(id)).transpose()?,
            amount: amount(depth)?,
            amount_mode,
            lot_method,
        },
        "Sweep" => EventEffect::Sweep {
            sources: build_withdrawal_sources(graph, ids, effect_id)?,
            to: ids.account(to()?)?,
            amount: amount(depth)?,
            amount_mode,
            lot_method,
            income_type: income_type()?,
        },
        "AdjustBalance" => EventEffect::AdjustBalance {
            account: ids.account(to()?)?,
            amount: amount(depth)?,
        },
        "CashTransfer" => EventEffect::CashTransfer {
            from: ids.account(from()?)?,
            to: ids.account(to()?)?,
            amount: amount(depth)?,
        },
        "TriggerEvent" => EventEffect::TriggerEvent(ids.event(target()?)?),
        "PauseEvent" => EventEffect::PauseEvent(ids.event(target()?)?),
        "ResumeEvent" => EventEffect::ResumeEvent(ids.event(target()?)?),
        "TerminateEvent" => EventEffect::TerminateEvent(ids.event(target()?)?),
        "DeleteAccount" => EventEffect::DeleteAccount(ids.account(to()?)?),
        "ApplyRmd" => EventEffect::ApplyRmd {
            destination: ids.account(to()?)?,
            lot_method,
        },
        "RsuVesting" => EventEffect::RsuVesting {
            to: ids.account(to()?)?,
            asset: AssetCoord {
                account_id: ids.account(to()?)?,
                asset_id: ids.asset(row.asset_id.ok_or_else(|| {
                    ApiError::unprocessable("RsuVesting effect is missing 'asset_id'")
                })?)?,
            },
            units: row
                .units
                .ok_or_else(|| ApiError::unprocessable("RsuVesting effect is missing 'units'"))?,
            sell_to_cover: row.sell_to_cover.unwrap_or(0) != 0,
            lot_method,
        },
        "Random" => {
            let probability = row
                .probability
                .ok_or_else(|| ApiError::unprocessable("Random effect is missing 'probability'"))?;
            let on_true_id = graph
                .effect_children
                .get(&(effect_id, "on_true".to_string()))
                .ok_or_else(|| ApiError::unprocessable("Random effect has no 'on_true' branch"))?;
            let on_false = graph
                .effect_children
                .get(&(effect_id, "on_false".to_string()))
                .map(|id| {
                    build_effect(graph, ids, metadata, parameters, *id, depth + 1).map(Box::new)
                })
                .transpose()?;

            EventEffect::Random {
                probability,
                on_true: Box::new(build_effect(
                    graph,
                    ids,
                    metadata,
                    parameters,
                    *on_true_id,
                    depth + 1,
                )?),
                on_false,
            }
        }
        other => {
            return Err(ApiError::unprocessable(format!(
                "unknown effect kind '{other}'"
            )));
        }
    };
    if let Some(source) = row
        .amount_id
        .and_then(|id| graph.amounts.get(&id))
        .and_then(|r| r.expression_source.as_deref())
    {
        compile_amount(source, metadata, parameters)
            .map_err(|e| ApiError::unprocessable(format!("effect {effect_id}: {e}")))?
            .apply_to(&mut effect)
            .map_err(|e| ApiError::unprocessable(format!("effect {effect_id}: {e}")))?;
    }
    Ok(effect)
}

fn build_withdrawal_sources(
    graph: &ScenarioGraph,
    ids: &IdMap,
    effect_id: i64,
) -> ApiResult<WithdrawalSources> {
    // A Sweep with no explicit source configuration falls back to the engine's
    // default tax-efficient strategy.
    let Some(row) = graph.withdrawal_sources.get(&effect_id) else {
        return Ok(WithdrawalSources::default());
    };

    let items = graph.withdrawal_items.get(&effect_id);

    Ok(match row.mode.as_str() {
        "SingleAsset" => WithdrawalSources::SingleAsset(AssetCoord {
            account_id: ids.account(row.account_id.ok_or_else(|| {
                ApiError::unprocessable("SingleAsset withdrawal source is missing 'account_id'")
            })?)?,
            asset_id: ids.asset(row.asset_id.ok_or_else(|| {
                ApiError::unprocessable("SingleAsset withdrawal source is missing 'asset_id'")
            })?)?,
        }),
        "SingleAccount" => {
            WithdrawalSources::SingleAccount(ids.account(row.account_id.ok_or_else(|| {
                ApiError::unprocessable("SingleAccount withdrawal source is missing 'account_id'")
            })?)?)
        }
        "Strategy" => {
            let order = match row.strategy.as_deref() {
                Some("TaxEfficientEarly") => WithdrawalOrder::TaxEfficientEarly,
                Some("TaxDeferredFirst") => WithdrawalOrder::TaxDeferredFirst,
                Some("TaxFreeFirst") => WithdrawalOrder::TaxFreeFirst,
                Some("ProRata") => WithdrawalOrder::ProRata,
                Some("PenaltyAware") => WithdrawalOrder::PenaltyAware,
                other => {
                    return Err(ApiError::unprocessable(format!(
                        "unknown withdrawal strategy '{}'",
                        other.unwrap_or("<null>")
                    )));
                }
            };
            let mut exclude_accounts = Vec::new();
            for item in items.into_iter().flatten() {
                if item.role == "exclude" {
                    exclude_accounts.push(ids.account(item.account_id)?);
                }
            }
            WithdrawalSources::Strategy {
                order,
                exclude_accounts,
            }
        }
        "Custom" => {
            let mut coords = Vec::new();
            for item in items.into_iter().flatten() {
                if item.role != "custom" {
                    continue;
                }
                let asset_id = item.asset_id.ok_or_else(|| {
                    ApiError::unprocessable("custom withdrawal source is missing 'asset_id'")
                })?;
                coords.push(AssetCoord {
                    account_id: ids.account(item.account_id)?,
                    asset_id: ids.asset(asset_id)?,
                });
            }
            if coords.is_empty() {
                return Err(ApiError::unprocessable(
                    "custom withdrawal source has no entries",
                ));
            }
            WithdrawalSources::Custom(coords)
        }
        other => {
            return Err(ApiError::unprocessable(format!(
                "unknown withdrawal source mode '{other}'"
            )));
        }
    })
}
