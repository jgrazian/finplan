//! Named inputs persisted with a TUI scenario. Names are resolved to core IDs
//! during conversion, just like account and event references.

use std::collections::HashSet;

use finplan_core::model::ParameterValue;
use serde::{Deserialize, Serialize};

use super::{
    app_data::SimulationData,
    events_data::{AmountData, EffectData, TriggerData},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedParameterData {
    pub name: String,
    pub value: ParameterValue,
}

impl SimulationData {
    pub fn validate_named_parameters(&self) -> Result<(), String> {
        if self.named_parameters.len() > usize::from(u16::MAX) + 1 {
            return Err("Too many parameters (maximum 65536)".into());
        }
        let mut names = HashSet::new();
        for parameter in &self.named_parameters {
            if parameter.name.trim().is_empty() || parameter.name != parameter.name.trim() {
                return Err(
                    "Parameter names must be nonempty with no surrounding whitespace".into(),
                );
            }
            if !names.insert(&parameter.name) {
                return Err(format!("Duplicate parameter name: {}", parameter.name));
            }
            if !parameter.value.is_valid() {
                return Err(format!("Invalid value for parameter '{}'", parameter.name));
            }
        }
        Ok(())
    }

    /// Update references only; the caller updates the registry entry itself.
    pub fn rename_parameter(&mut self, old_name: &str, new_name: &str) {
        if old_name == new_name {
            return;
        }
        for sweep in &mut self.analysis.sweep_parameters {
            if sweep.parameter_name == old_name {
                sweep.parameter_name = new_name.into();
            }
        }
        for event in &mut self.events {
            let mut rename = |name: &mut String| {
                if name == old_name {
                    *name = new_name.to_owned();
                }
            };
            visit_trigger_names(&mut event.trigger, &mut rename);
            for effect in &mut event.effects {
                visit_effect_names(effect, &mut rename);
            }
        }
    }

    /// Events using a parameter, including disabled events and nested references.
    pub fn parameter_references(&self, name: &str) -> Vec<String> {
        self.events
            .iter()
            .filter(|event| {
                // Reuse the same complete traversal as renaming. Only the event is
                // cloned, and reference checks run during editing, never simulation.
                let mut event = (*event).clone();
                let mut found = false;
                let mut check = |reference: &mut String| {
                    found |= reference == name;
                };
                visit_trigger_names(&mut event.trigger, &mut check);
                for effect in &mut event.effects {
                    visit_effect_names(effect, &mut check);
                }
                found
            })
            .map(|event| event.name.0.clone())
            .collect()
    }
}

fn visit_trigger_names(trigger: &mut TriggerData, visitor: &mut impl FnMut(&mut String)) {
    match trigger {
        TriggerData::DateParameter { name } | TriggerData::AgeParameter { name } => visitor(name),
        TriggerData::Repeating { start, end, .. } => {
            if let Some(start) = start {
                visit_trigger_names(start, visitor);
            }
            if let Some(end) = end {
                visit_trigger_names(end, visitor);
            }
        }
        TriggerData::And { conditions } | TriggerData::Or { conditions } => {
            for condition in conditions {
                visit_trigger_names(condition, visitor);
            }
        }
        _ => {}
    }
}

fn visit_effect_names(effect: &mut EffectData, visitor: &mut impl FnMut(&mut String)) {
    match effect {
        EffectData::Income { amount, .. }
        | EffectData::Expense { amount, .. }
        | EffectData::AssetPurchase { amount, .. }
        | EffectData::AssetSale { amount, .. }
        | EffectData::Sweep { amount, .. }
        | EffectData::AdjustBalance { amount, .. }
        | EffectData::CashTransfer { amount, .. } => visit_amount_names(amount, visitor),
        _ => {}
    }
}

fn visit_amount_names(amount: &mut AmountData, visitor: &mut impl FnMut(&mut String)) {
    match amount {
        AmountData::Expression { source } => {
            super::expressions::visit_parameter_names(source, visitor)
        }
        AmountData::Parameter { name } => visitor(name),
        AmountData::RateTimes { rate, inner } => {
            visitor(rate);
            visit_amount_names(inner, visitor);
        }
        AmountData::InflationAdjusted { inner } | AmountData::Scale { inner, .. } => {
            visit_amount_names(inner, visitor);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{
        convert::to_simulation_config,
        events_data::{AccountTag, EventData, EventTag, IntervalData},
        portfolio_data::{AccountData, AccountType, Property},
    };
    use finplan_core::model::{AccountId, CalendarAge, EventTrigger, ParameterId};
    use jiff::civil::date;

    fn parameterized_scenario() -> SimulationData {
        let mut data = SimulationData::default();
        data.parameters.birth_date = "1960-01-01".into();
        data.parameters.start_date = "2025-01-01".into();
        data.parameters.duration_years = 1;
        data.portfolios.accounts.push(AccountData {
            name: "Checking".into(),
            description: None,
            account_type: AccountType::Checking(Property {
                value: 0.0,
                return_profile: None,
            }),
        });
        data.named_parameters = vec![
            NamedParameterData {
                name: "Salary".into(),
                value: ParameterValue::Money(1000.0),
            },
            NamedParameterData {
                name: "Match".into(),
                value: ParameterValue::Rate(0.05),
            },
            NamedParameterData {
                name: "Start".into(),
                value: ParameterValue::Date(date(2025, 1, 1)),
            },
            NamedParameterData {
                name: "Retire".into(),
                value: ParameterValue::Age(CalendarAge::new(65, 2)),
            },
        ];
        data.events.push(EventData {
            name: EventTag("Monthly match".into()),
            description: None,
            once: false,
            enabled: true,
            trigger: TriggerData::Repeating {
                interval: IntervalData::Monthly,
                start: Some(Box::new(TriggerData::DateParameter {
                    name: "Start".into(),
                })),
                end: Some(Box::new(TriggerData::AgeParameter {
                    name: "Retire".into(),
                })),
                max_occurrences: None,
            },
            effects: vec![EffectData::Income {
                to: AccountTag("Checking".into()),
                gross: true,
                taxable: false,
                amount: AmountData::InflationAdjusted {
                    inner: Box::new(AmountData::RateTimes {
                        rate: "Match".into(),
                        inner: Box::new(AmountData::Parameter {
                            name: "Salary".into(),
                        }),
                    }),
                },
            }],
        });
        data
    }

    #[test]
    fn typed_yaml_round_trips_and_old_scenarios_remain_readable() {
        let data = parameterized_scenario();
        let yaml = data.to_yaml().unwrap();
        let decoded = SimulationData::from_yaml(&yaml).unwrap();
        assert_eq!(decoded.named_parameters, data.named_parameters);
        assert_eq!(decoded.to_yaml().unwrap(), yaml);
        let old_yaml = SimulationData::default().to_yaml().unwrap();
        assert!(!old_yaml.contains("named_parameters"));
        assert!(
            SimulationData::from_yaml(&old_yaml)
                .unwrap()
                .named_parameters
                .is_empty()
        );
        let amount: AmountData = serde_saphyr::from_str("1250.0").unwrap();
        assert_eq!(amount, AmountData::fixed(1250.0));
    }

    #[test]
    fn all_four_types_compile_and_drive_simulation_after_reload() {
        let data = parameterized_scenario();
        let decoded = SimulationData::from_yaml(&data.to_yaml().unwrap()).unwrap();
        let config = to_simulation_config(&decoded).unwrap();
        assert_eq!(config.parameters.len(), 4);
        assert_eq!(
            config.parameters[&ParameterId(1)],
            ParameterValue::Rate(0.05)
        );
        assert!(
            matches!(&config.events[0].trigger, EventTrigger::Repeating { start_condition: Some(start), end_condition: Some(end), .. }
            if matches!(**start, EventTrigger::DateParameter(ParameterId(2))) && matches!(**end, EventTrigger::AgeParameter(ParameterId(3))))
        );
        let result = finplan_core::simulation::simulate(&config, 42).unwrap();
        // January and February match payments; retirement in March is exclusive.
        assert_eq!(result.final_account_balance(AccountId(1)), Some(100.0));
        let mut modified = decoded;
        modified.named_parameters[0].value = ParameterValue::Money(2000.0);
        let updated = to_simulation_config(&modified).unwrap();
        let result = finplan_core::simulation::simulate(&updated, 42).unwrap();
        assert_eq!(result.final_account_balance(AccountId(1)), Some(200.0));
    }

    #[test]
    fn conversion_rejects_invalid_values_names_and_typed_references() {
        let baseline = parameterized_scenario();
        for value in [
            ParameterValue::Money(f64::NAN),
            ParameterValue::Rate(f64::INFINITY),
            ParameterValue::Age(CalendarAge::new(65, 12)),
        ] {
            let mut invalid = baseline.clone();
            invalid.named_parameters[0].value = value;
            assert!(to_simulation_config(&invalid).is_err());
        }
        for name in ["", " Salary ", "Match"] {
            let mut invalid = baseline.clone();
            invalid.named_parameters[0].name = name.into();
            assert!(to_simulation_config(&invalid).is_err());
        }
        for index in 0..4 {
            let mut invalid = baseline.clone();
            let missing_name = invalid.named_parameters.remove(index).name;
            assert!(
                to_simulation_config(&invalid)
                    .unwrap_err()
                    .to_string()
                    .contains(&missing_name)
            );
            let mut wrong_type = baseline.clone();
            wrong_type.named_parameters[index].value = match index {
                0 => ParameterValue::Rate(1.0),
                _ => ParameterValue::Money(1.0),
            };
            assert!(
                to_simulation_config(&wrong_type)
                    .unwrap_err()
                    .to_string()
                    .contains(&wrong_type.named_parameters[index].name)
            );
        }
    }

    #[test]
    fn renaming_updates_nested_references_including_disabled_events() {
        let mut data = parameterized_scenario();
        data.events[0].enabled = false;
        let mut nested = data.events[0].clone();
        nested.name = EventTag("Nested".into());
        nested.trigger = TriggerData::And {
            conditions: vec![TriggerData::Or {
                conditions: vec![nested.trigger],
            }],
        };
        data.events.push(nested);
        for index in 0..4 {
            let old = data.named_parameters[index].name.clone();
            assert_eq!(
                data.parameter_references(&old),
                vec!["Monthly match", "Nested"]
            );
            let new = format!("New {old}");
            data.rename_parameter(&old, &new);
            data.named_parameters[index].name = new.clone();
            assert!(data.parameter_references(&old).is_empty());
            assert_eq!(
                data.parameter_references(&new),
                vec!["Monthly match", "Nested"]
            );
        }
        for event in &mut data.events {
            event.enabled = true;
        }
        assert!(to_simulation_config(&data).is_ok());
        assert!(data.parameter_references("unused").is_empty());
    }

    #[test]
    fn account_rename_reaches_rate_parameter_wrappers() {
        let mut data = parameterized_scenario();
        let EffectData::Income { amount, .. } = &mut data.events[0].effects[0] else {
            unreachable!()
        };
        *amount = AmountData::RateTimes {
            rate: "Match".into(),
            inner: Box::new(AmountData::AccountBalance {
                account: AccountTag("Checking".into()),
            }),
        };
        data.rename_account("Checking", "Cash");
        data.portfolios.accounts[0].name = "Cash".into();
        assert!(to_simulation_config(&data).is_ok());
    }
    #[test]
    fn expression_round_trip_renames_references_and_runs_with_typed_parameters() {
        let mut data = parameterized_scenario();
        *data.events[0].effects[0].amount_mut().unwrap() = AmountData::Expression {
            source: "gross(if(age_years($Retire) >= 65 and days_until($Start) <= 0, inflation($Match * $Salary), 0))".into(),
        };
        let loaded = SimulationData::from_yaml(&data.to_yaml().unwrap()).unwrap();
        let config = to_simulation_config(&loaded).unwrap();
        assert_eq!(
            finplan_core::simulation::simulate(&config, 42)
                .unwrap()
                .final_account_balance(AccountId(1)),
            Some(100.0)
        );
        assert_eq!(data.parameter_references("Salary"), vec!["Monthly match"]);
        data.rename_parameter("Salary", "Pay \"monthly\" \\ net");
        data.named_parameters[0].name = "Pay \"monthly\" \\ net".into();
        assert!(data.parameter_references("Salary").is_empty());
        assert_eq!(
            data.parameter_references("Pay \"monthly\" \\ net"),
            vec!["Monthly match"]
        );
        data.named_parameters[0].value = ParameterValue::Money(2000.0);
        let config = to_simulation_config(&data).unwrap();
        assert_eq!(
            finplan_core::simulation::simulate(&config, 42)
                .unwrap()
                .final_account_balance(AccountId(1)),
            Some(200.0)
        );
    }

    #[test]
    fn expression_account_rename_preserves_mode_and_distinguishes_parameter_names() {
        for rename_first in [true, false] {
            let mut data = parameterized_scenario();
            *data.events[0].effects[0].amount_mut().unwrap() = AmountData::Expression {
                source: "net($Salary + balance(\"Checking\"))".into(),
            };
            if rename_first {
                data.portfolios.accounts[0].name = "Cash \"joint\"".into();
            }
            data.rename_account("Checking", "Cash \"joint\"");
            data.portfolios.accounts[0].name = "Cash \"joint\"".into();
            let source = data.events[0].effects[0].amount().unwrap().to_source();
            assert!(source.starts_with("net("));
            assert!(source.contains("$Salary"));
            assert!(source.contains("Cash \\\"joint\\\""));
            assert!(to_simulation_config(&data).is_ok());
        }
    }

    #[test]
    fn expression_reference_tracking_ignores_dollars_inside_account_names() {
        let mut data = parameterized_scenario();
        *data.events[0].effects[0].amount_mut().unwrap() = AmountData::Expression {
            source: "balance(\"$Salary\") + $\"Match\" * 100".into(),
        };
        assert!(data.parameter_references("Salary").is_empty());
        assert_eq!(data.parameter_references("Match"), vec!["Monthly match"]);
        data.rename_parameter("Match", "Match rate");
        assert_eq!(
            data.events[0].effects[0].amount().unwrap().to_source(),
            "balance(\"$Salary\") + $\"Match rate\" * 100"
        );
    }
}
