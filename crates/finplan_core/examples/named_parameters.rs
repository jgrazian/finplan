//! Demonstrates named amount parameters and explicit parameter optimization.

use finplan_core::config::{AccountBuilder, EventBuilder, SimulationBuilder};
use finplan_core::model::{CalendarAge, ParameterValue, TransferAmount};
use finplan_core::optimization::{
    OptimizableParameter, OptimizationAlgorithm, OptimizationConfig, optimize,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let builder = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(30)
        .birth_date(1980, 6, 15)
        .account(AccountBuilder::bank_account("Checking").cash(100_000.0))
        .parameter("monthly_savings", 2_000.0)
        .parameter("employer_match_rate", ParameterValue::Rate(0.05))
        .parameter(
            "savings_start",
            ParameterValue::Date(jiff::civil::date(2025, 2, 15)),
        )
        .parameter(
            "savings_end_age",
            ParameterValue::Age(CalendarAge::years(70)),
        );
    let savings_id = builder
        .parameter_id("monthly_savings")
        .expect("parameter was just registered");
    let match_id = builder
        .parameter_id("employer_match_rate")
        .expect("parameter was just registered");
    let start_id = builder
        .parameter_id("savings_start")
        .expect("registered start date");
    let end_age_id = builder
        .parameter_id("savings_end_age")
        .expect("registered end age");

    let (config, metadata) = builder
        .event(
            EventBuilder::income("Monthly savings")
                .to_account("Checking")
                .transfer_amount(TransferAmount::Add(
                    Box::new(TransferAmount::parameter(savings_id)),
                    Box::new(TransferAmount::Mul(
                        Box::new(TransferAmount::parameter(match_id)),
                        Box::new(TransferAmount::parameter(savings_id)),
                    )),
                ))
                .monthly()
                .starting_on_parameter(start_id)
                .until_age_parameter(end_age_id),
        )
        .build();

    let savings_id = metadata
        .parameter_id("monthly_savings")
        .expect("parameter metadata is returned with the configuration");
    let optimization = OptimizationConfig {
        objective: finplan_core::optimization::OptimizationObjective::MaximizeWealthAtDeath,
        parameters: vec![OptimizableParameter {
            parameter_id: savings_id,
            min_value: ParameterValue::Money(1_000.0),
            max_value: ParameterValue::Money(5_000.0),
        }],
        algorithm: OptimizationAlgorithm::GridSearch { grid_size: 5 },
        monte_carlo_iterations: 10,
        ..Default::default()
    };

    let result = optimize(&config, &optimization, None)?;
    println!(
        "{}: optimal monthly amount ${:.0}",
        metadata
            .parameter_name(savings_id)
            .expect("registered parameter"),
        match result.optimal_parameters.get(&savings_id) {
            Some(ParameterValue::Money(amount)) => *amount,
            _ => return Err("optimizer did not return the Money parameter".into()),
        },
    );
    Ok(())
}
