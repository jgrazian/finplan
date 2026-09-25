//! Validate editable amount source in the context of the selected effect.
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use finplan_core::{
    expression::{EvaluationContext, ExpressionError, compile_amount},
    model::{AssetCoord, TransferEndpoint},
    simulation_state::SimulationState,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::specs::{AmountSpec, EffectSpec, WithdrawalSourcesSpec};
use crate::{
    auth::session::CurrentUser,
    compile::{self, idmap::IdMap, rows::ScenarioGraph},
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/scenarios/{scenario_id}/expressions/validate",
        post(validate),
    )
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ExpressionValidationRequest {
    pub effect: EffectSpec,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ExpressionDiagnostic {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ExpressionValidation {
    pub valid: bool,
    pub source: String,
    pub diagnostics: Vec<ExpressionDiagnostic>,
    pub preview_value: Option<f64>,
    pub preview_label: Option<String>,
    pub parameter_ids: Vec<i64>,
}

async fn validate(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ExpressionValidationRequest>,
) -> ApiResult<Json<ExpressionValidation>> {
    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    Ok(Json(validate_effect(&graph, &body.effect)?))
}

fn diagnostic(error: ExpressionError) -> ExpressionDiagnostic {
    ExpressionDiagnostic {
        message: error.message,
        start: error.span.start,
        end: error.span.end,
    }
}

fn context_error(message: &str) -> bool {
    message.contains("no single account")
        || message.contains("no single endpoint")
        || message.contains("no cash")
        || message.contains("unavailable")
}

pub(crate) fn validate_effect(
    graph: &ScenarioGraph,
    effect: &EffectSpec,
) -> ApiResult<ExpressionValidation> {
    let Some(AmountSpec::Expression { source }) = effect_amount(effect) else {
        return Ok(ExpressionValidation {
            valid: true,
            source: String::new(),
            diagnostics: vec![],
            preview_value: None,
            preview_label: None,
            parameter_ids: vec![],
        });
    };
    // Compile names and typed values independently of saved events. An invalid
    // old event must not prevent editing the pending expression to repair it.
    let (ids, metadata, parameters) = compile::expression_context(graph)?;
    let mut report = ExpressionValidation {
        valid: false,
        source: source.clone(),
        diagnostics: vec![],
        preview_value: None,
        preview_label: None,
        parameter_ids: vec![],
    };
    let amount = match compile_amount(source, &metadata, &parameters) {
        Ok(amount) => amount,
        Err(error) => {
            report.diagnostics.push(diagnostic(error));
            return Ok(report);
        }
    };
    if amount.amount_mode.is_some()
        && !matches!(
            effect,
            EffectSpec::Income { .. } | EffectSpec::AssetSale { .. } | EffectSpec::Sweep { .. }
        )
    {
        report.diagnostics.push(ExpressionDiagnostic {
            message: "gross/net is unavailable for this effect".into(),
            start: 0,
            end: source.len(),
        });
        return Ok(report);
    }
    let references = amount.amount.expression().references();
    report.parameter_ids = references
        .parameters
        .into_iter()
        .filter_map(|id| ids.parameter_db_id(id))
        .collect();
    let (source_account, source_endpoint, target_account, target_endpoint) =
        effect_availability(effect);
    if let Err(error) = amount.amount.expression().validate_context(
        source_account,
        source_endpoint,
        target_account,
        target_endpoint,
    ) {
        report.diagnostics.push(diagnostic(error));
        return Ok(report);
    }
    let birth_date = graph
        .scenario
        .birth_date
        .as_deref()
        .map(str::parse::<jiff::civil::Date>)
        .transpose()
        .map_err(|error| ApiError::unprocessable(format!("invalid birth date: {error}")))?;
    let bound = match amount.amount.bind_parameters(&parameters, birth_date) {
        Ok(bound) => bound,
        Err(error) => {
            report.diagnostics.push(diagnostic(error));
            return Ok(report);
        }
    };
    // A saved event can compile but fail to bind at simulation start, for
    // example after the scenario's birth date is removed. Its effects do not
    // affect opening balances, so use an eventless preview while it is repaired.
    let preview = compile::compile(graph).ok().and_then(|compiled| {
        SimulationState::from_parameters(&compiled.config, 0)
            .ok()
            .map(|state| (compiled, state))
    });
    let preview = preview.or_else(|| {
        let mut opening = graph.clone();
        opening.events.clear();
        compile::compile(&opening).ok().and_then(|compiled| {
            SimulationState::from_parameters(&compiled.config, 0)
                .ok()
                .map(|state| (compiled, state))
        })
    });
    let Some((compiled, state)) = preview else {
        report.valid = true;
        report.preview_label =
            Some("Preview unavailable until the plan configuration is valid".into());
        return Ok(report);
    };
    let context = effect_context(effect, &compiled.id_map, &state)?;
    match bound.expression().evaluate(&context) {
        Ok(value) => {
            report.preview_value = Some(value);
            report.preview_label = Some("Value at plan start; future values may change".into());
        }
        Err(error)
            if context_error(&error.message)
                || !bound.expression().error_depends_on_state(&error, &context) =>
        {
            report.diagnostics.push(diagnostic(error));
        }
        Err(_) => {
            // A division or bound may become valid later in a run. The preview
            // cannot claim a value at plan start, but the source still compiles.
            report.preview_label = Some("Value depends on simulation state".into());
        }
    }
    report.valid = report.diagnostics.is_empty();
    Ok(report)
}

/// Account identity and a single transferable endpoint are different for sales
/// and sweeps. The order is source account, source endpoint, target account,
/// target endpoint.
fn effect_availability(effect: &EffectSpec) -> (bool, bool, bool, bool) {
    match effect {
        EffectSpec::Income { .. } => (false, false, true, true),
        EffectSpec::Expense { .. } => (true, true, false, false),
        EffectSpec::CashTransfer { .. } | EffectSpec::AssetPurchase { .. } => {
            (true, true, true, true)
        }
        EffectSpec::AssetSale { asset_id, .. } => (true, asset_id.is_some(), true, true),
        EffectSpec::Sweep { sources, .. } => match sources {
            Some(WithdrawalSourcesSpec::SingleAccount { .. }) => (true, false, true, true),
            Some(WithdrawalSourcesSpec::SingleAsset { .. }) => (true, true, true, true),
            _ => (false, false, true, true),
        },
        EffectSpec::AdjustBalance { .. } => (false, false, true, true),
        _ => (false, false, false, false),
    }
}

fn cash(id: finplan_core::model::AccountId) -> TransferEndpoint {
    TransferEndpoint::Cash { account_id: id }
}

fn effect_context<'a>(
    effect: &EffectSpec,
    ids: &IdMap,
    state: &'a SimulationState,
) -> ApiResult<EvaluationContext<'a>> {
    let context = EvaluationContext::new(state);
    let external = TransferEndpoint::External;
    Ok(match effect {
        EffectSpec::Income { to_account_id, .. } => {
            context.with_endpoints(external, cash(ids.account(*to_account_id)?))
        }
        EffectSpec::Expense {
            from_account_id, ..
        } => context.with_endpoints(cash(ids.account(*from_account_id)?), external),
        EffectSpec::CashTransfer {
            from_account_id,
            to_account_id,
            ..
        } => context.with_endpoints(
            cash(ids.account(*from_account_id)?),
            cash(ids.account(*to_account_id)?),
        ),
        EffectSpec::AssetPurchase {
            from_account_id,
            to_account_id,
            asset_id,
            ..
        } => context.with_endpoints(
            cash(ids.account(*from_account_id)?),
            TransferEndpoint::Asset {
                asset_coord: AssetCoord {
                    account_id: ids.account(*to_account_id)?,
                    asset_id: ids.asset(*asset_id)?,
                },
            },
        ),
        EffectSpec::AssetSale {
            from_account_id,
            asset_id,
            ..
        } => {
            let from = ids.account(*from_account_id)?;
            let target = cash(from);
            match asset_id {
                Some(asset) => context.with_endpoints(
                    TransferEndpoint::Asset {
                        asset_coord: AssetCoord {
                            account_id: from,
                            asset_id: ids.asset(*asset)?,
                        },
                    },
                    target,
                ),
                None => context
                    .with_endpoints(external, target)
                    .with_source_account(from),
            }
        }
        EffectSpec::Sweep {
            to_account_id,
            sources,
            ..
        } => {
            let target = cash(ids.account(*to_account_id)?);
            let context = context.with_endpoints(external, target);
            match sources {
                Some(WithdrawalSourcesSpec::SingleAccount { account_id }) => {
                    context.with_source_account(ids.account(*account_id)?)
                }
                Some(WithdrawalSourcesSpec::SingleAsset {
                    account_id,
                    asset_id,
                }) => context.with_endpoints(
                    TransferEndpoint::Asset {
                        asset_coord: AssetCoord {
                            account_id: ids.account(*account_id)?,
                            asset_id: ids.asset(*asset_id)?,
                        },
                    },
                    target,
                ),
                _ => context,
            }
        }
        EffectSpec::AdjustBalance { account_id, .. } => {
            context.with_endpoints(external, cash(ids.account(*account_id)?))
        }
        _ => context,
    })
}

fn effect_amount(effect: &EffectSpec) -> Option<&AmountSpec> {
    match effect {
        EffectSpec::Income { amount, .. }
        | EffectSpec::Expense { amount, .. }
        | EffectSpec::AssetPurchase { amount, .. }
        | EffectSpec::AssetSale { amount, .. }
        | EffectSpec::Sweep { amount, .. }
        | EffectSpec::AdjustBalance { amount, .. }
        | EffectSpec::CashTransfer { amount, .. }
        | EffectSpec::BuyProperty { price: amount, .. } => Some(amount),
        _ => None,
    }
}

pub(crate) fn validate_tree(graph: &ScenarioGraph, effects: &[EffectSpec]) -> ApiResult<()> {
    fn one(graph: &ScenarioGraph, effect: &EffectSpec) -> ApiResult<()> {
        if let EffectSpec::Random {
            on_true, on_false, ..
        } = effect
        {
            one(graph, on_true)?;
            if let Some(child) = on_false {
                one(graph, child)?;
            }
        }
        let report = validate_effect(graph, effect)?;
        if let Some(error) = report.diagnostics.first() {
            return Err(ApiError::bad_request(format!(
                "invalid amount expression: {} at bytes {}..{}",
                error.message, error.start, error.end,
            )));
        }
        // A purchase's second amount, which the preview endpoint never sees.
        if let EffectSpec::BuyProperty {
            financing: Some(financing),
            ..
        } = effect
            && let AmountSpec::Expression { source } = &financing.down_payment
        {
            let (_, metadata, parameters) = compile::expression_context(graph)?;
            let amount = compile_amount(source, &metadata, &parameters).map_err(|error| {
                ApiError::bad_request(format!("invalid down payment expression: {error}"))
            })?;
            if amount.amount_mode.is_some() {
                return Err(ApiError::bad_request(
                    "gross/net is unavailable for a down payment",
                ));
            }
        }
        Ok(())
    }
    for effect in effects {
        one(graph, effect)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use finplan_core::{config::SimulationBuilder, expression::Expression};

    #[test]
    fn preview_context_uses_each_effects_actual_accounts() {
        let (config, metadata) = SimulationBuilder::new()
            .start(2025, 1, 1)
            .years(1)
            .bank("Checking", 100.0)
            .bank("Savings", 200.0)
            .build();
        let state = SimulationState::from_parameters(&config, 0).unwrap();
        let mut ids = IdMap::new();
        assert_eq!(
            ids.intern_account(10).unwrap(),
            metadata.account_id("Checking").unwrap()
        );
        assert_eq!(
            ids.intern_account(20).unwrap(),
            metadata.account_id("Savings").unwrap()
        );
        let amount = AmountSpec::Expression {
            source: "balance(target)".into(),
        };
        let income = EffectSpec::Income {
            to_account_id: 20,
            amount: amount.clone(),
            amount_mode: Default::default(),
            income_type: super::super::specs::IncomeType::TaxFree,
        };
        let expression =
            Expression::compile("balance(target)", &metadata, &config.parameters).unwrap();
        assert_eq!(
            expression
                .evaluate(&effect_context(&income, &ids, &state).unwrap())
                .unwrap(),
            200.0
        );
        let expense = EffectSpec::Expense {
            from_account_id: 10,
            amount: amount.clone(),
        };
        let expression =
            Expression::compile("balance(source)", &metadata, &config.parameters).unwrap();
        assert_eq!(
            expression
                .evaluate(&effect_context(&expense, &ids, &state).unwrap())
                .unwrap(),
            100.0
        );
        assert!(
            expression
                .evaluate(&effect_context(&income, &ids, &state).unwrap())
                .is_err()
        );
        let sweep = EffectSpec::Sweep {
            to_account_id: 20,
            amount,
            sources: Some(WithdrawalSourcesSpec::SingleAccount { account_id: 10 }),
            amount_mode: Default::default(),
            lot_method: Default::default(),
            income_type: super::super::specs::IncomeType::TaxFree,
        };
        assert_eq!(
            expression
                .evaluate(&effect_context(&sweep, &ids, &state).unwrap())
                .unwrap(),
            100.0
        );
        let endpoint =
            Expression::compile("source_balance()", &metadata, &config.parameters).unwrap();
        assert!(
            endpoint
                .evaluate(&effect_context(&sweep, &ids, &state).unwrap())
                .is_err()
        );
        let lazy = Expression::compile(
            "if(false, balance(source), 100)",
            &metadata,
            &config.parameters,
        )
        .unwrap();
        assert!(lazy.references().source_account);
        assert!(lazy.validate_context(false, false, true, true).is_err());
        assert!(lazy.validate_context(true, false, true, true).is_ok());
    }
}
