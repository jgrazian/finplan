//! Guided setup: the "answer a few questions" way to make a plan.
//!
//! A [`SetupPlan`] is the answers; [`lower`] turns them into the changes that
//! write the accounts, holdings, parameters and events they describe, with the
//! same template builders the drafting agent uses; [`create_plan`] makes the
//! plan from them in memory. The server (`POST /scenarios/setup`) validates and
//! lowers with the same functions and writes the result to SQLite; the
//! browser calls [`create_plan`] and stores the graph.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::create::new_plan;
use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;
use crate::library::Library;
use crate::specs::Interval;
use crate::specs::parameters::ParameterValueSpec;
use crate::specs::scenarios::CreateScenario;
use crate::suggest::{Change, Created, resolve_steps};
use crate::templates::{
    Allocation, Employee401k, RecurringExpenseParams, RowRef, SalaryParams, Template, When,
    allocation_asset, bank_account, expand_template, investment_account, parameter, position,
};

/// The employee 401(k) deferral limit guided setup caps a contribution at: the
/// 2026 figure (IRS Notice 2025-67). The server's reference table
/// (`suggest::ai::tools::facts`) holds the same number for every year; a test
/// there keeps the two equal.
pub const GUIDED_SETUP_401K_DEFERRAL_LIMIT: f64 = 24_500.0;

/// The parameters guided setup's events follow, so Analysis can vary them and
/// the Plan tab edits each in one place.
pub const RETIREMENT_AGE: &str = "Retirement age";
pub const MONTHLY_SPENDING: &str = "Monthly spending";

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetupPlan {
    pub request_id: String,
    pub name: String,
    pub start_date: String,
    pub birth_date: String,
    pub duration_years: i64,
    pub retirement_age: u8,
    pub cash: f64,
    pub retirement_401k: f64,
    pub investments: f64,
    pub stock_percent: f64,
    pub cash_profile_id: i64,
    pub stock_profile_id: i64,
    pub bond_profile_id: i64,
    pub investment_tax_status: String,
    pub annual_income: f64,
    pub retirement_401k_contribution_percent: f64,
    pub annual_spending: f64,
    pub retirement_spending: f64,
    pub inflation_profile_id: Option<i64>,
    pub tax_config_id: Option<i64>,
    pub fund_from_investments: bool,
    pub assumptions_confirmed: bool,
}

/// Check the answers, as `POST /scenarios/setup` does before it writes anything.
pub fn validate(p: &SetupPlan) -> PlanResult<()> {
    let start = p
        .start_date
        .parse::<jiff::civil::Date>()
        .map_err(|_| PlanError::invalid("Invalid start date"))?;
    let birth = p
        .birth_date
        .parse::<jiff::civil::Date>()
        .map_err(|_| PlanError::invalid("Enter a valid birth date"))?;
    if p.name.trim().is_empty()
        || p.request_id.len() < 8
        || p.request_id.len() > 128
        || !(1..=120).contains(&p.duration_years)
        || birth > start
        || p.retirement_age > 120
    {
        return Err(PlanError::invalid(
            "Review the plan name, dates and retirement age",
        ));
    }
    if [
        p.cash,
        p.retirement_401k,
        p.investments,
        p.annual_income,
        p.retirement_401k_contribution_percent,
        p.annual_spending,
        p.retirement_spending,
    ]
    .iter()
    .any(|n| !n.is_finite() || *n < 0.)
        || !p.stock_percent.is_finite()
        || !(0. ..=100.).contains(&p.stock_percent)
        || !(0. ..=100.).contains(&p.retirement_401k_contribution_percent)
    {
        return Err(PlanError::invalid(
            "Amounts must be finite and nonnegative; percentage fields must be 0–100%",
        ));
    }
    if p.retirement_401k_contribution_percent > 0. && p.annual_income == 0. {
        return Err(PlanError::invalid(
            "A 401(k) contribution requires positive annual salary",
        ));
    }
    if p.fund_from_investments
        && p.retirement_401k + p.investments == 0.
        && p.retirement_401k_contribution_percent == 0.
    {
        return Err(PlanError::invalid(
            "Investment funding requires a 401(k) or another investment account",
        ));
    }
    if !p.assumptions_confirmed
        || !["Taxable", "TaxDeferred", "TaxFree"].contains(&p.investment_tax_status.as_str())
    {
        return Err(PlanError::invalid(
            "Confirm assumptions and select an investment account type",
        ));
    }
    Ok(())
}

/// What the person contributes to the 401(k) in a year, capped at the limit.
#[must_use]
pub fn annual_401k_contribution(p: &SetupPlan, deferral_limit: f64) -> f64 {
    (p.annual_income * p.retirement_401k_contribution_percent / 100.).min(deferral_limit)
}

/// Whether the plan has investments at all: the stock and bond profiles are
/// only needed (and only checked) when it does.
#[must_use]
pub fn has_investments(p: &SetupPlan, annual_401k_contribution: f64) -> bool {
    p.retirement_401k + p.investments > 0. || annual_401k_contribution > 0.
}

/// The plan a setup describes, as changes to the empty scenario row: accounts,
/// the allocation assets and opening lots, the parameters the answers set,
/// then the salary and spending events, which follow them. The same
/// builders back the drafting agent's templates.
pub fn lower(p: &SetupPlan, annual_401k_contribution: f64) -> PlanResult<Vec<Change>> {
    let cash_profile = RowRef::Id(p.cash_profile_id);
    let mut changes = vec![bank_account(
        "checking",
        "Checking",
        p.cash,
        cash_profile.clone(),
        Some(0),
    )];
    let checking = RowRef::new("checking");
    let mut investment_accounts = Vec::new();
    let mut retirement_account = None;
    if p.retirement_401k > 0. || annual_401k_contribution > 0. {
        retirement_account = Some(RowRef::new("401k"));
        investment_accounts.push(("401k", "401(k)", "TaxDeferred", 1, p.retirement_401k));
    }
    if p.investments > 0. {
        investment_accounts.push((
            "other",
            "Other investments",
            p.investment_tax_status.as_str(),
            2,
            p.investments,
        ));
    }
    let allocation: Vec<(RowRef, f64)> = if investment_accounts.is_empty() {
        vec![]
    } else {
        vec![
            (RowRef::new("stock"), p.stock_percent / 100.),
            (RowRef::new("bond"), 1. - p.stock_percent / 100.),
        ]
    };
    for (key, name, profile, sort_order) in [
        ("stock", "Stock allocation", p.stock_profile_id, 0),
        ("bond", "Bond allocation", p.bond_profile_id, 0),
    ] {
        if !investment_accounts.is_empty() {
            changes.push(allocation_asset(
                key,
                name,
                RowRef::Id(profile),
                Some(sort_order),
            ));
        }
    }
    for (key, name, tax_status, sort_order, value) in &investment_accounts {
        let positions = if *value > 0. {
            allocation
                .iter()
                .map(|(asset, fraction)| position(asset, value * fraction, value * fraction))
                .collect()
        } else {
            vec![]
        };
        changes.push(investment_account(
            key,
            name,
            tax_status,
            cash_profile.clone(),
            Some(*sort_order),
            positions,
        ));
    }

    // Created only when an event follows them, so Analysis offers exactly
    // the inputs the plan uses.
    let has_events = p.annual_income > 0. || p.annual_spending > 0. || p.retirement_spending > 0.;
    if has_events {
        changes.push(parameter(
            "retirement_age",
            RETIREMENT_AGE,
            ParameterValueSpec::Age {
                years: p.retirement_age,
                months: 0,
            },
        ));
    }
    if p.annual_spending > 0. {
        changes.push(parameter(
            "monthly_spending",
            MONTHLY_SPENDING,
            ParameterValueSpec::Money {
                value: p.annual_spending / 12.,
            },
        ));
    }

    let age = || When::AgeParameter {
        parameter_id: RowRef::new("retirement_age"),
    };
    let allocations: Vec<Allocation> = allocation
        .iter()
        .map(|(asset, fraction)| Allocation {
            asset_id: asset.clone(),
            fraction: *fraction,
        })
        .collect();
    let mut templates = Vec::new();
    if p.annual_income > 0. {
        templates.push((
            "",
            Template::Salary(SalaryParams {
                name: Some("Salary until retirement".into()),
                to_account_id: checking.clone(),
                annual_amount: p.annual_income,
                start: None,
                end: Some(age()),
                employee_401k: retirement_account
                    .filter(|_| annual_401k_contribution > 0.)
                    .map(|account_id| Employee401k {
                        account_id,
                        annual_amount: annual_401k_contribution,
                        allocation: allocations,
                    }),
                sort_order: Some(0),
            }),
        ));
    }
    for (prefix, name, amount, amount_parameter, sort_order, start, end) in [
        (
            "before_",
            "Spending before retirement",
            p.annual_spending,
            Some(MONTHLY_SPENDING),
            1,
            None,
            Some(age()),
        ),
        (
            "after_",
            "Retirement spending",
            p.retirement_spending,
            None,
            2,
            Some(age()),
            None,
        ),
    ] {
        if amount > 0. {
            templates.push((
                prefix,
                Template::RecurringExpense(RecurringExpenseParams {
                    name: name.into(),
                    from_account_id: checking.clone(),
                    amount,
                    amount_parameter: amount_parameter.map(Into::into),
                    parameter_interval: amount_parameter.map(|_| Interval::Monthly),
                    interval: None,
                    inflation_adjusted: None,
                    start,
                    end,
                    fund_from_investments: p.fund_from_investments,
                    sort_order: Some(sort_order),
                }),
            ));
        }
    }
    for (prefix, template) in templates {
        changes.extend(expand_template(prefix, &template)?.changes);
    }
    Ok(changes)
}

/// What the scenario row says about how the plan was made.
#[must_use]
pub fn description(p: &SetupPlan) -> &'static str {
    if p.fund_from_investments {
        "Guided setup: investment withdrawals explicitly authorized; assumptions reviewed."
    } else {
        "Guided setup: spending funded by checking only; assumptions reviewed."
    }
}

/// `POST /scenarios/setup`, in memory: the plan the answers describe, as the
/// graph the server would load after writing it. `id` and `now` are the new
/// plan's id and creation time (see [`new_plan`]); a name, an assumption or a
/// profile that is not in `library` is refused as the route refuses it.
pub fn create_plan(
    p: &SetupPlan,
    library: &Library,
    id: i64,
    now: &str,
) -> PlanResult<ScenarioGraph> {
    validate(p)?;
    let contribution = annual_401k_contribution(p, GUIDED_SETUP_401K_DEFERRAL_LIMIT);
    let invested = has_investments(p, contribution);
    let profile_known = |id: i64| library.return_profiles.iter().any(|r| r.id == id);
    let known = profile_known(p.cash_profile_id)
        && (!invested || (profile_known(p.stock_profile_id) && profile_known(p.bond_profile_id)))
        && p.inflation_profile_id
            .is_none_or(|id| library.inflation_profiles.iter().any(|r| r.id == id))
        && p.tax_config_id
            .is_none_or(|id| library.tax_configs.iter().any(|r| r.id == id));
    if !known {
        return Err(PlanError::invalid("Selected assumptions are unavailable"));
    }
    let graph = new_plan(
        &CreateScenario {
            name: p.name.trim().to_string(),
            description: Some(description(p).to_string()),
            start_date: p.start_date.clone(),
            birth_date: Some(p.birth_date.clone()),
            duration_years: p.duration_years,
            inflation_profile_id: p.inflation_profile_id,
            tax_config_id: p.tax_config_id,
        },
        library,
        id,
        now,
    )?;
    let changes = lower(p, contribution)?;
    match resolve_steps(&graph, &[changes], &Created::new())? {
        Ok(stepped) => Ok(stepped.graph),
        Err(problems) => Err(PlanError::internal(format!(
            "guided setup did not apply: {problems:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile;

    fn answers(library: &Library) -> SetupPlan {
        let profile = library.return_profiles[0].id;
        SetupPlan {
            request_id: "setup-request-1".into(),
            name: "  Guided ".into(),
            start_date: "2026-01-01".into(),
            birth_date: "1985-06-15".into(),
            duration_years: 40,
            retirement_age: 65,
            cash: 20_000.0,
            retirement_401k: 150_000.0,
            investments: 40_000.0,
            stock_percent: 70.0,
            cash_profile_id: profile,
            stock_profile_id: profile,
            bond_profile_id: profile,
            investment_tax_status: "Taxable".into(),
            annual_income: 120_000.0,
            retirement_401k_contribution_percent: 10.0,
            annual_spending: 60_000.0,
            retirement_spending: 55_000.0,
            inflation_profile_id: None,
            tax_config_id: None,
            fund_from_investments: true,
            assumptions_confirmed: true,
        }
    }

    #[test]
    fn answers_become_a_plan_that_compiles() {
        let library = crate::library::seed();
        let plan = create_plan(&answers(&library), &library, 3, "2026-10-03 12:00:00").unwrap();
        assert_eq!(plan.scenario.name, "Guided");
        assert_eq!(plan.scenario.id, 3);
        // Checking, the 401(k) and the other investments.
        assert_eq!(plan.accounts.len(), 3);
        assert_eq!(plan.assets.len(), 2);
        // Retirement age and spending, and the salary, before and after events.
        assert_eq!(plan.parameters.len(), 2);
        assert!(plan.events.len() >= 3);
        compile(&plan).expect("a guided plan runs");
    }

    #[test]
    fn answers_are_refused_as_the_route_refuses_them() {
        let library = crate::library::seed();
        let mut bad = answers(&library);
        bad.assumptions_confirmed = false;
        assert!(matches!(
            create_plan(&bad, &library, 3, "now"),
            Err(PlanError::Invalid(_))
        ));
        let mut unknown = answers(&library);
        unknown.stock_profile_id = 9_999;
        assert!(matches!(
            create_plan(&unknown, &library, 3, "now"),
            Err(PlanError::Invalid(message)) if message.contains("unavailable")
        ));
        // The stock and bond profiles matter only when something is invested.
        let mut cash_only = answers(&library);
        (cash_only.retirement_401k, cash_only.investments) = (0.0, 0.0);
        (
            cash_only.retirement_401k_contribution_percent,
            cash_only.fund_from_investments,
        ) = (0.0, false);
        cash_only.stock_profile_id = 9_999;
        assert!(create_plan(&cash_only, &library, 3, "now").is_ok());
    }
}
