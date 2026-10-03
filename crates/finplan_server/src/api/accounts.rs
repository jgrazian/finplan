//! Account CRUD across the four flavors, plus position (lot) management.
//!
//! The wire format is one tagged object; the storage is `accounts` plus the
//! matching detail table, written in a single transaction.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use super::ReorderRequest;
use crate::auth::activity::{ActivityFields, Submitted};
use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use finplan_plan::specs::{CatchUpSpec, repayment_of};

use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;

use finplan_plan::specs::accounts::{
    Account, ContributionPeriod, CreateAccount, CreatePosition, FlavorSpec, PlanType, Position,
    TaxStatus, UpdateAccount, UpdatePosition,
};
use finplan_plan::specs::accounts::{
    DetailRow, NAME_TAKEN, check_loan_payer, check_lot_figures, check_lot_home,
};
use finplan_plan::specs::scenarios::validate_date;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/{scenario_id}/accounts", get(list).post(create))
        .route("/scenarios/{scenario_id}/accounts/reorder", post(reorder))
        .route(
            "/scenarios/{scenario_id}/accounts/{id}",
            get(fetch).patch(update).delete(destroy),
        )
        .route(
            "/scenarios/{scenario_id}/accounts/{id}/positions",
            get(list_positions).post(add_position),
        )
        .route(
            "/scenarios/{scenario_id}/accounts/{id}/positions/reorder",
            post(reorder_positions),
        )
        .route(
            "/scenarios/{scenario_id}/accounts/{id}/positions/{position_id}",
            axum::routing::patch(update_position).delete(delete_position),
        )
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
            #[allow(clippy::type_complexity)]
            let (tax_status, cash_value, cash_return_profile_id, limit, period, plan, catch_up): (
                String,
                f64,
                i64,
                Option<f64>,
                Option<String>,
                Option<String>,
                sqlx::types::Json<Vec<CatchUpSpec>>,
            ) = sqlx::query_as(
                "SELECT tax_status, cash_value, cash_return_profile_id,
                        contribution_limit, contribution_period, plan_type, catch_up
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
                plan_type: plan.as_deref().and_then(PlanType::parse),
                catch_up: catch_up.0,
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
            let (principal, interest_rate, from, term): (f64, f64, Option<i64>, Option<i64>) =
                sqlx::query_as(
                    "SELECT principal, interest_rate, repay_from_account_id, term_months
                       FROM account_liability WHERE account_id = ?1",
                )
                .bind(id)
                .fetch_one(&state.db)
                .await?;
            FlavorSpec::Liability {
                principal,
                interest_rate,
                repayment: repayment_of(from, term),
            }
        }
    };

    let positions: Vec<Position> = sqlx::query_as(
        "SELECT id, asset_id, purchase_date, units, cost_basis
           FROM positions WHERE account_id = ?1 ORDER BY sort_order, purchase_date, id",
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
///
/// What each flavor stores is [`FlavorSpec::detail_row`]; this is only the SQL
/// and the checks that need to see the table.
async fn insert_detail(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: i64,
    flavor: &FlavorSpec,
) -> ApiResult<()> {
    match flavor.detail_row(account_id) {
        DetailRow::Bank(row) => {
            sqlx::query(
                "INSERT INTO account_bank (account_id, cash_value, return_profile_id)
                 VALUES (?1,?2,?3)",
            )
            .bind(row.account_id)
            .bind(row.cash_value)
            .bind(row.return_profile_id)
            .execute(&mut **tx)
            .await?;
        }
        DetailRow::Investment(row) => {
            sqlx::query(
                "INSERT INTO account_investment
                    (account_id, tax_status, cash_value, cash_return_profile_id,
                     contribution_limit, contribution_period, plan_type, catch_up)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )
            .bind(row.account_id)
            .bind(&row.tax_status)
            .bind(row.cash_value)
            .bind(row.cash_return_profile_id)
            .bind(row.contribution_limit)
            .bind(&row.contribution_period)
            .bind(&row.plan_type)
            .bind(sqlx::types::Json(&row.catch_up))
            .execute(&mut **tx)
            .await?;
        }
        DetailRow::Property(row) => {
            sqlx::query(
                "INSERT INTO account_property (account_id, asset_id, value) VALUES (?1,?2,?3)",
            )
            .bind(row.account_id)
            .bind(row.asset_id)
            .bind(row.value)
            .execute(&mut **tx)
            .await?;
        }
        DetailRow::Liability(row) => {
            if let Some(payer_id) = row.repay_from_account_id {
                // The payer has to be a cash-holding account in the same plan.
                let payer: Option<String> = sqlx::query_scalar(
                    "SELECT p.flavor FROM accounts p JOIN accounts a ON a.scenario_id = p.scenario_id
                      WHERE a.id = ?1 AND p.id = ?2",
                )
                .bind(account_id)
                .bind(payer_id)
                .fetch_optional(&mut **tx)
                .await?;
                check_loan_payer(payer.as_deref())?;
            }
            sqlx::query(
                "INSERT INTO account_liability
                    (account_id, principal, interest_rate, repay_from_account_id, term_months)
                 VALUES (?1,?2,?3,?4,?5)",
            )
            .bind(row.account_id)
            .bind(row.principal)
            .bind(row.interest_rate)
            .bind(row.repay_from_account_id)
            .bind(row.term_months)
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

/// Put the scenario's accounts in the order the body names.
async fn reorder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let current: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM accounts WHERE scenario_id = ?1 ORDER BY sort_order, id",
    )
    .bind(scenario_id)
    .fetch_all(&state.db)
    .await?;

    let affected = super::apply_order(&state.db, "accounts", &current, &body.ids).await?;

    if affected > 0 {
        state.telemetry.mutation(
            Resource::Account,
            Operation::Reordered,
            &EventFields {
                user_id: Some(&user.id),
                scenario_id: Some(scenario_id),
                fields: &["ids"],
                count: Some(affected),
                ..Default::default()
            },
        );
    }
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
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
    Json(Submitted { body, fields }): Json<Submitted<CreateAccount>>,
) -> ApiResult<(StatusCode, Json<Account>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let mut tx = state.db.begin().await?;
    let id = create_in(&mut tx, scenario_id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Account,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(load_account(&state, scenario_id, id).await?),
    ))
}

/// Add an account and its detail row; the SQL half of `POST .../accounts`.
pub(crate) async fn create_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    body: &CreateAccount,
) -> ApiResult<i64> {
    body.flavor.validate()?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO accounts (scenario_id, name, description, flavor, sort_order)
         VALUES (?1,?2,?3,?4,
                 COALESCE(?5, (SELECT COALESCE(MAX(sort_order), -1) + 1
                                 FROM accounts WHERE scenario_id = ?1)))
         RETURNING id",
    )
    .bind(scenario_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(body.flavor.name())
    .bind(body.sort_order)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))?;

    insert_detail(tx, id, &body.flavor).await?;
    Ok(id)
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
    Json(Submitted { body, fields }): Json<Submitted<UpdateAccount>>,
) -> ApiResult<Json<Account>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let rename_graph = if body.name.is_some() {
        Some(crate::db::graph::load(&state.db, scenario_id, &user.id).await?)
    } else {
        None
    };

    let mut tx = state.db.begin().await?;
    update_in(&mut tx, rename_graph.as_ref(), scenario_id, id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Account,
        Operation::Updated,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(Json(load_account(&state, scenario_id, id).await?))
}

/// Apply `body` to account `id`; the SQL half of `PATCH .../accounts/{id}`.
/// `rename_graph` is the plan before the change, needed only when `body`
/// renames the account.
pub(crate) async fn update_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    rename_graph: Option<&finplan_plan::graph::ScenarioGraph>,
    scenario_id: i64,
    id: i64,
    body: &UpdateAccount,
) -> ApiResult<()> {
    let existing_flavor: Option<String> =
        sqlx::query_scalar("SELECT flavor FROM accounts WHERE id = ?1 AND scenario_id = ?2")
            .bind(id)
            .bind(scenario_id)
            .fetch_optional(&mut **tx)
            .await?;
    let existing_flavor = existing_flavor.ok_or(ApiError::NotFound("account"))?;

    body.check(&existing_flavor)?;

    let affected = sqlx::query(
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
    .execute(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("account"));
    }
    if let Some(flavor) = &body.flavor {
        let table = match flavor {
            FlavorSpec::Bank { .. } => "account_bank",
            FlavorSpec::Investment { .. } => "account_investment",
            FlavorSpec::Property { .. } => "account_property",
            FlavorSpec::Liability { .. } => "account_liability",
        };
        sqlx::query(&format!("DELETE FROM {table} WHERE account_id = ?1"))
            .bind(id)
            .execute(&mut **tx)
            .await?;
        insert_detail(tx, id, flavor).await?;
    }
    if let (Some(graph), Some(name)) = (rename_graph, &body.name) {
        super::expression_refs::rerender(
            tx,
            graph,
            finplan_plan::expression_refs::Entity::Account(id),
            name.trim(),
        )
        .await?;
    }
    Ok(())
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    let mut conn = state.db.acquire().await?;
    destroy_in(&mut conn, &graph, scenario_id, id).await?;
    drop(conn);

    state.telemetry.mutation(
        Resource::Account,
        Operation::Deleted,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete an account, as `DELETE /scenarios/{id}/accounts/{account}` does.
/// `live` is the plan as stored, for the expression check. The suggestion path
/// writes through this too, inside its transaction.
pub(crate) async fn destroy_in(
    conn: &mut sqlx::SqliteConnection,
    live: &finplan_plan::graph::ScenarioGraph,
    scenario_id: i64,
    id: i64,
) -> ApiResult<()> {
    finplan_plan::expression_refs::refuse_if_used(
        live,
        finplan_plan::expression_refs::Entity::Account(id),
    )?;

    let affected = sqlx::query("DELETE FROM accounts WHERE id = ?1 AND scenario_id = ?2")
        .bind(id)
        .bind(scenario_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("account"));
    }
    Ok(())
}

// ── positions ───────────────────────────────────────────────────────────────

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
          ORDER BY p.sort_order, p.purchase_date, p.id",
    )
    .bind(id)
    .bind(scenario_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// Put one account's lots in the order the body names.
async fn reorder_positions(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    // The join is what confines the renumbering to the caller's scenario: a
    // position id on its own says nothing about who owns the account under it.
    let current: Vec<i64> = sqlx::query_scalar(
        "SELECT p.id FROM positions p JOIN accounts a ON a.id = p.account_id
          WHERE p.account_id = ?1 AND a.scenario_id = ?2
          ORDER BY p.sort_order, p.purchase_date, p.id",
    )
    .bind(id)
    .bind(scenario_id)
    .fetch_all(&state.db)
    .await?;

    let affected = super::apply_order(&state.db, "positions", &current, &body.ids).await?;

    if affected > 0 {
        state.telemetry.mutation(
            Resource::Position,
            Operation::Reordered,
            &EventFields {
                user_id: Some(&user.id),
                scenario_id: Some(scenario_id),
                resource_id: Some(id),
                fields: &["ids"],
                count: Some(affected),
                ..Default::default()
            },
        );
    }
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_position(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
    Json(Submitted { body, fields }): Json<Submitted<CreatePosition>>,
) -> ApiResult<(StatusCode, Json<Position>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let mut tx = state.db.begin().await?;
    let position = add_position_in(&mut tx, scenario_id, id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Position,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(position.id),
            fields: &fields,
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;

    Ok((StatusCode::CREATED, Json(position)))
}

/// Add a lot to account `id`; the SQL half of `POST .../accounts/{id}/positions`.
pub(crate) async fn add_position_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    id: i64,
    body: &CreatePosition,
) -> ApiResult<Position> {
    // Lots only exist inside investment accounts; the engine has nowhere to put
    // them on a bank, property or liability account.
    let flavor: Option<String> =
        sqlx::query_scalar("SELECT flavor FROM accounts WHERE id = ?1 AND scenario_id = ?2")
            .bind(id)
            .bind(scenario_id)
            .fetch_optional(&mut **tx)
            .await?;

    check_lot_home(flavor.as_deref())?;

    let purchase_date = match body.purchase_date.as_deref() {
        Some(d) => validate_date(d, "purchase_date")?,
        None => {
            sqlx::query_scalar::<_, String>("SELECT start_date FROM scenarios WHERE id = ?1")
                .bind(scenario_id)
                .fetch_one(&mut **tx)
                .await?
        }
    };

    check_lot_figures(Some(body.units), Some(body.cost_basis))?;

    let position_id: i64 = sqlx::query_scalar(
        "INSERT INTO positions (account_id, asset_id, purchase_date, units, cost_basis, sort_order)
         VALUES (?1,?2,?3,?4,?5,
                 (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM positions WHERE account_id = ?1))
         RETURNING id",
    )
    .bind(id)
    .bind(body.asset_id)
    .bind(&purchase_date)
    .bind(body.units)
    .bind(body.cost_basis)
    .fetch_one(&mut **tx)
    .await?;

    Ok(Position {
        id: position_id,
        asset_id: body.asset_id,
        purchase_date,
        units: body.units,
        cost_basis: body.cost_basis,
    })
}

async fn update_position(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id, position_id)): Path<(i64, i64, i64)>,
    Json(Submitted { body, fields }): Json<Submitted<UpdatePosition>>,
) -> ApiResult<Json<Position>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let mut tx = state.db.begin().await?;
    update_position_in(&mut tx, scenario_id, id, position_id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Position,
        Operation::Updated,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(position_id),
            fields: &fields,
            ..Default::default()
        },
    );
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

/// Resize or re-date lot `position_id` of account `id`; the SQL half of
/// `PATCH .../accounts/{id}/positions/{position}`.
pub(crate) async fn update_position_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    id: i64,
    position_id: i64,
    body: &UpdatePosition,
) -> ApiResult<()> {
    // Same rule as the insert: a lot is non-negative in both figures, and a
    // date has to be a date before it reaches the engine's timeline.
    check_lot_figures(body.units, body.cost_basis)?;
    let purchase_date = body
        .purchase_date
        .as_deref()
        .map(|d| validate_date(d, "purchase_date"))
        .transpose()?;

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
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("position"));
    }
    Ok(())
}

async fn delete_position(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id, position_id)): Path<(i64, i64, i64)>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let mut tx = state.db.begin().await?;
    delete_position_in(&mut tx, scenario_id, id, position_id).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Position,
        Operation::Deleted,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(position_id),
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove lot `position_id` from account `id`; the SQL half of
/// `DELETE .../accounts/{id}/positions/{position}`.
pub(crate) async fn delete_position_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    id: i64,
    position_id: i64,
) -> ApiResult<()> {
    let affected = sqlx::query(
        "DELETE FROM positions
          WHERE id = ?1 AND account_id = ?2
            AND account_id IN (SELECT id FROM accounts WHERE scenario_id = ?3)",
    )
    .bind(position_id)
    .bind(id)
    .bind(scenario_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("position"));
    }
    Ok(())
}

impl ActivityFields for CreateAccount {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "sort_order",
        "flavor",
        "cash_value",
        "return_profile_id",
        "tax_status",
        "cash_return_profile_id",
        "contribution_limit",
        "contribution_period",
        "asset_id",
        "value",
        "principal",
        "interest_rate",
        "repayment",
    ];
}

impl ActivityFields for UpdateAccount {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "sort_order",
        "flavor",
        "cash_value",
        "return_profile_id",
        "tax_status",
        "cash_return_profile_id",
        "contribution_limit",
        "contribution_period",
        "asset_id",
        "value",
        "principal",
        "interest_rate",
        "repayment",
    ];
}

impl ActivityFields for CreatePosition {
    const FIELDS: &'static [&'static str] = &["asset_id", "purchase_date", "units", "cost_basis"];
}

impl ActivityFields for UpdatePosition {
    const FIELDS: &'static [&'static str] = &["asset_id", "purchase_date", "units", "cost_basis"];
}
