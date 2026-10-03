//! Guided setup writes the same account, position and event records as advanced editing.
use crate::observability::{EventFields, Operation, Resource};
use crate::suggest::apply_steps_sql;
use crate::{
    auth::session::CurrentUser,
    error::{ApiError, ApiResult},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use finplan_plan::graph::ScenarioGraph;
use finplan_plan::specs::Interval;
use finplan_plan::specs::parameters::ParameterValueSpec;
use finplan_plan::suggest::{Change, Created};
use finplan_plan::templates::{
    Allocation, Employee401k, RecurringExpenseParams, RowRef, SalaryParams, Template, When,
    allocation_asset, bank_account, expand_template, investment_account, parameter, position,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::suggest::ai::tools::facts::employee_deferral_limit;

/// The tax year guided setup's 401(k) cap is read from in the reference table.
const GUIDED_SETUP_TAX_YEAR: i32 = 2026;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/setup", post(create))
        .route("/scenarios/{id}/preflight", get(preflight))
}
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
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SetupCreated {
    pub scenario_id: i64,
}
fn validate(p: &SetupPlan) -> ApiResult<()> {
    let start = p
        .start_date
        .parse::<jiff::civil::Date>()
        .map_err(|_| ApiError::bad_request("Invalid start date"))?;
    let birth = p
        .birth_date
        .parse::<jiff::civil::Date>()
        .map_err(|_| ApiError::bad_request("Enter a valid birth date"))?;
    if p.name.trim().is_empty()
        || p.request_id.len() < 8
        || p.request_id.len() > 128
        || !(1..=120).contains(&p.duration_years)
        || birth > start
        || p.retirement_age > 120
    {
        return Err(ApiError::bad_request(
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
        return Err(ApiError::bad_request(
            "Amounts must be finite and nonnegative; percentage fields must be 0–100%",
        ));
    }
    if p.retirement_401k_contribution_percent > 0. && p.annual_income == 0. {
        return Err(ApiError::bad_request(
            "A 401(k) contribution requires positive annual salary",
        ));
    }
    if p.fund_from_investments
        && p.retirement_401k + p.investments == 0.
        && p.retirement_401k_contribution_percent == 0.
    {
        return Err(ApiError::bad_request(
            "Investment funding requires a 401(k) or another investment account",
        ));
    }
    if !p.assumptions_confirmed
        || !["Taxable", "TaxDeferred", "TaxFree"].contains(&p.investment_tax_status.as_str())
    {
        return Err(ApiError::bad_request(
            "Confirm assumptions and select an investment account type",
        ));
    }
    Ok(())
}
/// The parameters guided setup's events follow, so Analysis can vary them and
/// the Plan tab edits each in one place.
pub(crate) const RETIREMENT_AGE: &str = "Retirement age";
pub(crate) const MONTHLY_SPENDING: &str = "Monthly spending";

/// The plan a setup describes, as changes to the empty scenario row: accounts,
/// the allocation assets and opening lots, the parameters the answers set,
/// then the salary and spending events, which follow them. The same
/// builders back the drafting agent's templates.
pub(crate) fn lower(p: &SetupPlan, annual_401k_contribution: f64) -> ApiResult<Vec<Change>> {
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
async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(p): Json<SetupPlan>,
) -> ApiResult<Json<SetupCreated>> {
    validate(&p)?;
    let serialized = serde_json::to_string(&p).map_err(|e| ApiError::internal(e.to_string()))?;
    // A completed retry must remain available even when it filled the final free slot.
    if let Some((old, scenario_id)) = sqlx::query_as::<_, (String, i64)>(
        "SELECT request_json,scenario_id FROM setup_receipts WHERE user_id=? AND request_id=?",
    )
    .bind(&user.id)
    .bind(&p.request_id)
    .fetch_optional(&state.db)
    .await?
    {
        if old != serialized {
            return Err(ApiError::bad_request(
                "This setup was already saved with different inputs; start a new setup",
            ));
        }

        state.telemetry.mutation(
            Resource::Onboarding,
            Operation::Completed,
            &EventFields {
                user_id: Some(&user.id),
                scenario_id: Some(scenario_id),
                resource_id: Some(scenario_id),
                count: Some(0),
                replay: true,
                ..Default::default()
            },
        );
        return Ok(Json(SetupCreated { scenario_id }));
    }
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    // Acquire the writer lock before checking the receipt so concurrent retries serialize.
    sqlx::query("UPDATE users SET id = id WHERE id = ?")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    if let Some((old, id)) = sqlx::query_as::<_, (String, i64)>(
        "SELECT request_json,scenario_id FROM setup_receipts WHERE user_id=? AND request_id=?",
    )
    .bind(&user.id)
    .bind(&p.request_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        if old != serialized {
            return Err(ApiError::bad_request(
                "This setup was already saved with different inputs; start a new setup",
            ));
        }

        state.telemetry.mutation(
            Resource::Onboarding,
            Operation::Completed,
            &EventFields {
                user_id: Some(&user.id),
                scenario_id: Some(id),
                resource_id: Some(id),
                count: Some(0),
                replay: true,
                ..Default::default()
            },
        );
        return Ok(Json(SetupCreated { scenario_id: id }));
    }
    crate::billing::check_plan_slot(&mut tx, &user.id, &state.config, 1).await?;
    let invested = p.retirement_401k + p.investments;
    let annual_401k_contribution = (p.annual_income * p.retirement_401k_contribution_percent
        / 100.)
        .min(employee_deferral_limit(GUIDED_SETUP_TAX_YEAR).unwrap_or(f64::INFINITY));
    let has_investments = invested > 0. || annual_401k_contribution > 0.;
    for (table, id) in [
        ("return_profiles", Some(p.cash_profile_id)),
        (
            "return_profiles",
            has_investments.then_some(p.stock_profile_id),
        ),
        (
            "return_profiles",
            has_investments.then_some(p.bond_profile_id),
        ),
        ("inflation_profiles", p.inflation_profile_id),
        ("tax_configs", p.tax_config_id),
    ] {
        if let Some(id) = id {
            let found: i64 = sqlx::query_scalar(&format!(
                "SELECT count(*) FROM {table} WHERE id=? AND user_id=?"
            ))
            .bind(id)
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
            if found != 1 {
                return Err(ApiError::bad_request(
                    "Selected assumptions are unavailable",
                ));
            }
        }
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO scenarios(user_id,name,start_date,birth_date,duration_years,inflation_profile_id,tax_config_id,description) VALUES(?,?,?,?,?,?,?,?) RETURNING id")
        .bind(&user.id).bind(p.name.trim()).bind(&p.start_date).bind(&p.birth_date).bind(p.duration_years).bind(p.inflation_profile_id).bind(p.tax_config_id)
        .bind(if p.fund_from_investments {"Guided setup: investment withdrawals explicitly authorized; assumptions reviewed."} else {"Guided setup: spending funded by checking only; assumptions reviewed."}).fetch_one(&mut *tx).await?;
    let changes = lower(&p, annual_401k_contribution)?;
    if let Err(problems) =
        apply_steps_sql(&mut tx, id, &user.id, &[changes], &Created::new()).await?
    {
        return Err(ApiError::internal(format!(
            "guided setup did not apply: {problems:?}"
        )));
    }
    sqlx::query(
        "INSERT INTO setup_receipts(user_id,request_id,request_json,scenario_id) VALUES(?,?,?,?)",
    )
    .bind(&user.id)
    .bind(&p.request_id)
    .bind(serialized)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let count: i64 = sqlx::query_scalar("SELECT 1 + (SELECT count(*) FROM accounts WHERE scenario_id = ?1) + (SELECT count(*) FROM assets WHERE scenario_id = ?1) + (SELECT count(*) FROM events WHERE scenario_id = ?1) + (SELECT count(*) FROM named_parameters WHERE scenario_id = ?1) + (SELECT count(*) FROM positions WHERE account_id IN (SELECT id FROM accounts WHERE scenario_id = ?1))")
        .bind(id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    state.telemetry.mutation(
        Resource::Onboarding,
        Operation::Completed,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(id),
            resource_id: Some(id),
            count: Some(count as u64),
            ..Default::default()
        },
    );

    Ok(Json(SetupCreated { scenario_id: id }))
}
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct PreflightIssue {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub section: String,
    pub record_id: Option<i64>,
}
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct PreflightReport {
    pub issues: Vec<PreflightIssue>,
    pub can_run: bool,
}
async fn preflight(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<PreflightReport>> {
    let g = crate::db::graph::load(&state.db, id, &user.id).await?;
    Ok(Json(review(&g)))
}
pub fn review(g: &ScenarioGraph) -> PreflightReport {
    let mut issues = vec![];
    let mut add = |code: &str, severity: &str, message: String, section: &str, record_id| {
        issues.push(PreflightIssue {
            code: code.into(),
            severity: severity.into(),
            message,
            section: section.into(),
            record_id,
        })
    };
    if let Err(e) = finplan_plan::compile::compile(g) {
        add("invalid_plan", "error", e.to_string(), "plan", None);
    }
    for a in &g.assets {
        if a.return_profile_id.is_none() {
            add(
                "unmapped_asset",
                "warning",
                format!(
                    "{} has no return assumption. Its price stays fixed in nominal dollars; confirm that this is intentional.",
                    a.name
                ),
                "portfolio",
                Some(a.id),
            );
        }
    }
    for e in &g.events {
        if e.enabled != 0 && g.event_effects.get(&e.id).is_none_or(|v| v.is_empty()) {
            add(
                "empty_event",
                "warning",
                format!("{} is enabled but has no effects.", e.name),
                "plan",
                Some(e.id),
            );
        }
    }
    let active_events: std::collections::HashSet<i64> = g
        .events
        .iter()
        .filter(|e| e.enabled != 0)
        .map(|e| e.id)
        .collect();
    let enabled_effect = |e: &&finplan_plan::graph::EffectRow| {
        e.event_id.is_some_and(|id| active_events.contains(&id))
    };
    if !g
        .effects
        .values()
        .filter(enabled_effect)
        .any(|e| e.kind == "Expense")
    {
        add(
            "missing_spending",
            "warning",
            "No spending is modeled. This run cannot assess your ability to fund living costs."
                .into(),
            "plan",
            None,
        );
    }
    if !g
        .effects
        .values()
        .filter(enabled_effect)
        .any(|e| e.kind == "Sweep" || e.kind == "CashTransfer")
    {
        add("funding_intent","warning","No withdrawal or transfer rule is modeled. Spending uses only its named account; review whether this is intentional.".into(),"plan",None);
    }
    if g.scenario.birth_date.is_none() {
        add(
            "missing_birth",
            "warning",
            "No birth date is set. Review age-based retirement and withdrawal assumptions.".into(),
            "plan",
            None,
        );
    }
    if let (Ok(start), Some(Ok(birth))) = (
        g.scenario.start_date.parse::<jiff::civil::Date>(),
        g.scenario
            .birth_date
            .as_ref()
            .map(|s| s.parse::<jiff::civil::Date>()),
    ) {
        let end_age = i64::from(start.year()) + g.scenario.duration_years
            - i64::from(birth.year())
            - i64::from((start.month(), start.day()) < (birth.month(), birth.day()));
        add(
            "horizon",
            "warning",
            format!(
                "The modeled horizon ends around age {end_age}. Confirm that it covers your household's intended planning lifetime."
            ),
            "plan",
            None,
        );
    }
    if g.scenario.inflation_profile_id.is_none() {
        add(
            "no_inflation",
            "warning",
            "No inflation is modeled. Inflation-adjusted spending will not grow.".into(),
            "plan",
            None,
        );
    }
    add(
        "assumptions",
        "warning",
        format!(
            "Review tax assumptions: {}. Returns and inflation are annual nominal assumptions. Tax modeling is simplified; profile labels retain their original year and filing status. Review omitted household income, benefits and costs before relying on results.",
            g.tax_config
                .as_ref()
                .map(|t| t.name.as_str())
                .unwrap_or("no tax profile")
        ),
        "plan",
        None,
    );
    let can_run = !issues.iter().any(|i| i.severity == "error");
    PreflightReport { issues, can_run }
}
