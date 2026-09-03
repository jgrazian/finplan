//! Account CRUD across the four flavors, plus position (lot) management.
//!
//! The wire format is one tagged object; the storage is `accounts` plus the
//! matching detail table, written in a single transaction.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::state::AppState;
use ts_rs::TS;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/{scenario_id}/accounts", get(list).post(create))
        .route(
            "/scenarios/{scenario_id}/accounts/{id}",
            get(fetch).patch(update).delete(destroy),
        )
        .route(
            "/scenarios/{scenario_id}/accounts/{id}/positions",
            get(list_positions).post(add_position),
        )
        .route(
            "/scenarios/{scenario_id}/accounts/{id}/positions/{position_id}",
            axum::routing::patch(update_position).delete(delete_position),
        )
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum TaxStatus {
    Taxable,
    TaxDeferred,
    TaxFree,
}

impl TaxStatus {
    fn as_str(self) -> &'static str {
        match self {
            TaxStatus::Taxable => "Taxable",
            TaxStatus::TaxDeferred => "TaxDeferred",
            TaxStatus::TaxFree => "TaxFree",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ContributionPeriod {
    Monthly,
    Yearly,
}

impl ContributionPeriod {
    fn as_str(self) -> &'static str {
        match self {
            ContributionPeriod::Monthly => "Monthly",
            ContributionPeriod::Yearly => "Yearly",
        }
    }
}

/// The flavor-specific half of an account.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "flavor")]
#[ts(export)]
pub enum FlavorSpec {
    Bank {
        #[serde(default)]
        cash_value: f64,
        return_profile_id: i64,
    },
    Investment {
        tax_status: TaxStatus,
        #[serde(default)]
        cash_value: f64,
        cash_return_profile_id: i64,
        #[serde(default)]
        contribution_limit: Option<f64>,
        #[serde(default)]
        contribution_period: Option<ContributionPeriod>,
    },
    Property {
        asset_id: i64,
        #[serde(default)]
        value: f64,
    },
    Liability {
        #[serde(default)]
        principal: f64,
        #[serde(default)]
        interest_rate: f64,
    },
}

impl FlavorSpec {
    fn name(&self) -> &'static str {
        match self {
            FlavorSpec::Bank { .. } => "Bank",
            FlavorSpec::Investment { .. } => "Investment",
            FlavorSpec::Property { .. } => "Property",
            FlavorSpec::Liability { .. } => "Liability",
        }
    }

    fn validate(&self) -> ApiResult<()> {
        if let FlavorSpec::Investment {
            contribution_limit,
            contribution_period,
            ..
        } = self
            && contribution_limit.is_some() != contribution_period.is_some()
        {
            return Err(ApiError::bad_request(
                "contribution_limit and contribution_period must be set together",
            ));
        }
        if let FlavorSpec::Liability { principal, .. } = self
            && *principal < 0.0
        {
            return Err(ApiError::bad_request(
                "liability principal is stored as a positive amount owed",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i64,
    #[serde(flatten)]
    pub flavor: FlavorSpec,
    pub positions: Vec<Position>,
}

#[derive(Debug, Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct Position {
    pub id: i64,
    pub asset_id: i64,
    pub purchase_date: String,
    pub units: f64,
    pub cost_basis: f64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateAccount {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(flatten)]
    pub flavor: FlavorSpec,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateAccount {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
    /// Replaces the detail row wholesale. The flavor itself cannot change:
    /// switching a 401k into a mortgage would silently invalidate every event
    /// and position pointing at it.
    ///
    /// Skipped in the TypeScript bindings: `FlavorSpec | null` has no flattened
    /// form, so the web client composes the union itself as
    /// `UpdateAccount & (FlavorSpec | {})` in `web/lib/api/types.ts`.
    #[serde(default, flatten)]
    #[ts(skip)]
    pub flavor: Option<FlavorSpec>,
}

async fn load_account(state: &AppState, scenario_id: i64, id: i64) -> ApiResult<Account> {
    let base: Option<(i64, String, Option<String>, String, i64)> = sqlx::query_as(
        "SELECT id, name, description, flavor, sort_order
           FROM accounts WHERE id = ?1 AND scenario_id = ?2",
    )
    .bind(id)
    .bind(scenario_id)
    .fetch_optional(&state.db)
    .await?;

    let (id, name, description, flavor_name, sort_order) =
        base.ok_or(ApiError::NotFound("account"))?;

    let flavor = match flavor_name.as_str() {
        "Bank" => {
            let (cash_value, return_profile_id): (f64, i64) = sqlx::query_as(
                "SELECT cash_value, return_profile_id FROM account_bank WHERE account_id = ?1",
            )
            .bind(id)
            .fetch_one(&state.db)
            .await?;
            FlavorSpec::Bank {
                cash_value,
                return_profile_id,
            }
        }
        "Investment" => {
            let (tax_status, cash_value, cash_return_profile_id, limit, period): (
                String,
                f64,
                i64,
                Option<f64>,
                Option<String>,
            ) = sqlx::query_as(
                "SELECT tax_status, cash_value, cash_return_profile_id,
                        contribution_limit, contribution_period
                   FROM account_investment WHERE account_id = ?1",
            )
            .bind(id)
            .fetch_one(&state.db)
            .await?;

            FlavorSpec::Investment {
                tax_status: match tax_status.as_str() {
                    "TaxDeferred" => TaxStatus::TaxDeferred,
                    "TaxFree" => TaxStatus::TaxFree,
                    _ => TaxStatus::Taxable,
                },
                cash_value,
                cash_return_profile_id,
                contribution_limit: limit,
                contribution_period: match period.as_deref() {
                    Some("Monthly") => Some(ContributionPeriod::Monthly),
                    Some("Yearly") => Some(ContributionPeriod::Yearly),
                    _ => None,
                },
            }
        }
        "Property" => {
            let (asset_id, value): (i64, f64) = sqlx::query_as(
                "SELECT asset_id, value FROM account_property WHERE account_id = ?1",
            )
            .bind(id)
            .fetch_one(&state.db)
            .await?;
            FlavorSpec::Property { asset_id, value }
        }
        _ => {
            let (principal, interest_rate): (f64, f64) = sqlx::query_as(
                "SELECT principal, interest_rate FROM account_liability WHERE account_id = ?1",
            )
            .bind(id)
            .fetch_one(&state.db)
            .await?;
            FlavorSpec::Liability {
                principal,
                interest_rate,
            }
        }
    };

    let positions: Vec<Position> = sqlx::query_as(
        "SELECT id, asset_id, purchase_date, units, cost_basis
           FROM positions WHERE account_id = ?1 ORDER BY purchase_date, id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Account {
        id,
        name,
        description,
        sort_order,
        flavor,
        positions,
    })
}

/// Write the detail row for `flavor`. Assumes any previous detail row for this
/// account has already been removed.
async fn insert_detail(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: i64,
    flavor: &FlavorSpec,
) -> ApiResult<()> {
    match flavor {
        FlavorSpec::Bank {
            cash_value,
            return_profile_id,
        } => {
            sqlx::query(
                "INSERT INTO account_bank (account_id, cash_value, return_profile_id)
                 VALUES (?1,?2,?3)",
            )
            .bind(account_id)
            .bind(cash_value)
            .bind(return_profile_id)
            .execute(&mut **tx)
            .await?;
        }
        FlavorSpec::Investment {
            tax_status,
            cash_value,
            cash_return_profile_id,
            contribution_limit,
            contribution_period,
        } => {
            sqlx::query(
                "INSERT INTO account_investment
                    (account_id, tax_status, cash_value, cash_return_profile_id,
                     contribution_limit, contribution_period)
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )
            .bind(account_id)
            .bind(tax_status.as_str())
            .bind(cash_value)
            .bind(cash_return_profile_id)
            .bind(contribution_limit)
            .bind(contribution_period.map(|p| p.as_str()))
            .execute(&mut **tx)
            .await?;
        }
        FlavorSpec::Property { asset_id, value } => {
            sqlx::query(
                "INSERT INTO account_property (account_id, asset_id, value) VALUES (?1,?2,?3)",
            )
            .bind(account_id)
            .bind(asset_id)
            .bind(value)
            .execute(&mut **tx)
            .await?;
        }
        FlavorSpec::Liability {
            principal,
            interest_rate,
        } => {
            sqlx::query(
                "INSERT INTO account_liability (account_id, principal, interest_rate)
                 VALUES (?1,?2,?3)",
            )
            .bind(account_id)
            .bind(principal)
            .bind(interest_rate)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Vec<Account>>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM accounts WHERE scenario_id = ?1 ORDER BY sort_order, id",
    )
    .bind(scenario_id)
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(load_account(&state, scenario_id, id).await?);
    }
    Ok(Json(out))
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<Json<Account>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    Ok(Json(load_account(&state, scenario_id, id).await?))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<CreateAccount>,
) -> ApiResult<(StatusCode, Json<Account>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    body.flavor.validate()?;

    let mut tx = state.db.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO accounts (scenario_id, name, description, flavor, sort_order)
         VALUES (?1,?2,?3,?4,?5) RETURNING id",
    )
    .bind(scenario_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(body.flavor.name())
    .bind(body.sort_order)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "an account with that name already exists"))?;

    insert_detail(&mut tx, id, &body.flavor).await?;
    tx.commit().await?;

    super::touch_scenario(&state.db, scenario_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(load_account(&state, scenario_id, id).await?),
    ))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
    Json(body): Json<UpdateAccount>,
) -> ApiResult<Json<Account>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let existing_flavor: Option<String> =
        sqlx::query_scalar("SELECT flavor FROM accounts WHERE id = ?1 AND scenario_id = ?2")
            .bind(id)
            .bind(scenario_id)
            .fetch_optional(&state.db)
            .await?;
    let existing_flavor = existing_flavor.ok_or(ApiError::NotFound("account"))?;

    // An absent name leaves the stored one alone; a blank one is a mistake, and
    // COALESCE would write it as the account's name.
    if body.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(ApiError::bad_request("an account needs a name"));
    }

    if let Some(flavor) = &body.flavor {
        flavor.validate()?;
        if flavor.name() != existing_flavor {
            return Err(ApiError::Conflict(format!(
                "cannot change account flavor from {existing_flavor} to {}; \
                 create a new account instead",
                flavor.name()
            )));
        }
    }

    let mut tx = state.db.begin().await?;

    sqlx::query(
        "UPDATE accounts SET
            name        = COALESCE(?3, name),
            description = COALESCE(?4, description),
            sort_order  = COALESCE(?5, sort_order),
            updated_at  = datetime('now')
          WHERE id = ?1 AND scenario_id = ?2",
    )
    .bind(id)
    .bind(scenario_id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(&body.description)
    .bind(body.sort_order)
    .execute(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "an account with that name already exists"))?;

    if let Some(flavor) = &body.flavor {
        let table = match flavor {
            FlavorSpec::Bank { .. } => "account_bank",
            FlavorSpec::Investment { .. } => "account_investment",
            FlavorSpec::Property { .. } => "account_property",
            FlavorSpec::Liability { .. } => "account_liability",
        };
        sqlx::query(&format!("DELETE FROM {table} WHERE account_id = ?1"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        insert_detail(&mut tx, id, flavor).await?;
    }

    tx.commit().await?;
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(Json(load_account(&state, scenario_id, id).await?))
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let affected = sqlx::query("DELETE FROM accounts WHERE id = ?1 AND scenario_id = ?2")
        .bind(id)
        .bind(scenario_id)
        .execute(&state.db)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("account"));
    }
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── positions ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreatePosition {
    pub asset_id: i64,
    /// Defaults to the scenario start date, i.e. an opening holding.
    #[serde(default)]
    pub purchase_date: Option<String>,
    pub units: f64,
    pub cost_basis: f64,
}

/// Every field of a lot is resizable in place. Absent means unchanged, so a
/// client that only wants to resize a holding sends `units` alone.
#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdatePosition {
    #[serde(default)]
    pub asset_id: Option<i64>,
    #[serde(default)]
    pub purchase_date: Option<String>,
    #[serde(default)]
    pub units: Option<f64>,
    #[serde(default)]
    pub cost_basis: Option<f64>,
}

async fn list_positions(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<Json<Vec<Position>>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let rows: Vec<Position> = sqlx::query_as(
        "SELECT p.id, p.asset_id, p.purchase_date, p.units, p.cost_basis
           FROM positions p JOIN accounts a ON a.id = p.account_id
          WHERE p.account_id = ?1 AND a.scenario_id = ?2
          ORDER BY p.purchase_date, p.id",
    )
    .bind(id)
    .bind(scenario_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn add_position(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
    Json(body): Json<CreatePosition>,
) -> ApiResult<(StatusCode, Json<Position>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    // Lots only exist inside investment accounts; the engine has nowhere to put
    // them on a bank, property or liability account.
    let flavor: Option<String> =
        sqlx::query_scalar("SELECT flavor FROM accounts WHERE id = ?1 AND scenario_id = ?2")
            .bind(id)
            .bind(scenario_id)
            .fetch_optional(&state.db)
            .await?;

    match flavor.as_deref() {
        Some("Investment") => {}
        Some(other) => {
            return Err(ApiError::Conflict(format!(
                "positions can only be held in Investment accounts, not {other}"
            )));
        }
        None => return Err(ApiError::NotFound("account")),
    }

    let purchase_date = match body.purchase_date.as_deref() {
        Some(d) => d
            .parse::<jiff::civil::Date>()
            .map_err(|e| ApiError::bad_request(format!("invalid purchase_date '{d}': {e}")))?
            .to_string(),
        None => {
            sqlx::query_scalar::<_, String>("SELECT start_date FROM scenarios WHERE id = ?1")
                .bind(scenario_id)
                .fetch_one(&state.db)
                .await?
        }
    };

    if body.units < 0.0 || body.cost_basis < 0.0 {
        return Err(ApiError::bad_request(
            "units and cost_basis must be non-negative",
        ));
    }

    let position_id: i64 = sqlx::query_scalar(
        "INSERT INTO positions (account_id, asset_id, purchase_date, units, cost_basis)
         VALUES (?1,?2,?3,?4,?5) RETURNING id",
    )
    .bind(id)
    .bind(body.asset_id)
    .bind(&purchase_date)
    .bind(body.units)
    .bind(body.cost_basis)
    .fetch_one(&state.db)
    .await?;

    super::touch_scenario(&state.db, scenario_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(Position {
            id: position_id,
            asset_id: body.asset_id,
            purchase_date,
            units: body.units,
            cost_basis: body.cost_basis,
        }),
    ))
}

async fn update_position(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id, position_id)): Path<(i64, i64, i64)>,
    Json(body): Json<UpdatePosition>,
) -> ApiResult<Json<Position>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    // Same rule as the insert: a lot is non-negative in both figures, and a
    // date has to be a date before it reaches the engine's timeline.
    if body.units.is_some_and(|u| u < 0.0) || body.cost_basis.is_some_and(|b| b < 0.0) {
        return Err(ApiError::bad_request(
            "units and cost_basis must be non-negative",
        ));
    }
    let purchase_date = match body.purchase_date.as_deref() {
        Some(d) => Some(
            d.parse::<jiff::civil::Date>()
                .map_err(|e| ApiError::bad_request(format!("invalid purchase_date '{d}': {e}")))?
                .to_string(),
        ),
        None => None,
    };

    // The join is what confines the write to the caller's scenario: the
    // position id alone says nothing about who owns the account under it.
    let affected = sqlx::query(
        "UPDATE positions SET
            asset_id      = COALESCE(?4, asset_id),
            purchase_date = COALESCE(?5, purchase_date),
            units         = COALESCE(?6, units),
            cost_basis    = COALESCE(?7, cost_basis)
          WHERE id = ?1 AND account_id = ?2
            AND account_id IN (SELECT id FROM accounts WHERE scenario_id = ?3)",
    )
    .bind(position_id)
    .bind(id)
    .bind(scenario_id)
    .bind(body.asset_id)
    .bind(purchase_date)
    .bind(body.units)
    .bind(body.cost_basis)
    .execute(&state.db)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("position"));
    }
    super::touch_scenario(&state.db, scenario_id).await?;

    let row: Position = sqlx::query_as(
        "SELECT id, asset_id, purchase_date, units, cost_basis
           FROM positions WHERE id = ?1",
    )
    .bind(position_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(row))
}

async fn delete_position(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id, position_id)): Path<(i64, i64, i64)>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let affected = sqlx::query(
        "DELETE FROM positions
          WHERE id = ?1 AND account_id = ?2
            AND account_id IN (SELECT id FROM accounts WHERE scenario_id = ?3)",
    )
    .bind(position_id)
    .bind(id)
    .bind(scenario_id)
    .execute(&state.db)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("position"));
    }
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
