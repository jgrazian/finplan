//! Guided setup writes the same account, position and event records as advanced editing.
use super::specs::*;
use crate::{
    auth::session::CurrentUser,
    compile::rows::ScenarioGraph,
    error::{ApiError, ApiResult},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const EMPLOYEE_401K_DEFERRAL_LIMIT_2026: f64 = 24_500.;

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
        return Ok(Json(SetupCreated { scenario_id: id }));
    }
    crate::billing::check_plan_slot(&mut tx, &user.id, state.config.hosted, 1).await?;
    let invested = p.retirement_401k + p.investments;
    let annual_401k_contribution = (p.annual_income * p.retirement_401k_contribution_percent
        / 100.)
        .min(EMPLOYEE_401K_DEFERRAL_LIMIT_2026);
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
    let checking: i64 = sqlx::query_scalar(
        "INSERT INTO accounts(scenario_id,name,flavor) VALUES(?,'Checking','Bank') RETURNING id",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO account_bank(account_id,cash_value,return_profile_id) VALUES(?,?,?)")
        .bind(checking)
        .bind(p.cash)
        .bind(p.cash_profile_id)
        .execute(&mut *tx)
        .await?;
    let mut investment_accounts = Vec::new();
    let mut retirement_account = None;
    if p.retirement_401k > 0. || annual_401k_contribution > 0. {
        let account: i64 = sqlx::query_scalar("INSERT INTO accounts(scenario_id,name,flavor,sort_order) VALUES(?,'401(k)','Investment',1) RETURNING id").bind(id).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO account_investment(account_id,tax_status,cash_value,cash_return_profile_id) VALUES(?,'TaxDeferred',0,?)").bind(account).bind(p.cash_profile_id).execute(&mut *tx).await?;
        retirement_account = Some(account);
        investment_accounts.push((account, p.retirement_401k));
    }
    if p.investments > 0. {
        let account: i64 = sqlx::query_scalar("INSERT INTO accounts(scenario_id,name,flavor,sort_order) VALUES(?,'Other investments','Investment',2) RETURNING id").bind(id).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO account_investment(account_id,tax_status,cash_value,cash_return_profile_id) VALUES(?,?,0,?)").bind(account).bind(&p.investment_tax_status).bind(p.cash_profile_id).execute(&mut *tx).await?;
        investment_accounts.push((account, p.investments));
    }
    let mut allocation_assets = Vec::new();
    for (name, profile, fraction) in [
        (
            "Stock allocation",
            p.stock_profile_id,
            p.stock_percent / 100.,
        ),
        (
            "Bond allocation",
            p.bond_profile_id,
            1. - p.stock_percent / 100.,
        ),
    ] {
        if investment_accounts.is_empty() {
            break;
        }
        let asset: i64 = sqlx::query_scalar("INSERT INTO assets(scenario_id,name,initial_price,return_profile_id) VALUES(?,?,1,?) RETURNING id").bind(id).bind(name).bind(profile).fetch_one(&mut *tx).await?;
        allocation_assets.push((asset, fraction));
        for (account, value) in &investment_accounts {
            if *value > 0. {
                sqlx::query("INSERT INTO positions(account_id,asset_id,purchase_date,units,cost_basis) VALUES(?,?,?,?,?)").bind(account).bind(asset).bind(&p.start_date).bind(value*fraction).bind(value*fraction).execute(&mut *tx).await?;
            }
        }
    }
    let adjusted = |value| AmountSpec::InflationAdjusted {
        inner: Box::new(AmountSpec::Fixed { value }),
    };
    let age = || {
        Some(Box::new(TriggerSpec::Age {
            years: p.retirement_age,
            months: None,
        }))
    };
    for (order, name, value, income, retired) in [
        (0, "Salary until retirement", p.annual_income, true, false),
        (
            1,
            "Spending before retirement",
            p.annual_spending,
            false,
            false,
        ),
        (2, "Retirement spending", p.retirement_spending, false, true),
    ] {
        if value == 0. {
            continue;
        }
        let event: i64 = sqlx::query_scalar("INSERT INTO events(scenario_id,name,fires_once,enabled,sort_order) VALUES(?,?,0,1,?) RETURNING id").bind(id).bind(name).bind(order).fetch_one(&mut *tx).await?;
        TriggerSpec::Repeating {
            interval: Interval::Yearly,
            start_condition: if retired { age() } else { None },
            end_condition: if retired { None } else { age() },
            max_occurrences: None,
        }
        .insert(&mut tx, id, TriggerParent::Event(event), 0)
        .await?;
        let mut effects = vec![];
        if income {
            let taxable_salary = value - annual_401k_contribution;
            if taxable_salary > 0. {
                effects.push(EffectSpec::Income {
                    to_account_id: checking,
                    amount: adjusted(taxable_salary),
                    amount_mode: AmountMode::Gross,
                    income_type: IncomeType::Taxable,
                });
            }
            if let Some(retirement_account) = retirement_account
                && annual_401k_contribution > 0.
            {
                effects.push(EffectSpec::Income {
                    to_account_id: retirement_account,
                    amount: adjusted(annual_401k_contribution),
                    amount_mode: AmountMode::Gross,
                    income_type: IncomeType::TaxFree,
                });
                for (asset_id, fraction) in &allocation_assets {
                    if *fraction > 0. {
                        effects.push(EffectSpec::AssetPurchase {
                            from_account_id: retirement_account,
                            to_account_id: retirement_account,
                            asset_id: *asset_id,
                            amount: adjusted(annual_401k_contribution * fraction),
                        });
                    }
                }
            }
        } else {
            if p.fund_from_investments {
                effects.push(EffectSpec::Sweep {
                    to_account_id: checking,
                    amount: AmountSpec::Max {
                        left: Box::new(AmountSpec::Fixed { value: 0. }),
                        right: Box::new(AmountSpec::Sub {
                            left: Box::new(adjusted(value)),
                            right: Box::new(AmountSpec::AccountCashBalance {
                                account_id: checking,
                            }),
                        }),
                    },
                    sources: Some(WithdrawalSourcesSpec::Strategy {
                        strategy: WithdrawalStrategy::TaxEfficientEarly,
                        exclude_accounts: vec![],
                    }),
                    amount_mode: AmountMode::Net,
                    lot_method: LotMethod::Fifo,
                    income_type: IncomeType::TaxFree,
                });
            }
            effects.push(EffectSpec::Expense {
                from_account_id: checking,
                amount: adjusted(value),
            });
        }
        for (position, effect) in effects.iter().enumerate() {
            effect
                .insert(
                    &mut tx,
                    id,
                    EffectParent::Event {
                        event_id: event,
                        position: position as i64,
                    },
                    0,
                )
                .await?;
        }
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
    tx.commit().await?;
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
    let g = ScenarioGraph::load(&state.db, id, &user.id).await?;
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
    if let Err(e) = crate::compile::compile(g) {
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
    let enabled_effect = |e: &&crate::compile::rows::EffectRow| {
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
