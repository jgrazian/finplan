use std::collections::HashMap;

use crate::apply::process_events;
use crate::config::SimulationBuilder;
use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, CalendarAge, Cash, Event, EventEffect, EventId,
    EventTrigger, IncomeType, InflationProfile, ParameterId, ParameterValue, RepeatInterval,
    ReturnProfileId, StateEvent, TransferAmount,
};
use crate::optimization::{OptimizableParameter, apply_parameters};
use crate::simulation::{monte_carlo_simulate_with_config, simulate};
use crate::simulation_state::SimulationState;

fn config_with_amount(amount: TransferAmount) -> SimulationConfig {
    SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 1,
        inflation_profile: InflationProfile::None,
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Bank(Cash {
                value: 0.0,
                return_profile_id: ReturnProfileId(0),
            }),
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Date(jiff::civil::date(2025, 1, 1)),
            effects: vec![EventEffect::AdjustBalance {
                account: AccountId(1),
                amount,
            }],
            once: true,
        }],
        ..Default::default()
    }
}

#[test]
fn parameter_references_bind_in_nested_expressions_and_preserve_config() {
    let id = ParameterId(7);
    let mut config = config_with_amount(TransferAmount::InflationAdjusted(Box::new(
        TransferAmount::Mul(
            Box::new(TransferAmount::parameter(id)),
            Box::new(TransferAmount::Fixed(2.0)),
        ),
    )));
    config.parameters.insert(id, ParameterValue::Money(125.0));
    let original_config = serde_json::to_value(&config).unwrap();

    let state = SimulationState::from_parameters(&config, 5).unwrap();
    let bound = state.event_state.get_event(EventId(1)).unwrap();
    let EventEffect::AdjustBalance { amount, .. } = &bound.effects[0] else {
        panic!("expected balance adjustment")
    };
    assert!(matches!(
        amount,
        TransferAmount::InflationAdjusted(inner)
            if matches!(**inner, TransferAmount::Mul(ref left, ref right)
                if matches!(**left, TransferAmount::Fixed(125.0))
                    && matches!(**right, TransferAmount::Fixed(2.0)))
    ));
    assert_eq!(serde_json::to_value(&config).unwrap(), original_config);
    assert!(matches!(
        config.events[0].effects[0],
        EventEffect::AdjustBalance {
            amount: TransferAmount::InflationAdjusted(_),
            ..
        }
    ));

    let parameter_result = simulate(&config, 5).unwrap();
    let literal_result = simulate(
        &config_with_amount(TransferAmount::InflationAdjusted(Box::new(
            TransferAmount::Fixed(250.0),
        ))),
        5,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&parameter_result).unwrap(),
        serde_json::to_value(&literal_result).unwrap()
    );
    assert_eq!(
        parameter_result.final_account_balance(AccountId(1)),
        literal_result.final_account_balance(AccountId(1))
    );
}

#[test]
fn shared_references_in_both_random_branches_match_literal_monte_carlo() {
    let id = ParameterId(4);
    let mut parameterized = config_with_amount(TransferAmount::Fixed(0.0));
    parameterized
        .parameters
        .insert(id, ParameterValue::Money(80.0));
    parameterized.events[0].effects = vec![EventEffect::Random {
        probability: 0.5,
        on_true: Box::new(EventEffect::AdjustBalance {
            account: AccountId(1),
            amount: TransferAmount::InflationAdjusted(Box::new(TransferAmount::parameter(id))),
        }),
        on_false: Some(Box::new(EventEffect::AdjustBalance {
            account: AccountId(1),
            amount: TransferAmount::Scale(2.0, Box::new(TransferAmount::parameter(id))),
        })),
    }];
    parameterized.events.push(Event {
        event_id: EventId(2),
        trigger: EventTrigger::Date(jiff::civil::date(2025, 1, 1)),
        effects: vec![EventEffect::AdjustBalance {
            account: AccountId(1),
            amount: TransferAmount::Add(
                Box::new(TransferAmount::Fixed(0.0)),
                Box::new(TransferAmount::parameter(id)),
            ),
        }],
        once: true,
    });

    let mut literal = config_with_amount(TransferAmount::Fixed(0.0));
    literal.events[0].effects = vec![EventEffect::Random {
        probability: 0.5,
        on_true: Box::new(EventEffect::AdjustBalance {
            account: AccountId(1),
            amount: TransferAmount::InflationAdjusted(Box::new(TransferAmount::Fixed(80.0))),
        }),
        on_false: Some(Box::new(EventEffect::AdjustBalance {
            account: AccountId(1),
            amount: TransferAmount::Scale(2.0, Box::new(TransferAmount::Fixed(80.0))),
        })),
    }];
    literal.events.push(Event {
        event_id: EventId(2),
        trigger: EventTrigger::Date(jiff::civil::date(2025, 1, 1)),
        effects: vec![EventEffect::AdjustBalance {
            account: AccountId(1),
            amount: TransferAmount::Add(
                Box::new(TransferAmount::Fixed(0.0)),
                Box::new(TransferAmount::Fixed(80.0)),
            ),
        }],
        once: true,
    });

    let mc = crate::model::MonteCarloConfig {
        iterations: 12,
        seed: Some(42),
        compute_mean: true,
        ..Default::default()
    };
    let parameterized_summary = monte_carlo_simulate_with_config(&parameterized, &mc).unwrap();
    let literal_summary = monte_carlo_simulate_with_config(&literal, &mc).unwrap();
    assert_eq!(
        parameterized_summary.stats.mean_final_net_worth,
        literal_summary.stats.mean_final_net_worth
    );
    assert_eq!(
        parameterized_summary.stats.success_rate,
        literal_summary.stats.success_rate
    );
    for seed in [7, 42, 99] {
        let parameterized_result = simulate(&parameterized, seed).unwrap();
        let literal_result = simulate(&literal, seed).unwrap();
        assert_eq!(
            serde_json::to_value(parameterized_result).unwrap(),
            serde_json::to_value(literal_result).unwrap(),
            "parameter binding changed seeded run {seed}"
        );
    }
}

#[test]
fn parameterized_rate_uses_the_current_balance_and_inflation_at_event_time() {
    let amount_id = ParameterId(1);
    let rate_id = ParameterId(2);
    let mut config = SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 3,
        inflation_profile: InflationProfile::Fixed(0.10),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Bank(Cash {
                value: 100.0,
                return_profile_id: ReturnProfileId(0),
            }),
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Yearly,
                start_condition: None,
                end_condition: None,
                max_occurrences: Some(2),
            },
            effects: vec![EventEffect::Income {
                to: AccountId(1),
                amount: TransferAmount::Add(
                    Box::new(TransferAmount::InflationAdjusted(Box::new(
                        TransferAmount::parameter(amount_id),
                    ))),
                    Box::new(TransferAmount::Mul(
                        Box::new(TransferAmount::parameter(rate_id)),
                        Box::new(TransferAmount::AccountCashBalance {
                            account_id: AccountId(1),
                        }),
                    )),
                ),
                amount_mode: AmountMode::Gross,
                income_type: IncomeType::TaxFree,
            }],
            once: false,
        }],
        ..Default::default()
    };
    config.parameters = HashMap::from([
        (amount_id, ParameterValue::Money(100.0)),
        (rate_id, ParameterValue::Rate(0.10)),
    ]);

    let result = simulate(&config, 1).unwrap();
    let credits: Vec<(i16, f64)> = result
        .ledger
        .iter()
        .filter_map(|entry| match &entry.event {
            StateEvent::CashCredit { amount, kind, .. }
                if *kind == crate::model::CashFlowKind::Income =>
            {
                Some((entry.date.year(), *amount))
            }
            _ => None,
        })
        .collect();
    assert_eq!(credits.len(), 2);
    assert_eq!(credits[0], (2025, 110.0));
    assert_eq!(credits[1].0, 2026);
    assert!((credits[1].1 - 131.0).abs() < 1e-9, "credits: {credits:?}");
}

#[test]
fn missing_reference_in_unselected_random_branch_fails_during_initialization() {
    let missing = ParameterId(22);
    let mut config = config_with_amount(TransferAmount::Fixed(10.0));
    config.events[0].effects = vec![EventEffect::Random {
        probability: 1.0,
        on_true: Box::new(EventEffect::AdjustBalance {
            account: AccountId(1),
            amount: TransferAmount::Fixed(10.0),
        }),
        on_false: Some(Box::new(EventEffect::AdjustBalance {
            account: AccountId(1),
            amount: TransferAmount::Scale(0.5, Box::new(TransferAmount::parameter(missing))),
        })),
    }];

    let error = SimulationState::from_parameters(&config, 1).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing parameter ParameterId(22)")
    );
}

#[test]
fn non_finite_parameters_are_rejected_even_when_unused() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut config = SimulationConfig::default();
        config
            .parameters
            .insert(ParameterId(3), ParameterValue::Money(value));
        let error = SimulationState::from_parameters(&config, 1).unwrap_err();
        assert!(error.to_string().contains("invalid value"));
    }
}

#[test]
fn numeric_optimizer_changes_only_the_target_parameter() {
    let a = ParameterId(1);
    let b = ParameterId(2);
    let mut config = config_with_amount(TransferAmount::Fixed(77.0));
    config.parameters = HashMap::from([
        (a, ParameterValue::Money(10.0)),
        (b, ParameterValue::Rate(20.0)),
    ]);
    let updated = apply_parameters(
        &config,
        &[OptimizableParameter::NumericParameter {
            parameter_id: a,
            min_value: 0.0,
            max_value: 100.0,
        }],
        &[12.5],
    )
    .unwrap();

    assert_eq!(updated.parameters[&a], ParameterValue::Money(12.5));
    assert_eq!(updated.parameters[&b], ParameterValue::Rate(20.0));
    assert_eq!(config.parameters[&a], ParameterValue::Money(10.0));
    assert!(matches!(
        updated.events[0].effects[0],
        EventEffect::AdjustBalance {
            amount: TransferAmount::Fixed(77.0),
            ..
        }
    ));
    assert!(
        apply_parameters(
            &config,
            &[OptimizableParameter::NumericParameter {
                parameter_id: ParameterId(99),
                min_value: 0.0,
                max_value: 100.0,
            }],
            &[12.5],
        )
        .is_none()
    );
    assert!(
        apply_parameters(
            &config,
            &[OptimizableParameter::NumericParameter {
                parameter_id: a,
                min_value: 0.0,
                max_value: 100.0,
            }],
            &[f64::NAN],
        )
        .is_none()
    );
}

#[test]
fn parameter_config_serialization_is_backward_compatible_and_round_trips() {
    let id = ParameterId(9);
    let mut config = config_with_amount(TransferAmount::parameter(id));
    config.parameters.insert(id, ParameterValue::Money(4.25));

    let encoded = serde_json::to_value(&config).unwrap();
    let decoded: SimulationConfig = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded.parameters[&id], ParameterValue::Money(4.25));
    assert!(matches!(
        decoded.events[0].effects[0],
        EventEffect::AdjustBalance {
            amount: TransferAmount::Parameter(ParameterId(9)),
            ..
        }
    ));

    let mut legacy = encoded;
    legacy.as_object_mut().unwrap().remove("parameters");
    let legacy: SimulationConfig = serde_json::from_value(legacy).unwrap();
    assert!(legacy.parameters.is_empty());
}

#[test]
fn builder_registers_and_resolves_parameter_names() {
    let builder = SimulationBuilder::new()
        .parameter("withdrawal_rate", ParameterValue::Rate(0.04))
        .parameter("monthly_spending", 4_000.0);
    let rate_id = builder.parameter_id("withdrawal_rate").unwrap();
    let spending_id = builder.parameter_id("monthly_spending").unwrap();
    assert_ne!(rate_id, spending_id);

    let (config, metadata) = builder.build();
    assert_eq!(config.parameters[&rate_id], ParameterValue::Rate(0.04));
    assert_eq!(
        config.parameters[&spending_id],
        ParameterValue::Money(4_000.0)
    );
    assert_eq!(metadata.parameter_id("withdrawal_rate"), Some(rate_id));
    assert_eq!(
        metadata.parameter_name(spending_id),
        Some("monthly_spending")
    );
}

#[test]
fn typed_arithmetic_accepts_money_times_rate_and_rejects_mismatches() {
    let money = ParameterId(1);
    let rate = ParameterId(2);
    let mut config = config_with_amount(TransferAmount::Mul(
        Box::new(TransferAmount::parameter(money)),
        Box::new(TransferAmount::Mul(
            Box::new(TransferAmount::parameter(rate)),
            Box::new(TransferAmount::Fixed(2.0)),
        )),
    ));
    config
        .parameters
        .insert(money, ParameterValue::Money(100.0));
    config.parameters.insert(rate, ParameterValue::Rate(0.05));
    assert_eq!(
        simulate(&config, 3)
            .unwrap()
            .final_account_balance(AccountId(1)),
        Some(10.0)
    );

    for amount in [
        TransferAmount::Add(
            Box::new(TransferAmount::parameter(money)),
            Box::new(TransferAmount::parameter(rate)),
        ),
        TransferAmount::Mul(
            Box::new(TransferAmount::parameter(money)),
            Box::new(TransferAmount::parameter(money)),
        ),
        TransferAmount::parameter(rate),
    ] {
        config.events[0].effects = vec![EventEffect::AdjustBalance {
            account: AccountId(1),
            amount,
        }];
        assert!(SimulationState::from_parameters(&config, 3).is_err());
    }
    for value in [
        ParameterValue::Date(jiff::civil::date(2025, 2, 15)),
        ParameterValue::Age(CalendarAge::years(65)),
    ] {
        config.parameters.insert(ParameterId(3), value);
        config.events[0].effects = vec![EventEffect::AdjustBalance {
            account: AccountId(1),
            amount: TransferAmount::parameter(ParameterId(3)),
        }];
        assert!(SimulationState::from_parameters(&config, 3).is_err());
    }
    config.events[0].effects = vec![EventEffect::AdjustBalance {
        account: AccountId(1),
        amount: TransferAmount::Mul(
            Box::new(TransferAmount::parameter(money)),
            Box::new(TransferAmount::Scale(
                2.0,
                Box::new(TransferAmount::parameter(rate)),
            )),
        ),
    }];
    assert_eq!(
        simulate(&config, 3)
            .unwrap()
            .final_account_balance(AccountId(1)),
        Some(10.0)
    );
    config.events[0].effects = vec![EventEffect::AdjustBalance {
        account: AccountId(1),
        amount: TransferAmount::Mul(
            Box::new(TransferAmount::parameter(rate)),
            Box::new(TransferAmount::Fixed(100.0)),
        ),
    }];
    assert!(SimulationState::from_parameters(&config, 3).is_ok());
}

#[test]
fn typed_values_round_trip_and_legacy_numbers_become_money() {
    let mut config = config_with_amount(TransferAmount::Fixed(1.0));
    config.parameters = HashMap::from([
        (ParameterId(1), ParameterValue::Money(100.0)),
        (ParameterId(2), ParameterValue::Rate(-0.5)),
        (
            ParameterId(3),
            ParameterValue::Date(jiff::civil::date(2025, 2, 15)),
        ),
        (ParameterId(4), ParameterValue::Age(CalendarAge::new(65, 6))),
    ]);
    let encoded = serde_json::to_value(&config).unwrap();
    let decoded: SimulationConfig = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded.parameters, config.parameters);
    let mut legacy = encoded;
    legacy["parameters"]["1"] = serde_json::json!(42.0);
    let decoded: SimulationConfig = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        decoded.parameters[&ParameterId(1)],
        ParameterValue::Money(42.0)
    );
}

#[test]
fn overrides_and_optimizer_preserve_parameter_kinds() {
    let mut config = config_with_amount(TransferAmount::Fixed(1.0));
    let rate = ParameterId(1);
    let date = ParameterId(2);
    let age = ParameterId(3);
    config.parameters.insert(rate, ParameterValue::Rate(1.5));
    config
        .parameters
        .insert(date, ParameterValue::Date(jiff::civil::date(2025, 2, 1)));
    config
        .parameters
        .insert(age, ParameterValue::Age(CalendarAge::years(65)));
    assert_eq!(
        config
            .with_parameter_value(rate, ParameterValue::Rate(-0.25))
            .unwrap()
            .parameters[&rate],
        ParameterValue::Rate(-0.25)
    );
    assert!(
        config
            .with_parameter_value(rate, ParameterValue::Money(0.2))
            .is_none()
    );
    assert_eq!(
        config
            .with_parameter_value(date, ParameterValue::Date(jiff::civil::date(2025, 3, 1)))
            .unwrap()
            .parameters[&date],
        ParameterValue::Date(jiff::civil::date(2025, 3, 1))
    );
    assert_eq!(
        config
            .with_parameter_value(age, ParameterValue::Age(CalendarAge::new(65, 3)))
            .unwrap()
            .parameters[&age],
        ParameterValue::Age(CalendarAge::new(65, 3))
    );
    assert!(
        config
            .with_parameter_value(age, ParameterValue::Age(CalendarAge::new(65, 12)))
            .is_none()
    );
    let target = |parameter_id| OptimizableParameter::NumericParameter {
        parameter_id,
        min_value: -2.0,
        max_value: 2.0,
    };
    assert_eq!(
        apply_parameters(&config, &[target(rate)], &[-1.25])
            .unwrap()
            .parameters[&rate],
        ParameterValue::Rate(-1.25)
    );
    assert!(apply_parameters(&config, &[target(date)], &[1.0]).is_none());
    assert!(apply_parameters(&config, &[target(age)], &[1.0]).is_none());
    assert_eq!(config.parameters[&rate], ParameterValue::Rate(1.5));
}

#[test]
fn calendar_parameters_bind_nested_recurring_conditions_on_exact_dates() {
    let mut config = config_with_amount(TransferAmount::Fixed(10.0));
    config.duration_years = 1;
    config.birth_date = Some(jiff::civil::date(1960, 1, 15));
    let start = ParameterId(1);
    let end = ParameterId(2);
    config
        .parameters
        .insert(start, ParameterValue::Age(CalendarAge::new(65, 1)));
    config
        .parameters
        .insert(end, ParameterValue::Date(jiff::civil::date(2025, 4, 15)));
    config.events[0].trigger = EventTrigger::Repeating {
        interval: RepeatInterval::Monthly,
        start_condition: Some(Box::new(EventTrigger::And(vec![
            EventTrigger::AgeParameter(start),
            EventTrigger::Date(jiff::civil::date(2025, 2, 1)),
        ]))),
        end_condition: Some(Box::new(EventTrigger::Or(vec![
            EventTrigger::DateParameter(end),
            EventTrigger::Age {
                years: 66,
                months: None,
            },
        ]))),
        max_occurrences: None,
    };
    let original = serde_json::to_value(&config).unwrap();
    let state = SimulationState::from_parameters(&config, 4).unwrap();
    let bound = &state.event_state.get_event(EventId(1)).unwrap().trigger;
    assert!(matches!(
        bound,
        EventTrigger::Repeating {
            start_condition: Some(_),
            end_condition: Some(_),
            ..
        }
    ));
    assert_eq!(serde_json::to_value(&config).unwrap(), original);
    let result = simulate(&config, 4).unwrap();
    let mut literal = config.clone();
    literal.events[0].trigger = bound.clone();
    literal.parameters.clear();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(simulate(&literal, 4).unwrap()).unwrap()
    );
    let mut dates: Vec<_> = result
        .ledger
        .iter()
        .filter(|entry| entry.source_event == Some(EventId(1)))
        .map(|entry| entry.date)
        .collect();
    dates.dedup();
    assert_eq!(
        dates,
        vec![
            jiff::civil::date(2025, 2, 15),
            jiff::civil::date(2025, 3, 15)
        ]
    );

    let equivalent = config
        .with_parameter_value(start, ParameterValue::Age(CalendarAge::new(65, 2)))
        .unwrap();
    let later = simulate(&equivalent, 4).unwrap();
    let mut dates: Vec<_> = later
        .ledger
        .iter()
        .filter(|entry| entry.source_event == Some(EventId(1)))
        .map(|entry| entry.date)
        .collect();
    dates.dedup();
    assert_eq!(dates, vec![jiff::civil::date(2025, 3, 15)]);
}

#[test]
fn two_parameterized_ages_in_one_recurring_event_remain_distinct() {
    let mut config = config_with_amount(TransferAmount::Fixed(10.0));
    config.birth_date = Some(jiff::civil::date(1960, 1, 15));
    config
        .parameters
        .insert(ParameterId(1), ParameterValue::Age(CalendarAge::new(65, 1)));
    config
        .parameters
        .insert(ParameterId(2), ParameterValue::Age(CalendarAge::new(65, 2)));
    config.events[0].trigger = EventTrigger::Repeating {
        interval: RepeatInterval::Monthly,
        start_condition: Some(Box::new(EventTrigger::AgeParameter(ParameterId(1)))),
        end_condition: Some(Box::new(EventTrigger::AgeParameter(ParameterId(2)))),
        max_occurrences: None,
    };
    let result = simulate(&config, 7).unwrap();
    let mut dates: Vec<_> = result
        .ledger
        .iter()
        .filter(|entry| entry.source_event == Some(EventId(1)))
        .map(|entry| entry.date)
        .collect();
    dates.dedup();
    assert_eq!(dates, vec![jiff::civil::date(2025, 2, 15)]);

    let mut literal = config.clone();
    literal.parameters.clear();
    literal.events[0].trigger = EventTrigger::Repeating {
        interval: RepeatInterval::Monthly,
        start_condition: Some(Box::new(EventTrigger::Age {
            years: 65,
            months: Some(1),
        })),
        end_condition: Some(Box::new(EventTrigger::Age {
            years: 65,
            months: Some(2),
        })),
        max_occurrences: None,
    };
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(simulate(&literal, 7).unwrap()).unwrap()
    );

    config.birth_date = Some(jiff::civil::date(1960, 2, 29));
    config
        .parameters
        .insert(ParameterId(1), ParameterValue::Age(CalendarAge::years(65)));
    config.events[0].trigger = EventTrigger::AgeParameter(ParameterId(1));
    let state = SimulationState::from_parameters(&config, 7).unwrap();
    assert!(
        matches!(state.event_state.get_event(EventId(1)).unwrap().trigger,
        EventTrigger::Date(date) if date == jiff::civil::date(2025, 2, 28))
    );
    config.birth_date = Some(jiff::civil::date(1960, 1, 31));
    config
        .parameters
        .insert(ParameterId(1), ParameterValue::Age(CalendarAge::new(65, 1)));
    let state = SimulationState::from_parameters(&config, 7).unwrap();
    assert!(
        matches!(state.event_state.get_event(EventId(1)).unwrap().trigger,
        EventTrigger::Date(date) if date == jiff::civil::date(2025, 2, 28))
    );
}

#[test]
fn recurring_date_parameter_stops_between_scheduled_occurrences() {
    let mut config = config_with_amount(TransferAmount::Fixed(10.0));
    let end = ParameterId(1);
    config
        .parameters
        .insert(end, ParameterValue::Date(jiff::civil::date(2025, 1, 15)));
    config.events[0].trigger = EventTrigger::Repeating {
        interval: RepeatInterval::Monthly,
        start_condition: None,
        end_condition: Some(Box::new(EventTrigger::DateParameter(end))),
        max_occurrences: None,
    };
    let mut state = SimulationState::from_parameters(&config, 7).unwrap();
    assert_eq!(process_events(&mut state), vec![EventId(1)]);
    assert_eq!(
        state.event_state.next_date(EventId(1)),
        Some(jiff::civil::date(2025, 2, 1))
    );
    assert_eq!(state.event_state.next_possible_trigger(EventId(1)), None);

    state.timeline.current_date = jiff::civil::date(2025, 1, 15);
    assert!(process_events(&mut state).is_empty());
    assert_eq!(state.event_state.repeating_active(EventId(1)), None);
    assert_eq!(state.event_state.next_date(EventId(1)), None);
    assert_eq!(state.event_state.next_possible_trigger(EventId(1)), None);
    assert_eq!(state.event_state.occurrence_count(EventId(1)), 1);
}

#[test]
fn invalid_calendar_references_fail_before_run() {
    let id = ParameterId(1);
    let mut config = config_with_amount(TransferAmount::Fixed(1.0));
    config.events[0].trigger = EventTrigger::AgeParameter(id);
    config
        .parameters
        .insert(id, ParameterValue::Age(CalendarAge::years(65)));
    assert!(
        SimulationState::from_parameters(&config, 1)
            .unwrap_err()
            .to_string()
            .contains("birth_date")
    );
    config.birth_date = Some(jiff::civil::date(9999, 12, 1));
    assert!(SimulationState::from_parameters(&config, 1).is_err());
    config.birth_date = Some(jiff::civil::date(1960, 1, 31));
    config
        .parameters
        .insert(id, ParameterValue::Age(CalendarAge::new(65, 12)));
    assert!(SimulationState::from_parameters(&config, 1).is_err());
    config
        .parameters
        .insert(id, ParameterValue::Date(jiff::civil::date(2025, 2, 1)));
    assert!(SimulationState::from_parameters(&config, 1).is_err());
    config.events[0].trigger = EventTrigger::DateParameter(ParameterId(9));
    assert!(SimulationState::from_parameters(&config, 1).is_err());
}
