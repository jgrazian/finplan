// Config actions - tax and inflation configuration

use crate::data::parameters_data::{DistributionType, FederalBracketsPreset, InflationData};
use crate::modals::parse_percentage;
use crate::state::context::{
    ConfigContext, InflationConfigContext, ModalContext, TaxConfigContext,
};
use crate::state::{AppState, FormField, FormModal, ModalAction, ModalState, PickerModal};

use super::{ActionContext, ActionResult};

/// Handle federal tax brackets preset selection
pub fn handle_federal_brackets_pick(state: &mut AppState, value: &str) -> ActionResult {
    let preset = match value {
        "2024 Single" => FederalBracketsPreset::Single2024,
        "2024 Married Joint" => FederalBracketsPreset::MarriedJoint2024,
        _ => FederalBracketsPreset::Single2024,
    };

    state.data_mut().parameters.tax_config.federal_brackets = preset;
    ActionResult::modified()
}

/// Handle tax config editing (state rate, cap gains rate)
pub fn handle_edit_tax_config(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let parts = ctx.value_parts();

    // Get typed config context
    let config_ctx = ctx
        .typed_context()
        .and_then(|c| c.as_config())
        .and_then(|c| {
            if let ConfigContext::Tax(tax) = c {
                Some(tax)
            } else {
                None
            }
        });

    match config_ctx {
        Some(TaxConfigContext::StateRate) => {
            if let Some(rate) = parts.first().and_then(|s| parse_percentage(s).ok()) {
                state.data_mut().parameters.tax_config.state_rate = rate;
                return ActionResult::modified();
            }
        }
        Some(TaxConfigContext::CapGainsRate) => {
            if let Some(rate) = parts.first().and_then(|s| parse_percentage(s).ok()) {
                state.data_mut().parameters.tax_config.capital_gains_rate = rate;
                return ActionResult::modified();
            }
        }
        Some(TaxConfigContext::FederalBrackets) | None => {}
    }

    ActionResult::close()
}

/// Handle inflation type selection
pub fn handle_inflation_type_pick(state: &mut AppState, value: &str) -> ActionResult {
    match value {
        "None" => {
            state.data_mut().parameters.inflation = InflationData::None;
            ActionResult::modified()
        }
        "Fixed" => {
            // Show form for fixed rate
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "Fixed Inflation",
                    vec![FormField::percentage("Rate", 0.03)],
                    ModalAction::EDIT_INFLATION,
                )
                .with_typed_context(ModalContext::Config(
                    ConfigContext::Inflation(InflationConfigContext::Fixed),
                )),
            ))
        }
        "Normal" => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Normal Inflation",
                vec![
                    FormField::percentage("Mean", 0.03),
                    FormField::percentage("Std Dev", 0.02),
                ],
                ModalAction::EDIT_INFLATION,
            )
            .with_typed_context(ModalContext::Config(ConfigContext::Inflation(
                InflationConfigContext::Normal,
            ))),
        )),
        "Log-Normal" => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Log-Normal Inflation",
                vec![
                    FormField::percentage("Mean", 0.03),
                    FormField::percentage("Std Dev", 0.02),
                ],
                ModalAction::EDIT_INFLATION,
            )
            .with_typed_context(ModalContext::Config(ConfigContext::Inflation(
                InflationConfigContext::LogNormal,
            ))),
        )),
        "US Historical" => {
            // Show picker for distribution type
            let options = vec![
                "Fixed (Mean)".to_string(),
                "Normal".to_string(),
                "Log-Normal".to_string(),
            ];
            ActionResult::modal(ModalState::Picker(PickerModal::new(
                "Historical Distribution",
                options,
                ModalAction::EDIT_INFLATION,
            )))
        }
        // Handle US Historical distribution sub-selection
        "Fixed (Mean)" => {
            state.data_mut().parameters.inflation = InflationData::USHistorical {
                distribution: DistributionType::Fixed,
            };
            ActionResult::modified()
        }
        _ => ActionResult::close(),
    }
}

/// Handle inflation editing
pub fn handle_edit_inflation(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let parts = ctx.value_parts();
    let value = ctx.value();

    // Get typed config context
    let inflation_ctx = ctx
        .typed_context()
        .and_then(|c| c.as_config())
        .and_then(|c| {
            if let ConfigContext::Inflation(inf) = c {
                Some(inf)
            } else {
                None
            }
        });

    match inflation_ctx {
        Some(InflationConfigContext::Fixed) => {
            let rate = parts
                .first()
                .and_then(|s| parse_percentage(s).ok())
                .unwrap_or(0.03);
            state.data_mut().parameters.inflation = InflationData::Fixed { rate };
            ActionResult::modified()
        }
        Some(InflationConfigContext::Normal) => {
            let mean = parts
                .first()
                .and_then(|s| parse_percentage(s).ok())
                .unwrap_or(0.03);
            let std_dev = parts
                .get(1)
                .and_then(|s| parse_percentage(s).ok())
                .unwrap_or(0.02);
            state.data_mut().parameters.inflation = InflationData::Normal { mean, std_dev };
            ActionResult::modified()
        }
        Some(InflationConfigContext::LogNormal) => {
            let mean = parts
                .first()
                .and_then(|s| parse_percentage(s).ok())
                .unwrap_or(0.03);
            let std_dev = parts
                .get(1)
                .and_then(|s| parse_percentage(s).ok())
                .unwrap_or(0.02);
            state.data_mut().parameters.inflation = InflationData::LogNormal { mean, std_dev };
            ActionResult::modified()
        }
        // Handle US Historical sub-picker selection (no typed context)
        None if value == "Normal" => {
            state.data_mut().parameters.inflation = InflationData::USHistorical {
                distribution: DistributionType::Normal,
            };
            ActionResult::modified()
        }
        None if value == "Log-Normal" => {
            state.data_mut().parameters.inflation = InflationData::USHistorical {
                distribution: DistributionType::LogNormal,
            };
            ActionResult::modified()
        }
        None if value == "Fixed (Mean)" => {
            state.data_mut().parameters.inflation = InflationData::USHistorical {
                distribution: DistributionType::Fixed,
            };
            ActionResult::modified()
        }
        None => ActionResult::close(),
    }
}
