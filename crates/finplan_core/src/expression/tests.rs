use super::*;
use crate::{
    config::{SimulationBuilder, SimulationConfig},
    evaluate::{EvalEvent, evaluate_effect},
    model::{
        Account, AccountFlavor, AssetCoord, CalendarAge, Event, EventId, EventTrigger, IncomeType,
        LoanDetail, LotMethod, TransferEndpoint, WithdrawalSources,
    },
    simulation::simulate,
    simulation_state::SimulationState,
};
use jiff::civil::date;

fn fixture() -> (SimulationConfig, SimulationMetadata) {
    SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(2)
        .birth_date(1960, 6, 15)
        .inflation(0.1)
        .asset_fixed("VTI", 100.0, 0.0)
        .brokerage("Vanguard", 1_000.0)
        .bank("Checking", 100.0)
        .position("Vanguard", "VTI", 10.0, 800.0)
        .parameter("MonthlySpending", 500.0)
        .parameter("WithdrawalRate", ParameterValue::Rate(0.2))
        .parameter("RetirementDate", ParameterValue::Date(date(2026, 1, 1)))
        .parameter(
            "RetirementAge",
            ParameterValue::Age(CalendarAge::new(65, 6)),
        )
        .build()
}

fn eval(source: &str) -> Result<f64, ExpressionError> {
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    Expression::compile(source, &metadata, &config.parameters)?
        .bind_parameters(&config.parameters, config.birth_date)?
        .evaluate(&EvaluationContext::new(&state).with_endpoints(
            TransferEndpoint::Cash {
                account_id: metadata.account_id("Vanguard").unwrap(),
            },
            TransferEndpoint::Cash {
                account_id: metadata.account_id("Checking").unwrap(),
            },
        ))
}

fn eval_bool(source: &str) -> Result<bool, ExpressionError> {
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    Expression::compile(source, &metadata, &config.parameters)?
        .bind_parameters(&config.parameters, config.birth_date)?
        .evaluate_bool(&EvaluationContext::new(&state).with_endpoints(
            TransferEndpoint::Cash {
                account_id: metadata.account_id("Vanguard").unwrap(),
            },
            TransferEndpoint::Cash {
                account_id: metadata.account_id("Checking").unwrap(),
            },
        ))
}

#[test]
fn conditions_support_comparisons_boolean_operators_and_precedence() {
    for (source, expected) in [
        ("true", true),
        ("false", false),
        ("1 + 2 * 3 == 7", true),
        ("1 < 2", true),
        ("2 <= 2", true),
        ("3 > 2", true),
        ("3 >= 3", true),
        ("2 != 3", true),
        ("2 == 3", false),
        ("true == (not false)", true),
        ("true != false", true),
        ("(1 < 2) == true", true),
        ("not age() >= 65", true),
        ("not (age() >= 65)", true),
        ("false or true and false", false),
        ("(false or true) and false", false),
        ("true or false and false", true),
        ("not false and false or true", true),
        ("$MonthlySpending < balance(\"Vanguard\")", true),
        ("$WithdrawalRate >= 20%", true),
    ] {
        assert_eq!(eval_bool(source).unwrap(), expected, "{source}");
    }
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let condition = Expression::compile("true", &metadata, &config.parameters).unwrap();
    assert!(condition.evaluate(&EvaluationContext::new(&state)).is_err());
    assert!(compile_amount("true", &metadata, &config.parameters).is_err());
}

#[test]
fn if_selects_typed_branches_and_supports_nested_conditions() {
    for (source, expected) in [
        ("if(true, 10, 20)", 10.0),
        ("if(false, 10, 20)", 20.0),
        ("if(age() >= 65, 100, 200)", 200.0),
        ("if($WithdrawalRate > 10%, balance(source), 0)", 2000.0),
        ("if(true, if(false, 1, 2), 3)", 2.0),
        ("if(if(true, false, true), 1, 2)", 2.0),
    ] {
        assert_eq!(eval(source).unwrap(), expected, "{source}");
    }
    for (source, expected) in [
        ("if(true, false, true)", false),
        ("if(false, true, 1 < 2)", true),
    ] {
        assert_eq!(eval_bool(source).unwrap(), expected, "{source}");
    }
}

#[test]
fn boolean_and_conditional_type_errors_are_reported_at_compile_time() {
    let (config, metadata) = fixture();
    for source in [
        "if()",
        "if(true)",
        "if(true, 1)",
        "if(true, 1, 2, 3)",
        "if(1, 2, 3)",
        "if(true, 1, false)",
        "if(true, $MonthlySpending, $WithdrawalRate)",
        "true and 1",
        "1 or false",
        "not 1",
        "+true",
        "-false",
        "if(+(1 < 2), 1, 2)",
        "true + false",
        "true * 1",
        "true%",
        "min(true, false)",
        "1 < true",
        "true < false",
        "$MonthlySpending < $WithdrawalRate",
        "$MonthlySpending == $WithdrawalRate",
        "true == 1",
        "1 < 2 < 3",
        "1 == 1 == true",
        "if(true, $Missing, 1)",
        "if(false, balance(\"Missing\"), 1)",
    ] {
        let error = Expression::compile(source, &metadata, &config.parameters).unwrap_err();
        assert!(
            error.span.start <= error.span.end && error.span.end <= source.len(),
            "{source}: {error}"
        );
    }
    assert!(
        Expression::compile("1", &metadata, &config.parameters)
            .unwrap()
            .evaluate_bool(&EvaluationContext::new(
                &SimulationState::from_parameters(&config, 42).unwrap()
            ))
            .is_err()
    );
}

#[test]
fn conditional_and_boolean_operators_skip_unselected_runtime_errors() {
    for (source, expected) in [
        ("if(true, 5, 1 / 0)", 5.0),
        ("if(false, 1e308 * 1e308, 7)", 7.0),
        ("if(true, 9, holding(target, \"VTI\"))", 9.0),
        (
            "if(false, holding(target, \"VTI\"), if(true, 8, 1 / 0))",
            8.0,
        ),
    ] {
        assert_eq!(eval(source).unwrap(), expected, "{source}");
    }
    for (source, expected) in [
        ("false and 1 / 0 > 0", false),
        ("true or 1e308 * 1e308 > 0", true),
        ("true or holding(target, \"VTI\") > 0", true),
    ] {
        assert_eq!(eval_bool(source).unwrap(), expected, "{source}");
    }
    for source in ["if(true, 1 / 0, 2)", "if(false, 1, 1 / 0)"] {
        assert!(eval(source).is_err(), "{source}");
    }
    assert!(eval_bool("true and 1 / 0 > 0").is_err());
    assert!(eval_bool("false or holding(target, \"VTI\") > 0").is_err());
}

#[test]
fn conditional_amounts_round_trip_through_rendering_and_serialization() {
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let context = EvaluationContext::new(&state).with_endpoints(
        TransferEndpoint::Cash {
            account_id: metadata.account_id("Vanguard").unwrap(),
        },
        TransferEndpoint::Cash {
            account_id: metadata.account_id("Checking").unwrap(),
        },
    );
    for source in [
        "if(age() >= 65 or $WithdrawalRate > 10%, balance(source), 0)",
        "if(not (age() >= 65), if(true, $MonthlySpending, 0), balance(target))",
        "if(+age() < 65, +$MonthlySpending, 0)",
    ] {
        let expression = Expression::compile(source, &metadata, &config.parameters).unwrap();
        let rendered = expression.to_source(&metadata).unwrap();
        let encoded = serde_json::to_value(&expression).unwrap();
        let decoded: Expression = serde_json::from_value(encoded).unwrap();
        for form in [
            expression,
            decoded,
            Expression::compile(&rendered, &metadata, &config.parameters).unwrap(),
        ] {
            let value = form
                .bind_parameters(&config.parameters, config.birth_date)
                .unwrap()
                .evaluate(&context)
                .unwrap();
            assert_eq!(value, eval(source).unwrap(), "{source} -> {rendered}");
        }
    }
    for source in [
        "if(true, false, true)",
        "not (age() >= 65) and (true or false)",
    ] {
        let rendered = Expression::compile(source, &metadata, &config.parameters)
            .unwrap()
            .to_source(&metadata)
            .unwrap();
        assert_eq!(
            eval_bool(source).unwrap(),
            eval_bool(&rendered).unwrap(),
            "{rendered}"
        );
    }
}

#[test]
fn conditional_amounts_rebind_each_run_and_validate_unused_branches() {
    let (mut config, metadata) = fixture();
    let from = metadata.account_id("Vanguard").unwrap();
    let to = metadata.account_id("Checking").unwrap();
    let source = "if($WithdrawalRate > 10%, $MonthlySpending, 100)";
    let amount = compile_amount(source, &metadata, &config.parameters)
        .unwrap()
        .amount;
    config.events.push(Event {
        event_id: EventId(1),
        trigger: EventTrigger::Date(date(2025, 1, 1)),
        effects: vec![EventEffect::CashTransfer { from, to, amount }],
        once: true,
    });
    let encoded = serde_json::to_value(&config.events).unwrap();
    let mut decoded = config.clone();
    decoded.events = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(
        simulate(&decoded, 42).unwrap().final_account_balance(to),
        Some(600.0)
    );
    assert_eq!(serde_json::to_value(&config.events).unwrap(), encoded);

    let rate = metadata.parameter_id("WithdrawalRate").unwrap();
    let spending = metadata.parameter_id("MonthlySpending").unwrap();
    config.parameters.insert(rate, ParameterValue::Rate(0.05));
    assert_eq!(
        simulate(&config, 42).unwrap().final_account_balance(to),
        Some(200.0)
    );
    config.parameters.remove(&spending);
    assert!(SimulationState::from_parameters(&config, 42).is_err());
    config
        .parameters
        .insert(spending, ParameterValue::Rate(500.0));
    assert!(SimulationState::from_parameters(&config, 42).is_err());
}

#[test]
fn multiplier_sweeps_preserve_conditional_operands() {
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let context = EvaluationContext::new(&state);
    for source in [
        "4% * if(age() < 65 and not false, $MonthlySpending, 1 / 0)",
        "if(age() < 65, $MonthlySpending, 1 / 0) * 4%",
        "+4% * if(true, $MonthlySpending, 0)",
    ] {
        let amount = compile_amount(source, &metadata, &config.parameters)
            .unwrap()
            .amount;
        let updated = amount.with_scale_factor(0.1).unwrap();
        for (form, expected) in [(amount, 20.0), (updated, 50.0)] {
            let result = form
                .bind_parameters(&config.parameters, config.birth_date)
                .unwrap()
                .expression()
                .evaluate(&context)
                .unwrap();
            assert_eq!(result, expected, "{source}");
        }
    }
    let computed_factor = compile_amount(
        "if(true, 4%, 3%) * $MonthlySpending",
        &metadata,
        &config.parameters,
    )
    .unwrap()
    .amount;
    assert!(computed_factor.with_scale_factor(0.1).is_err());
}

#[test]
fn malformed_serialized_condition_programs_are_rejected() {
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let context = EvaluationContext::new(&state);
    for source in ["if(true, 1, 2)", "true and false"] {
        let mut encoded = serde_json::to_value(
            Expression::compile(source, &metadata, &config.parameters).unwrap(),
        )
        .unwrap();
        let program = encoded["program"].as_array_mut().unwrap();
        let operator = program.pop().unwrap();
        *program = vec![operator];
        let expression: Expression = serde_json::from_value(encoded).unwrap();
        assert!(expression.evaluate(&context).is_err(), "{source}");
        assert!(expression.evaluate_bool(&context).is_err(), "{source}");
        assert!(expression.into_amount().is_err(), "{source}");
    }
}

#[test]
fn long_flat_boolean_expressions_evaluate_without_recursive_stack_growth() {
    let (config, metadata) = fixture();
    let source = format!("{}true", "false or ".repeat(300));
    let expression = Expression::compile(&source, &metadata, &config.parameters).unwrap();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    assert!(
        expression
            .evaluate_bool(&EvaluationContext::new(&state))
            .unwrap()
    );
}

#[test]
fn examples_arithmetic_precedence_and_percentages() {
    for (source, expected) in [
        ("5000", 5000.0),
        ("$MonthlySpending", 500.0),
        ("$WithdrawalRate * balance(\"Vanguard\")", 400.0),
        ("0.5 * holding(\"Vanguard\", \"VTI\")", 500.0),
        ("10%", 0.1),
        ("10% * 5000", 500.0),
        ("1 + 2 * 3", 7.0),
        ("(1 + 2) * 3", 9.0),
        ("10 - 3 - 2", 5.0),
        ("100 / 10 / 2", 5.0),
        ("-2 * +3", -6.0),
        ("-(2 + 3)", -5.0),
        (".5e2 + 1E-1", 50.1),
        ("(5 + 5)%", 0.1),
        ("$MonthlySpending / 12", 500.0 / 12.0),
        ("balance(source) / balance(target)", 20.0),
        ("min(100, max(40, 50))", 50.0),
        ("clamp(500, 100, 200)", 200.0),
        ("abs(-500)", 500.0),
        ("net_worth()", 2100.0),
        ("cash(source)", 1000.0),
        ("source_balance()", 1000.0),
        ("target_balance()", 100.0),
        ("top_up($MonthlySpending)", 400.0),
        ("top_up(50)", 0.0),
        ("payoff()", 0.0),
        ("year()", 2025.0),
        ("month()", 1.0),
        ("years_since_start()", 0.0),
        ("days_until($RetirementDate)", 365.0),
        ("years_until(\"2026-01-01\")", 365.0 / 365.2425),
        ("days_until(\"2024-12-31\")", -1.0),
        ("age_years($RetirementAge)", 65.5),
    ] {
        let actual = eval(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        assert!(
            (actual - expected).abs() < 1e-9,
            "{source}: {actual} != {expected}"
        );
    }
}

#[test]
fn errors_identify_bad_syntax_names_arity_and_types() {
    let (config, metadata) = fixture();
    for source in [
        "",
        "1 2",
        "1 +",
        "(1",
        "1)",
        "1e",
        "1e999",
        ".",
        "NaN",
        "2 ^ 3",
        "min(1)",
        "max(1,2,3)",
        "clamp(1,2)",
        "balance()",
        "age(1)",
        "mystery(1)",
        "$Missing",
        "$",
        "balance(\"Missing\")",
        "balance(Vanguard)",
        "holding(source, \"Missing\")",
        "holding(source, 2)",
        "balance(\"oops)",
        "days_until(\"2025-02-30\")",
        "days_until($MonthlySpending)",
        "age_years($MonthlySpending)",
        "$RetirementDate + 1",
        "$RetirementAge + 1",
        "$MonthlySpending + $WithdrawalRate",
        "$MonthlySpending * $MonthlySpending",
        "inflation($WithdrawalRate)",
        "$MonthlySpending + 10%",
        "$MonthlySpending%",
        "endpoint_balance(\"Vanguard\")",
        "1 + net(2)",
        "net(gross(2))",
    ] {
        let error = Expression::compile(source, &metadata, &config.parameters).unwrap_err();
        assert!(
            error.span.start <= error.span.end && error.span.end <= source.len(),
            "{source}: {error}"
        );
    }
    for source in [
        "10%",
        "$WithdrawalRate",
        "age()",
        "balance(source) / balance(target)",
    ] {
        assert!(
            compile_amount(source, &metadata, &config.parameters).is_err(),
            "{source}"
        );
    }
    let error = Expression::compile("1 + $Missing", &metadata, &config.parameters).unwrap_err();
    assert_eq!(error.span, 5..12);
}

#[test]
fn runtime_rejects_nonfinite_values_and_invalid_operations() {
    for (source, message) in [
        ("1 / 0", "division by zero"),
        ("1 / -0", "division by zero"),
        ("1e308 * 1e308", "non-finite"),
        ("clamp(1, 3, 2)", "minimum exceeds maximum"),
        ("holding(target, \"VTI\")", "not found"),
    ] {
        assert!(
            eval(source).unwrap_err().message.contains(message),
            "{source}"
        );
    }
}

#[test]
fn account_endpoint_and_holding_balances_are_distinct() {
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let context = EvaluationContext::new(&state).with_endpoints(
        TransferEndpoint::Asset {
            asset_coord: AssetCoord {
                account_id: metadata.account_id("Vanguard").unwrap(),
                asset_id: metadata.asset_id("VTI").unwrap(),
            },
        },
        TransferEndpoint::External,
    );
    for (source, expected) in [
        ("balance(source)", 2000.0),
        ("cash(source)", 1000.0),
        ("endpoint_balance(source)", 1000.0),
    ] {
        assert_eq!(
            Expression::compile(source, &metadata, &config.parameters)
                .unwrap()
                .evaluate(&context)
                .unwrap(),
            expected
        );
    }
    for source in ["balance(target)", "cash(target)", "top_up(100)"] {
        assert!(
            Expression::compile(source, &metadata, &config.parameters)
                .unwrap()
                .evaluate(&context)
                .is_err()
        );
    }
}

#[test]
fn debt_payoff_includes_liabilities_in_net_worth() {
    let (mut config, mut metadata) = fixture();
    let debt = AccountId(99);
    config.accounts.push(Account {
        account_id: debt,
        flavor: AccountFlavor::Liability(LoanDetail {
            principal: 750.0,
            interest_rate: 0.0,
            repayment: None,
            schedule: None,
        }),
    });
    metadata.register_account(debt, Some("Debt".into()), None);
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let context = EvaluationContext::new(&state).with_endpoints(
        TransferEndpoint::External,
        TransferEndpoint::Cash { account_id: debt },
    );
    for (source, expected) in [
        ("payoff()", 750.0),
        ("balance(\"Debt\")", -750.0),
        ("net_worth()", 1350.0),
    ] {
        assert_eq!(
            Expression::compile(source, &metadata, &config.parameters)
                .unwrap()
                .evaluate(&context)
                .unwrap(),
            expected
        );
    }
}

#[test]
fn inflation_uses_live_market_and_dates() {
    let (config, metadata) = fixture();
    let mut state = SimulationState::from_parameters(&config, 42).unwrap();
    state.timeline.current_date = date(2026, 1, 1);
    let amount = Expression::compile("inflation($MonthlySpending)", &metadata, &config.parameters)
        .unwrap()
        .bind_parameters(&config.parameters, config.birth_date)
        .unwrap();
    let actual = amount.evaluate(&EvaluationContext::new(&state)).unwrap();
    assert!((actual - 550.0).abs() < 1e-6, "{actual}");
}

#[test]
fn age_uses_completed_calendar_months_and_clamped_anniversaries() {
    let (config, metadata) = fixture();
    let mut state = SimulationState::from_parameters(&config, 42).unwrap();
    let expression = Expression::compile("age()", &metadata, &config.parameters).unwrap();
    for (today, expected) in [
        (date(2025, 6, 14), 64.0 + 11.0 / 12.0),
        (date(2025, 6, 15), 65.0),
        (date(2025, 12, 15), 65.5),
    ] {
        state.timeline.current_date = today;
        assert_eq!(
            expression
                .evaluate(&EvaluationContext::new(&state))
                .unwrap(),
            expected
        );
    }
    state.timeline.birth_date = date(1960, 2, 29);
    state.timeline.current_date = date(2025, 2, 28);
    assert_eq!(
        expression
            .evaluate(&EvaluationContext::new(&state))
            .unwrap(),
        65.0
    );
    assert!(
        expression
            .bind_parameters(&config.parameters, None)
            .is_err()
    );
}

#[test]
fn names_with_spaces_unicode_and_escapes_round_trip() {
    let (mut config, mut metadata) = fixture();
    metadata.register_account(
        metadata.account_id("Vanguard").unwrap(),
        Some("Épargne \"taxable\"\\账户".into()),
        None,
    );
    let id = ParameterId(99);
    metadata.register_parameter(id, Some("Monthly spending".into()), None);
    config.parameters.insert(id, ParameterValue::Money(42.0));
    let source = r#"$"Monthly spending" + cash("Épargne \"taxable\"\\账户")"#;
    let expression = Expression::compile(source, &metadata, &config.parameters).unwrap();
    let rendered = expression.to_source(&metadata).unwrap();
    let compiled = Expression::compile(&rendered, &metadata, &config.parameters).unwrap();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    assert_eq!(
        compiled
            .bind_parameters(&config.parameters, config.birth_date)
            .unwrap()
            .evaluate(&EvaluationContext::new(&state))
            .unwrap(),
        1042.0
    );
}

#[test]
fn parameters_rebind_per_run_without_mutating_the_expression() {
    let (mut config, metadata) = fixture();
    let from = metadata.account_id("Vanguard").unwrap();
    let to = metadata.account_id("Checking").unwrap();
    let mut effect = EventEffect::CashTransfer {
        from,
        to,
        amount: TransferAmount::fixed(0.0),
    };
    compile_amount(
        "$WithdrawalRate * balance(source)",
        &metadata,
        &config.parameters,
    )
    .unwrap()
    .apply_to(&mut effect)
    .unwrap();
    config.events.push(Event {
        event_id: EventId(1),
        trigger: EventTrigger::Date(date(2025, 1, 1)),
        effects: vec![effect],
        once: true,
    });
    let encoded = serde_json::to_value(&config.events).unwrap();
    let mut decoded = config.clone();
    decoded.events = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(
        simulate(&decoded, 42).unwrap().final_account_balance(to),
        Some(500.0)
    );
    assert_eq!(serde_json::to_value(&config.events).unwrap(), encoded);
    let rate = metadata.parameter_id("WithdrawalRate").unwrap();
    config.parameters.insert(rate, ParameterValue::Rate(0.3));
    assert_eq!(
        simulate(&config, 42).unwrap().final_account_balance(to),
        Some(700.0)
    );
    config.parameters.insert(rate, ParameterValue::Money(0.3));
    assert!(
        SimulationState::from_parameters(&config, 42)
            .unwrap_err()
            .to_string()
            .contains("changed type")
    );
    config.parameters.remove(&rate);
    assert!(SimulationState::from_parameters(&config, 42).is_err());
}

#[test]
fn gross_net_annotations_use_existing_tax_effects_and_fail_atomically() {
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let to = metadata.account_id("Checking").unwrap();
    for (source, mode) in [
        ("gross(5000)", AmountMode::Gross),
        ("net(5000)", AmountMode::Net),
    ] {
        let expected = EventEffect::Income {
            to,
            amount: TransferAmount::fixed(5000.0),
            amount_mode: mode,
            income_type: IncomeType::Taxable,
        };
        let mut effect = expected.clone();
        compile_amount(source, &metadata, &config.parameters)
            .unwrap()
            .apply_to(&mut effect)
            .unwrap();
        assert_eq!(
            format!("{:?}", evaluate_effect(&effect, &state).unwrap()),
            format!("{:?}", evaluate_effect(&expected, &state).unwrap())
        );
    }
    let mut expense = EventEffect::Expense {
        from: to,
        amount: TransferAmount::fixed(3.0),
    };
    let before = serde_json::to_value(&expense).unwrap();
    assert!(
        compile_amount("gross(1)", &metadata, &config.parameters)
            .unwrap()
            .apply_to(&mut expense)
            .is_err()
    );
    assert_eq!(serde_json::to_value(&expense).unwrap(), before);
}

#[test]
fn effects_supply_correct_source_and_target_context() {
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let from = metadata.account_id("Vanguard").unwrap();
    let to = metadata.account_id("Checking").unwrap();
    let amount = |source| {
        compile_amount(source, &metadata, &config.parameters)
            .unwrap()
            .amount
    };
    let transfer = EventEffect::CashTransfer {
        from,
        to,
        amount: amount("top_up(500)"),
    };
    assert!(evaluate_effect(&transfer, &state).unwrap().iter().any(
        |effect| matches!(effect, EvalEvent::CashCredit { net_amount, .. } if *net_amount == 400.0)
    ));
    let adjust = EventEffect::AdjustBalance {
        account: to,
        amount: amount("balance(target)"),
    };
    assert!(matches!(
        evaluate_effect(&adjust, &state).unwrap()[0],
        EvalEvent::AdjustBalance { delta: 100.0, .. }
    ));
    let sale = EventEffect::AssetSale {
        from,
        asset_id: metadata.asset_id("VTI"),
        amount: amount("10% * holding(source, \"VTI\")"),
        amount_mode: AmountMode::Gross,
        lot_method: LotMethod::Fifo,
    };
    assert!(!evaluate_effect(&sale, &state).unwrap().is_empty());
    let sweep = |sources| EventEffect::Sweep {
        sources,
        to,
        amount: amount("10% * balance(source)"),
        amount_mode: AmountMode::Gross,
        lot_method: LotMethod::Fifo,
        income_type: IncomeType::Taxable,
    };
    assert!(evaluate_effect(&sweep(WithdrawalSources::SingleAccount(from)), &state).is_ok());
    assert!(
        evaluate_effect(&sweep(WithdrawalSources::default()), &state)
            .unwrap_err()
            .to_string()
            .contains("no single account")
    );
}

#[test]
fn account_wide_sales_calculate_holdings_without_a_synthetic_endpoint() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(1)
        .birth_date(1960, 1, 1)
        .asset_fixed("VTI", 100.0, 0.0)
        .asset_fixed("BND", 100.0, 0.0)
        .brokerage("Brokerage", 0.0)
        .bank("Checking", 0.0)
        .position("Brokerage", "VTI", 10.0, 1_000.0)
        .position("Brokerage", "BND", 5.0, 500.0)
        .build();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let from = metadata.account_id("Brokerage").unwrap();
    let to = metadata.account_id("Checking").unwrap();
    let vti = metadata.asset_id("VTI").unwrap();
    let bnd = metadata.asset_id("BND").unwrap();

    let context = EvaluationContext::new(&state).with_source_account(from);
    assert_eq!(
        Expression::compile(
            "balance(source) - cash(source)",
            &metadata,
            &config.parameters
        )
        .unwrap()
        .evaluate(&context)
        .unwrap(),
        1500.0
    );
    assert!(
        Expression::compile("source_balance()", &metadata, &config.parameters)
            .unwrap()
            .evaluate(&context)
            .unwrap_err()
            .message
            .contains("no single endpoint")
    );

    let sale = EventEffect::AssetSale {
        from,
        asset_id: None,
        amount: TransferAmount::account_balance(AccountRef::Source)
            .minus(TransferAmount::cash_balance(AccountRef::Source)),
        amount_mode: AmountMode::Gross,
        lot_method: LotMethod::Fifo,
    };
    let sold = evaluate_effect(&sale, &state).unwrap();
    for asset_id in [vti, bnd] {
        assert!(sold.iter().any(|event| matches!(
            event,
            EvalEvent::SubtractAssetLot { from: coord, .. } if coord.asset_id == asset_id
        )));
    }

    let sweep = EventEffect::Sweep {
        sources: WithdrawalSources::SingleAccount(from),
        to,
        amount: TransferAmount::account_balance(AccountRef::Source)
            .minus(TransferAmount::cash_balance(AccountRef::Source)),
        amount_mode: AmountMode::Gross,
        lot_method: LotMethod::Fifo,
        income_type: IncomeType::Taxable,
    };
    let swept = evaluate_effect(&sweep, &state).unwrap();
    for asset_id in [vti, bnd] {
        assert!(swept.iter().any(|event| matches!(
            event,
            EvalEvent::SubtractAssetLot { from: coord, .. } if coord.asset_id == asset_id
        )));
    }

    let single_asset = EventEffect::Sweep {
        sources: WithdrawalSources::SingleAsset(AssetCoord {
            account_id: from,
            asset_id: bnd,
        }),
        to,
        amount: TransferAmount::source_balance(),
        amount_mode: AmountMode::Gross,
        lot_method: LotMethod::Fifo,
        income_type: IncomeType::Taxable,
    };
    let sold = evaluate_effect(&single_asset, &state).unwrap();
    assert!(sold.iter().any(|event| matches!(
        event,
        EvalEvent::SubtractAssetLot { from: coord, .. } if coord.asset_id == bnd
    )));
    assert!(!sold.iter().any(|event| matches!(
        event,
        EvalEvent::SubtractAssetLot { from: coord, .. } if coord.asset_id == vti
    )));
}

#[test]
fn rust_constructors_and_dsl_share_evaluation_and_serialization() {
    let (config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let from = metadata.account_id("Vanguard").unwrap();
    let to = metadata.account_id("Checking").unwrap();
    let fixed = || TransferAmount::fixed(50.0);
    let amounts = vec![
        (TransferAmount::fixed(42.0), 42.0),
        (TransferAmount::source_balance(), 1000.0),
        (TransferAmount::target_balance(), 100.0),
        (TransferAmount::top_up(500.0), 400.0),
        (fixed().inflated(), 50.0),
        (TransferAmount::account_balance(from), 2000.0),
        (TransferAmount::cash_balance(from), 1000.0),
        (
            TransferAmount::holding_balance(AssetCoord {
                account_id: from,
                asset_id: metadata.asset_id("VTI").unwrap(),
            }),
            1000.0,
        ),
        (TransferAmount::scaled(0.5, fixed()), 25.0),
        (fixed().min(fixed()), 50.0),
        (fixed().max(fixed()), 50.0),
        (fixed().plus(fixed()), 100.0),
        (fixed().minus(fixed()), 0.0),
        (TransferAmount::net_worth(), 2100.0),
        (TransferAmount::up_to(2000.0), 1000.0),
        (TransferAmount::excess_above(400.0), 600.0),
        (TransferAmount::payoff(), 0.0),
        (TransferAmount::percent_of_account(0.1, from), 200.0),
        (
            TransferAmount::scaled_rate(metadata.parameter_id("WithdrawalRate").unwrap(), fixed()),
            10.0,
        ),
    ];
    let context = EvaluationContext::new(&state).with_endpoints(
        TransferEndpoint::Cash { account_id: from },
        TransferEndpoint::Cash { account_id: to },
    );
    for (amount, expected) in amounts {
        let source = amount.to_source(&metadata).unwrap();
        let compiled = compile_amount(&source, &metadata, &config.parameters)
            .unwrap()
            .amount;
        let encoded = serde_json::to_value(&amount).unwrap();
        assert!(
            encoded.get("program").is_some(),
            "amount must serialize as an expression"
        );
        let decoded: TransferAmount = serde_json::from_value(encoded).unwrap();
        for form in [amount, compiled, decoded] {
            let bound = form
                .bind_parameters(&config.parameters, config.birth_date)
                .unwrap();
            assert_eq!(
                bound.expression().evaluate(&context).unwrap(),
                expected,
                "{source}"
            );
        }
    }
    assert_eq!(
        TransferAmount::parameter(metadata.parameter_id("MonthlySpending").unwrap())
            .to_source(&metadata)
            .unwrap(),
        "$MonthlySpending"
    );
}

#[test]
fn amount_deserialization_validates_money_and_rejects_old_enum_forms() {
    let (config, metadata) = fixture();
    for source in ["10%", "$WithdrawalRate", "age()"] {
        let expression = Expression::compile(source, &metadata, &config.parameters).unwrap();
        assert!(
            serde_json::from_value::<TransferAmount>(serde_json::to_value(expression).unwrap())
                .is_err(),
            "{source}"
        );
    }
    for json in [
        r#"{"Fixed": 5000.0}"#,
        r#"{"Parameter": 0}"#,
        r#""SourceBalance""#,
        r#"{"program":[]}"#,
    ] {
        assert!(
            serde_json::from_str::<TransferAmount>(json).is_err(),
            "{json}"
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            TransferAmount::fixed(value)
                .bind_parameters(&config.parameters, config.birth_date)
                .is_err()
        );
        assert!(
            TransferAmount::scaled(value, TransferAmount::fixed(1.0))
                .bind_parameters(&config.parameters, config.birth_date)
                .is_err()
        );
    }
}

#[test]
fn rendered_compiled_expressions_round_trip() {
    let (config, metadata) = fixture();
    for source in [
        "-$MonthlySpending + clamp(10% * balance(source), 100, 500)",
        "max(top_up(500), payoff())",
        "inflation(min(100, abs(-200))) / 12",
        "holding(source, \"VTI\")",
        "days_until($RetirementDate) + age_years($RetirementAge)",
        "age() + year() + month() + years_since_start()",
        "years_until(\"2026-01-01\")",
    ] {
        let expression = Expression::compile(source, &metadata, &config.parameters).unwrap();
        let rendered = expression.to_source(&metadata).unwrap();
        assert_eq!(
            eval(source).unwrap(),
            eval(&rendered).unwrap(),
            "{rendered}"
        );
    }
}

#[test]
fn complexity_limits_and_invalid_serialized_programs_are_errors() {
    let (config, metadata) = fixture();
    for source in [
        format!("{}1{}", "(".repeat(100), ")".repeat(100)),
        "1+".repeat(1500) + "1",
        " ".repeat(16_385),
    ] {
        assert!(Expression::compile(&source, &metadata, &config.parameters).is_err());
    }
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    for json in [
        r#"{"program":[]}"#,
        r#"{"program":[{"op":"Add","span":{"start":0,"end":1}}]}"#,
    ] {
        let expression: Expression = serde_json::from_str(json).unwrap();
        assert!(
            expression
                .evaluate(&EvaluationContext::new(&state))
                .is_err()
        );
        assert!(expression.into_amount().is_err());
    }
}

#[test]
fn calendar_parameters_rebind_and_unbound_references_are_errors() {
    let (mut config, metadata) = fixture();
    let state = SimulationState::from_parameters(&config, 42).unwrap();
    let context = EvaluationContext::new(&state);
    let expression = Expression::compile(
        "days_until($RetirementDate) + age_years($RetirementAge)",
        &metadata,
        &config.parameters,
    )
    .unwrap();
    assert!(
        expression
            .evaluate(&context)
            .unwrap_err()
            .message
            .contains("not bound")
    );
    config.parameters.insert(
        metadata.parameter_id("RetirementDate").unwrap(),
        ParameterValue::Date(date(2025, 1, 2)),
    );
    config.parameters.insert(
        metadata.parameter_id("RetirementAge").unwrap(),
        ParameterValue::Age(CalendarAge::years(70)),
    );
    assert_eq!(
        expression
            .bind_parameters(&config.parameters, config.birth_date)
            .unwrap()
            .evaluate(&context)
            .unwrap(),
        71.0
    );
    config.parameters.insert(
        metadata.parameter_id("RetirementDate").unwrap(),
        ParameterValue::Money(1.0),
    );
    assert!(
        expression
            .bind_parameters(&config.parameters, config.birth_date)
            .is_err()
    );
}

#[test]
fn initialization_validates_expressions_in_nested_amounts_and_random_branches() {
    let (mut config, metadata) = fixture();
    let amount = compile_amount("$MonthlySpending * age()", &metadata, &config.parameters)
        .unwrap()
        .amount;
    let account = metadata.account_id("Checking").unwrap();
    config.events.push(Event {
        event_id: EventId(1),
        trigger: EventTrigger::Date(date(2025, 1, 1)),
        once: true,
        effects: vec![EventEffect::Random {
            probability: 1.0,
            on_true: Box::new(EventEffect::AdjustBalance {
                account,
                amount: TransferAmount::fixed(0.0),
            }),
            on_false: Some(Box::new(EventEffect::AdjustBalance {
                account,
                amount: amount.inflated(),
            })),
        }],
    });
    config.birth_date = None;
    assert!(
        SimulationState::from_parameters(&config, 42)
            .unwrap_err()
            .to_string()
            .contains("birth_date")
    );
    config.birth_date = Some(date(1960, 1, 1));
    config
        .parameters
        .remove(&metadata.parameter_id("MonthlySpending").unwrap());
    assert!(
        SimulationState::from_parameters(&config, 42)
            .unwrap_err()
            .to_string()
            .contains("parameter")
    );
}
