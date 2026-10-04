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
use finplan_plan::preflight::PreflightReport;
use finplan_plan::setup;
use finplan_plan::suggest::Created;
use serde::Serialize;
use ts_rs::TS;

use crate::suggest::ai::tools::facts::employee_deferral_limit;

/// The tax year guided setup's 401(k) cap is read from in the reference table.
const GUIDED_SETUP_TAX_YEAR: i32 = 2026;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/setup", post(create))
        .route("/scenarios/{id}/preflight", get(preflight))
}
pub use finplan_plan::setup::SetupPlan;
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SetupCreated {
    pub scenario_id: i64,
}
async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(p): Json<SetupPlan>,
) -> ApiResult<Json<SetupCreated>> {
    setup::validate(&p)?;
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
    let annual_401k_contribution = setup::annual_401k_contribution(
        &p,
        employee_deferral_limit(GUIDED_SETUP_TAX_YEAR).unwrap_or(f64::INFINITY),
    );
    let has_investments = setup::has_investments(&p, annual_401k_contribution);
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
        .bind(setup::description(&p)).fetch_one(&mut *tx).await?;
    let changes = setup::lower(&p, annual_401k_contribution)?;
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

async fn preflight(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<PreflightReport>> {
    let graph = crate::db::graph::load(&state.db, id, &user.id).await?;
    Ok(Json(finplan_plan::read::preflight(&graph)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser caps a guided-setup contribution at the plan crate's copy of
    /// the limit; it must be the reference table's.
    #[test]
    fn guided_setup_caps_at_the_reference_tables_deferral_limit() {
        assert_eq!(
            employee_deferral_limit(GUIDED_SETUP_TAX_YEAR),
            Some(setup::GUIDED_SETUP_401K_DEFERRAL_LIMIT)
        );
    }
}
