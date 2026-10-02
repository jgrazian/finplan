//! Plan edits applied to an in-memory [`ScenarioGraph`] instead of the database.
//!
//! Each operation mirrors one write route — the same validation, the same rows
//! (lowered through the same [`RowBatch`] the route inserts), the same cascades
//! and orphan collection the schema performs — so compiling the edited graph
//! gives what compiling the plan would give after the route had run. That is
//! what lets a change be previewed from a run's snapshot without writing.
//!
//! Every operation is atomic: it works on a copy and only replaces `graph` once
//! everything has succeeded, the way the route's transaction would roll back.
//!
//! What does not carry over from the database:
//!
//!   * references are checked against this plan only, where a foreign key
//!     accepts a row from any scenario; and
//!   * `updated_at` and other bookkeeping columns are not modelled.

use std::collections::HashSet;

use crate::api::accounts::{
    CreateAccount, CreatePosition, FlavorSpec, UpdateAccount, UpdatePosition,
};
use crate::api::assets::{CreateAsset, UpdateAsset};
use crate::api::events::{EventBody, lower_tree};
use crate::api::expression_refs::{self, Entity};
use crate::api::expressions::validate_tree;
use crate::api::parameters::{self, ParameterBody};
use crate::api::profiles::{CreateProfile, DistributionSpec};
use crate::api::row_batch::RowBatch;
use crate::api::scenarios::UpdateScenario;
use crate::api::taxes::{self, CreateTaxConfig};
use crate::compile::rows::{
    AccountRow, AssetRow, BankRow, DistributionRow, EventRow, InflationEntry, InvestmentRow,
    LiabilityRow, ParameterRow, PositionRow, PropertyRow, ReturnProfileRow, ScenarioGraph,
    TaxBracketRow, TaxConfigEntry, TaxConfigRow,
};
use crate::error::{ApiError, ApiResult};

/// Run `edit` on a copy of `graph`, keeping the result only if it succeeds.
fn atomically<T>(
    graph: &mut ScenarioGraph,
    edit: impl FnOnce(&mut ScenarioGraph) -> ApiResult<T>,
) -> ApiResult<T> {
    let mut staged = graph.clone();
    let out = edit(&mut staged)?;
    *graph = staged;
    Ok(out)
}

// ── events ───────────────────────────────────────────────────────────────────

/// Add an event, as `POST /scenarios/{id}/events` would. Returns its new id.
pub(crate) fn create_event(graph: &mut ScenarioGraph, body: &EventBody) -> ApiResult<i64> {
    validate_tree(graph, &body.effects)?;
    atomically(graph, |g| {
        let name = body.name.trim();
        if g.events.iter().any(|e| e.name == name) {
            return Err(ApiError::Conflict(
                "an event with that name already exists".into(),
            ));
        }
        let id = g.events.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        let sort_order = body
            .sort_order
            .unwrap_or_else(|| g.events.iter().map(|e| e.sort_order).max().unwrap_or(-1) + 1);
        g.events.push(EventRow {
            id,
            name: name.to_string(),
            description: body.description.clone(),
            fires_once: i64::from(body.fires_once),
            enabled: i64::from(body.enabled),
            sort_order,
        });
        sort_events(g);
        write_tree(g, id, body)?;
        Ok(id)
    })
}

/// Rewrite an event wholesale, as `PUT /scenarios/{id}/events/{event}` would.
pub(crate) fn replace_event(
    graph: &mut ScenarioGraph,
    event_id: i64,
    body: &EventBody,
) -> ApiResult<()> {
    validate_tree(graph, &body.effects)?;
    atomically(graph, |g| {
        if !g.events.iter().any(|e| e.id == event_id) {
            return Err(ApiError::NotFound("event"));
        }
        let name = body.name.trim();
        if g.events.iter().any(|e| e.id != event_id && e.name == name) {
            return Err(ApiError::Conflict(
                "an event with that name already exists".into(),
            ));
        }
        let row = g
            .events
            .iter_mut()
            .find(|e| e.id == event_id)
            .ok_or(ApiError::NotFound("event"))?;
        row.name = name.to_string();
        row.description = body.description.clone();
        row.fires_once = i64::from(body.fires_once);
        row.enabled = i64::from(body.enabled);
        if let Some(sort_order) = body.sort_order {
            row.sort_order = sort_order;
        }
        sort_events(g);

        // The route deletes the root trigger and the top-level effects; the
        // schema's cascades take their subtrees with them.
        let mut doomed = Doomed::default();
        doomed.triggers.extend(
            g.triggers
                .values()
                .filter(|t| t.event_id == Some(event_id))
                .map(|t| t.id),
        );
        doomed.effects.extend(
            g.effects
                .values()
                .filter(|e| e.event_id == Some(event_id))
                .map(|e| e.id),
        );
        cascade(g, doomed);

        write_tree(g, event_id, body)?;
        collect_orphans(g);
        Ok(())
    })
}

/// Delete an event, as `DELETE /scenarios/{id}/events/{event}` would.
///
/// Refused while another event's root trigger or top-level effect points at
/// it. Like the route, that check does not look inside nested conditions or
/// `Random` branches: a reference there is removed by the cascade.
pub(crate) fn delete_event(graph: &mut ScenarioGraph, event_id: i64) -> ApiResult<()> {
    atomically(graph, |g| {
        let name_of = |id: i64| g.events.iter().find(|e| e.id == id).map(|e| e.name.clone());
        let mut referrers: Vec<String> = g
            .triggers
            .values()
            .filter(|t| t.ref_event_id == Some(event_id))
            .filter_map(|t| t.event_id.filter(|owner| *owner != event_id))
            .chain(
                g.effects
                    .values()
                    .filter(|f| f.target_event_id == Some(event_id))
                    .filter_map(|f| f.event_id.filter(|owner| *owner != event_id)),
            )
            .filter_map(name_of)
            .collect();
        referrers.sort();
        referrers.dedup();
        if !referrers.is_empty() {
            return Err(ApiError::Conflict(format!(
                "event is referenced by: {}",
                referrers.join(", ")
            )));
        }

        if !g.events.iter().any(|e| e.id == event_id) {
            return Err(ApiError::NotFound("event"));
        }
        let mut doomed = Doomed::default();
        doomed.events.insert(event_id);
        cascade(g, doomed);
        collect_orphans(g);
        Ok(())
    })
}

fn write_tree(graph: &mut ScenarioGraph, event_id: i64, body: &EventBody) -> ApiResult<()> {
    let mut batch = RowBatch::for_graph(graph);
    lower_tree(&mut batch, event_id, body)?;
    batch.merge_into(graph)?;
    Ok(())
}

fn sort_events(graph: &mut ScenarioGraph) {
    graph.events.sort_by_key(|e| (e.sort_order, e.id));
}

// ── assets ───────────────────────────────────────────────────────────────────

/// Apply an asset update, as `PATCH /scenarios/{id}/assets/{asset}` would.
/// Add an asset, as `POST /scenarios/{id}/assets` would. Returns its new id.
pub(crate) fn create_asset(graph: &mut ScenarioGraph, body: &CreateAsset) -> ApiResult<i64> {
    // The route checks the profile is the caller's before anything else; the
    // graph carries the caller's whole profile library.
    if let Some(profile_id) = body.return_profile_id
        && !graph.return_profiles.contains_key(&profile_id)
    {
        return Err(ApiError::NotFound("return profile"));
    }
    if body.initial_price <= 0.0 {
        return Err(ApiError::bad_request("initial_price must be positive"));
    }
    atomically(graph, |g| {
        let name = body.name.trim();
        if g.assets.iter().any(|a| a.name == name) {
            return Err(ApiError::Conflict(
                "an asset with that name already exists".into(),
            ));
        }
        // CHECK constraint on the table.
        if body.tracking_error.is_some_and(|t| t < 0.0) {
            return Err(ApiError::bad_request("tracking_error cannot be negative"));
        }
        let id = g.assets.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        let sort_order = body
            .sort_order
            .unwrap_or_else(|| g.assets.iter().map(|a| a.sort_order).max().unwrap_or(-1) + 1);
        g.assets.push(AssetRow {
            id,
            name: name.to_string(),
            description: body.description.clone(),
            initial_price: body.initial_price,
            return_profile_id: body.return_profile_id,
            tracking_error: body.tracking_error,
            sort_order,
        });
        g.assets.sort_by_key(|a| (a.sort_order, a.id));
        Ok(id)
    })
}

pub(crate) fn update_asset(
    graph: &mut ScenarioGraph,
    asset_id: i64,
    body: &UpdateAsset,
) -> ApiResult<()> {
    // The route checks the profile is the caller's before anything else; the
    // graph carries the caller's whole profile library.
    if let Some(Some(profile_id)) = body.return_profile_id
        && !graph.return_profiles.contains_key(&profile_id)
    {
        return Err(ApiError::NotFound("return profile"));
    }
    atomically(graph, |g| {
        if !g.assets.iter().any(|a| a.id == asset_id) {
            return Err(ApiError::NotFound("asset"));
        }
        let name = body.name.as_deref().map(str::trim);
        if let Some(name) = name
            && g.assets.iter().any(|a| a.id != asset_id && a.name == name)
        {
            return Err(ApiError::Conflict(
                "an asset with that name already exists".into(),
            ));
        }
        // CHECK constraints on the table.
        if body.initial_price.is_some_and(|p| p <= 0.0) {
            return Err(ApiError::bad_request("initial_price must be positive"));
        }
        if body.tracking_error.flatten().is_some_and(|t| t < 0.0) {
            return Err(ApiError::bad_request("tracking_error cannot be negative"));
        }
        let renamed = match name {
            Some(name) => expression_refs::rerendered(g, Entity::Asset(asset_id), name)?,
            None => Vec::new(),
        };

        let row = g
            .assets
            .iter_mut()
            .find(|a| a.id == asset_id)
            .ok_or(ApiError::NotFound("asset"))?;
        if let Some(name) = name {
            row.name = name.to_string();
        }
        if let Some(description) = &body.description {
            row.description = Some(description.clone());
        }
        if let Some(price) = body.initial_price {
            row.initial_price = price;
        }
        if let Some(profile) = body.return_profile_id {
            row.return_profile_id = profile;
        }
        if let Some(tracking_error) = body.tracking_error {
            row.tracking_error = tracking_error;
        }
        if let Some(sort_order) = body.sort_order {
            row.sort_order = sort_order;
        }
        g.assets.sort_by_key(|a| (a.sort_order, a.id));
        set_sources(g, renamed);
        Ok(())
    })
}

// ── accounts ─────────────────────────────────────────────────────────────────

/// Add an account and its detail row, as `POST /scenarios/{id}/accounts`
/// would. Returns its new id.
pub(crate) fn create_account(graph: &mut ScenarioGraph, body: &CreateAccount) -> ApiResult<i64> {
    body.flavor.validate()?;
    atomically(graph, |g| {
        let name = body.name.trim();
        if g.accounts.iter().any(|a| a.name == name) {
            return Err(ApiError::Conflict(
                "an account with that name already exists".into(),
            ));
        }
        let id = g.accounts.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        let sort_order = body
            .sort_order
            .unwrap_or_else(|| g.accounts.iter().map(|a| a.sort_order).max().unwrap_or(-1) + 1);
        g.accounts.push(AccountRow {
            id,
            name: name.to_string(),
            description: body.description.clone(),
            flavor: body.flavor.name().to_string(),
            sort_order,
        });
        g.accounts.sort_by_key(|a| (a.sort_order, a.id));
        replace_detail(g, id, &body.flavor)?;
        Ok(id)
    })
}

/// Apply an account update, as `PATCH /scenarios/{id}/accounts/{account}` would.
pub(crate) fn update_account(
    graph: &mut ScenarioGraph,
    account_id: i64,
    body: &UpdateAccount,
) -> ApiResult<()> {
    atomically(graph, |g| {
        let existing_flavor = g
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .map(|a| a.flavor.clone())
            .ok_or(ApiError::NotFound("account"))?;

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

        let name = body.name.as_deref().map(str::trim);
        if let Some(name) = name
            && g.accounts
                .iter()
                .any(|a| a.id != account_id && a.name == name)
        {
            return Err(ApiError::Conflict(
                "an account with that name already exists".into(),
            ));
        }
        let renamed = match name {
            Some(name) => expression_refs::rerendered(g, Entity::Account(account_id), name)?,
            None => Vec::new(),
        };

        let row = g
            .accounts
            .iter_mut()
            .find(|a| a.id == account_id)
            .ok_or(ApiError::NotFound("account"))?;
        if let Some(name) = name {
            row.name = name.to_string();
        }
        if let Some(description) = &body.description {
            row.description = Some(description.clone());
        }
        if let Some(sort_order) = body.sort_order {
            row.sort_order = sort_order;
        }
        g.accounts.sort_by_key(|a| (a.sort_order, a.id));

        if let Some(flavor) = &body.flavor {
            replace_detail(g, account_id, flavor)?;
        }
        set_sources(g, renamed);
        Ok(())
    })
}

/// Swap in the detail row for `flavor`, with the checks its insert makes.
fn replace_detail(
    graph: &mut ScenarioGraph,
    account_id: i64,
    flavor: &FlavorSpec,
) -> ApiResult<()> {
    let profile = |id: i64| {
        if graph.return_profiles.contains_key(&id) {
            Ok(())
        } else {
            Err(ApiError::bad_request(format!(
                "return profile {id} does not exist"
            )))
        }
    };
    match flavor {
        FlavorSpec::Bank {
            cash_value,
            return_profile_id,
        } => {
            profile(*return_profile_id)?;
            graph.bank.insert(
                account_id,
                BankRow {
                    account_id,
                    cash_value: *cash_value,
                    return_profile_id: *return_profile_id,
                },
            );
        }
        FlavorSpec::Investment {
            tax_status,
            cash_value,
            cash_return_profile_id,
            contribution_limit,
            contribution_period,
        } => {
            profile(*cash_return_profile_id)?;
            graph.investment.insert(
                account_id,
                InvestmentRow {
                    account_id,
                    tax_status: tax_status.as_str().to_string(),
                    cash_value: *cash_value,
                    cash_return_profile_id: *cash_return_profile_id,
                    contribution_limit: *contribution_limit,
                    contribution_period: contribution_period.map(|p| p.as_str().to_string()),
                },
            );
        }
        FlavorSpec::Property { asset_id, value } => {
            if !graph.assets.iter().any(|a| a.id == *asset_id) {
                return Err(ApiError::bad_request(format!(
                    "asset {asset_id} does not exist in this plan"
                )));
            }
            graph.property.insert(
                account_id,
                PropertyRow {
                    account_id,
                    asset_id: *asset_id,
                    value: *value,
                },
            );
        }
        FlavorSpec::Liability {
            principal,
            interest_rate,
            repayment,
        } => {
            if let Some(repayment) = repayment {
                let payer = graph
                    .accounts
                    .iter()
                    .find(|a| a.id == repayment.from_account_id)
                    .map(|a| a.flavor.as_str());
                if !matches!(payer, Some("Bank" | "Investment")) {
                    return Err(ApiError::bad_request(
                        "a loan is repaid from a bank or investment account in the same plan",
                    ));
                }
            }
            graph.liability.insert(
                account_id,
                LiabilityRow {
                    account_id,
                    principal: *principal,
                    interest_rate: *interest_rate,
                    repay_from_account_id: repayment.map(|r| r.from_account_id),
                    term_months: repayment.map(|r| i64::from(r.term_months)),
                },
            );
        }
    }
    Ok(())
}

/// Delete an asset, as `DELETE /scenarios/{id}/assets/{asset}` would.
///
/// Stricter than the route, which lets the schema's cascades quietly delete the
/// lots, events and amounts that name the asset: a change batch that deletes it
/// must first remove what still points at it, so nothing disappears unseen.
pub(crate) fn delete_asset(graph: &mut ScenarioGraph, asset_id: i64) -> ApiResult<()> {
    atomically(graph, |g| {
        if !g.assets.iter().any(|a| a.id == asset_id) {
            return Err(ApiError::NotFound("asset"));
        }
        if expression_refs::used_by(g, Entity::Asset(asset_id))? {
            return Err(ApiError::Conflict(
                "asset is referenced by an amount expression".into(),
            ));
        }
        let referrers = referrers(g, Held::Asset(asset_id));
        if !referrers.is_empty() {
            return Err(ApiError::Conflict(format!(
                "asset is still used by: {}",
                referrers.join(", ")
            )));
        }
        g.assets.retain(|a| a.id != asset_id);
        Ok(())
    })
}

/// Delete an account, as `DELETE /scenarios/{id}/accounts/{account}` would,
/// with the same extra strictness as [`delete_asset`]. A loan repaid from the
/// account loses its payer, as the schema's `SET NULL` does.
pub(crate) fn delete_account(graph: &mut ScenarioGraph, account_id: i64) -> ApiResult<()> {
    atomically(graph, |g| {
        if !g.accounts.iter().any(|a| a.id == account_id) {
            return Err(ApiError::NotFound("account"));
        }
        if expression_refs::used_by(g, Entity::Account(account_id))? {
            return Err(ApiError::Conflict(
                "account is referenced by an amount expression".into(),
            ));
        }
        let referrers = referrers(g, Held::Account(account_id));
        if !referrers.is_empty() {
            return Err(ApiError::Conflict(format!(
                "account is still used by: {}",
                referrers.join(", ")
            )));
        }
        g.accounts.retain(|a| a.id != account_id);
        g.bank.remove(&account_id);
        g.investment.remove(&account_id);
        g.property.remove(&account_id);
        g.liability.remove(&account_id);
        g.positions.remove(&account_id);
        for loan in g.liability.values_mut() {
            if loan.repay_from_account_id == Some(account_id) {
                loan.repay_from_account_id = None;
            }
        }
        Ok(())
    })
}

/// An account or asset a delete is about.
#[derive(Clone, Copy)]
enum Held {
    Account(i64),
    Asset(i64),
}

/// What still points at the account or asset, as display names: the events
/// whose triggers, effects, amounts or withdrawal sources name it, and the
/// accounts that hold it. Empty when it can be deleted without taking anything
/// else with it.
fn referrers(graph: &ScenarioGraph, held: Held) -> Vec<String> {
    let is = |account: Option<i64>, asset: Option<i64>| match held {
        Held::Account(id) => account == Some(id),
        Held::Asset(id) => asset == Some(id),
    };
    let account_name = |id: i64| {
        graph
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| format!("account {}", a.name))
    };
    let mut events: HashSet<i64> = HashSet::new();
    let mut out: Vec<String> = Vec::new();

    for t in graph.triggers.values() {
        if is(t.account_id, t.asset_id)
            && let Some(event) = trigger_event(graph, t.id)
        {
            events.insert(event);
        }
    }
    for f in graph.effects.values() {
        let named = match held {
            Held::Account(id) => {
                [f.from_account_id, f.to_account_id, f.loan_account_id].contains(&Some(id))
            }
            Held::Asset(id) => f.asset_id == Some(id),
        };
        if named && let Some(event) = effect_event(graph, f.id) {
            events.insert(event);
        }
    }
    for a in graph.amounts.values() {
        if is(a.account_id, a.asset_id) {
            amount_events(graph, a.id, &mut events, 0);
        }
    }
    for w in graph.withdrawal_sources.values() {
        if is(w.account_id, w.asset_id)
            && let Some(event) = effect_event(graph, w.effect_id)
        {
            events.insert(event);
        }
    }
    for (effect, items) in &graph.withdrawal_items {
        if items.iter().any(|i| is(Some(i.account_id), i.asset_id))
            && let Some(event) = effect_event(graph, *effect)
        {
            events.insert(event);
        }
    }
    let mut named: Vec<String> = graph
        .events
        .iter()
        .filter(|e| events.contains(&e.id))
        .map(|e| format!("event {}", e.name))
        .collect();
    named.sort();
    out.extend(named);

    if let Held::Asset(id) = held {
        let mut holders: Vec<String> = graph
            .positions
            .iter()
            .filter(|(_, lots)| lots.iter().any(|p| p.asset_id == id))
            .filter_map(|(account, _)| account_name(*account))
            .chain(
                graph
                    .property
                    .values()
                    .filter(|p| p.asset_id == id)
                    .filter_map(|p| account_name(p.account_id)),
            )
            .collect();
        holders.sort();
        holders.dedup();
        out.extend(holders);
    }
    out
}

/// The event an effect belongs to, climbing out of `Random` branches.
fn effect_event(graph: &ScenarioGraph, mut id: i64) -> Option<i64> {
    for _ in 0..64 {
        let row = graph.effects.get(&id)?;
        if let Some(event) = row.event_id {
            return Some(event);
        }
        id = row.parent_id?;
    }
    None
}

/// The event a trigger belongs to, climbing out of `And`/`Or` members and the
/// start and end conditions of a repeating trigger.
fn trigger_event(graph: &ScenarioGraph, mut id: i64) -> Option<i64> {
    for _ in 0..64 {
        let row = graph.triggers.get(&id)?;
        if let Some(event) = row.event_id {
            return Some(event);
        }
        id = match row.parent_id {
            Some(parent) => parent,
            None => {
                graph
                    .triggers
                    .values()
                    .find(|p| p.start_trigger_id == Some(id) || p.end_trigger_id == Some(id))?
                    .id
            }
        };
    }
    None
}

/// The events whose effects use the amount, directly or inside another amount.
fn amount_events(graph: &ScenarioGraph, id: i64, out: &mut HashSet<i64>, depth: usize) {
    if depth > 64 {
        return;
    }
    for f in graph.effects.values() {
        if (f.amount_id == Some(id) || f.down_payment_amount_id == Some(id))
            && let Some(event) = effect_event(graph, f.id)
        {
            out.insert(event);
        }
    }
    for a in graph.amounts.values() {
        if a.left_id == Some(id) || a.right_id == Some(id) {
            amount_events(graph, a.id, out, depth + 1);
        }
    }
}

// ── named parameters ─────────────────────────────────────────────────────────

/// Add a named parameter, as `POST /scenarios/{id}/parameters` would. Returns
/// its new id.
pub(crate) fn create_parameter(graph: &mut ScenarioGraph, body: &ParameterBody) -> ApiResult<i64> {
    let name = parameters::validate(body)?.to_owned();
    atomically(graph, |g| {
        if g.parameters.iter().any(|p| p.name == name) {
            return Err(ApiError::Conflict(
                "a parameter with that name already exists".into(),
            ));
        }
        let id = g.parameters.iter().map(|p| p.id).max().unwrap_or(0) + 1;
        g.parameters.push(parameter_row(id, name, body));
        Ok(id)
    })
}

/// Rewrite a parameter, as `PATCH /scenarios/{id}/parameters/{parameter}`
/// would: a rename rewrites the expressions that use it, and a change of type
/// is refused while anything does.
pub(crate) fn update_parameter(
    graph: &mut ScenarioGraph,
    parameter_id: i64,
    body: &ParameterBody,
) -> ApiResult<()> {
    let name = parameters::validate(body)?.to_owned();
    atomically(graph, |g| {
        let old = g
            .parameters
            .iter()
            .find(|p| p.id == parameter_id)
            .ok_or(ApiError::NotFound("parameter"))?;
        let (kind, ..) = body.value.fields();
        if old.kind != kind
            && (!parameters::usages(g, parameter_id)?.is_empty()
                || expression_refs::used_by(g, Entity::Parameter(parameter_id))?)
        {
            return Err(ApiError::Conflict(
                "remove references before changing parameter type".into(),
            ));
        }
        if g.parameters
            .iter()
            .any(|p| p.id != parameter_id && p.name == name)
        {
            return Err(ApiError::Conflict(
                "a parameter with that name already exists".into(),
            ));
        }
        let renamed = if name != old.name {
            expression_refs::rerendered(g, Entity::Parameter(parameter_id), &name)?
        } else {
            Vec::new()
        };
        let row = g
            .parameters
            .iter_mut()
            .find(|p| p.id == parameter_id)
            .ok_or(ApiError::NotFound("parameter"))?;
        *row = parameter_row(parameter_id, name, body);
        set_sources(g, renamed);
        Ok(())
    })
}

/// Delete a parameter nothing uses, as `DELETE /scenarios/{id}/parameters/{parameter}`.
pub(crate) fn delete_parameter(graph: &mut ScenarioGraph, parameter_id: i64) -> ApiResult<()> {
    parameters::delete_refusal(graph, parameter_id)?;
    graph.parameters.retain(|p| p.id != parameter_id);
    Ok(())
}

fn parameter_row(id: i64, name: String, body: &ParameterBody) -> ParameterRow {
    let (kind, number, date, years, months) = body.value.fields();
    ParameterRow {
        id,
        name,
        kind: kind.to_string(),
        number_value: number,
        date_value: date.map(str::to_string),
        age_years: years,
        age_months: months,
    }
}

// ── scenario settings ────────────────────────────────────────────────────────

/// Apply a settings update, as `PATCH /scenarios/{id}` would: fields the body
/// omits are left alone. Switching the tax config or inflation profile needs
/// the target in the graph's library ([`ScenarioGraph::tax_configs`]), which a
/// live load has and a run snapshot has only for what the caller loaded.
pub(crate) fn update_scenario(graph: &mut ScenarioGraph, body: &UpdateScenario) -> ApiResult<()> {
    if body
        .name
        .as_deref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(ApiError::bad_request("scenario name cannot be empty"));
    }
    let date = |text: &str, field: &str| {
        text.parse::<jiff::civil::Date>()
            .map(|d| d.to_string())
            .map_err(|e| ApiError::bad_request(format!("invalid {field} '{text}': {e}")))
    };
    let start_date = body
        .start_date
        .as_deref()
        .map(|d| date(d, "start_date"))
        .transpose()?;
    let birth_date = body
        .birth_date
        .as_deref()
        .map(|d| date(d, "birth_date"))
        .transpose()?;
    // CHECK constraint on the table.
    if body.duration_years.is_some_and(|y| !(1..=120).contains(&y)) {
        return Err(ApiError::bad_request(
            "duration_years must be between 1 and 120",
        ));
    }
    atomically(graph, |g| {
        if let Some(id) = body.tax_config_id
            && g.scenario.tax_config_id != Some(id)
        {
            let TaxConfigEntry { config, brackets } = g
                .tax_configs
                .get(&id)
                .cloned()
                .ok_or(ApiError::NotFound("tax config"))?;
            g.scenario.tax_config_id = Some(id);
            g.tax_config = Some(config);
            g.tax_brackets = brackets;
        }
        if let Some(id) = body.inflation_profile_id
            && g.scenario.inflation_profile_id != Some(id)
        {
            let InflationEntry {
                name,
                distribution_id,
            } = g
                .inflation_profiles
                .get(&id)
                .cloned()
                .ok_or(ApiError::NotFound("inflation profile"))?;
            g.scenario.inflation_profile_id = Some(id);
            g.inflation_distribution_id = Some(distribution_id);
            g.inflation_profile_name = Some(name);
        }
        if let Some(name) = body.name.as_deref() {
            g.scenario.name = name.trim().to_string();
        }
        if let Some(description) = &body.description {
            g.scenario.description = Some(description.clone());
        }
        if let Some(start_date) = start_date {
            g.scenario.start_date = start_date;
        }
        if let Some(birth_date) = birth_date {
            g.scenario.birth_date = Some(birth_date);
        }
        if let Some(years) = body.duration_years {
            g.scenario.duration_years = years;
        }
        if let Some(ledger) = body.collect_ledger {
            g.scenario.collect_ledger = i64::from(ledger);
        }
        Ok(())
    })
}

// ── the caller's return profiles and tax configs ─────────────────────────────

/// Ids for rows that exist only in memory start above every real id a run
/// snapshot (which keeps just the library rows a run used) could be missing.
const IN_MEMORY_ID_FLOOR: i64 = 1_000_000_000;

fn next_library_id(existing: impl Iterator<Item = i64>) -> i64 {
    existing.max().unwrap_or(0).max(IN_MEMORY_ID_FLOOR) + 1
}

/// Add a return profile to the caller's library, as `POST /return-profiles`
/// would. Returns its id, which exists only in this graph.
pub(crate) fn create_return_profile(
    graph: &mut ScenarioGraph,
    body: &CreateProfile,
) -> ApiResult<i64> {
    body.distribution.validate(0)?;
    atomically(graph, |g| {
        let name = body.name.trim();
        if name.is_empty() {
            return Err(ApiError::bad_request("a return profile needs a name"));
        }
        if g.return_profiles.values().any(|p| p.name == name) {
            return Err(ApiError::Conflict(
                "a return profile with that name already exists".into(),
            ));
        }
        let distribution_id = add_distribution(g, &body.distribution);
        let id = next_library_id(g.return_profiles.keys().copied());
        g.return_profiles.insert(
            id,
            ReturnProfileRow {
                asset_class: body.asset_class.map(|c| c.as_str().to_string()),
                id,
                name: name.to_string(),
                description: body.description.clone(),
                distribution_id,
            },
        );
        Ok(id)
    })
}

/// The `distributions` rows for `spec`, children first; returns the root's id.
fn add_distribution(graph: &mut ScenarioGraph, spec: &DistributionSpec) -> i64 {
    let (bull_id, bear_id) = match spec {
        DistributionSpec::RegimeSwitching { bull, bear, .. } => (
            Some(add_distribution(graph, bull)),
            Some(add_distribution(graph, bear)),
        ),
        _ => (None, None),
    };
    let mut row = DistributionRow {
        id: next_library_id(graph.distributions.keys().copied()),
        kind: String::new(),
        rate: None,
        mean: None,
        std_dev: None,
        scale: None,
        df: None,
        bull_id,
        bear_id,
        bull_to_bear_prob: None,
        bear_to_bull_prob: None,
        history_preset: None,
        block_size: None,
    };
    match spec {
        DistributionSpec::None => row.kind = "None".into(),
        DistributionSpec::Fixed { rate } => {
            row.kind = "Fixed".into();
            row.rate = Some(*rate);
        }
        DistributionSpec::Normal { mean, std_dev } => {
            row.kind = "Normal".into();
            (row.mean, row.std_dev) = (Some(*mean), Some(*std_dev));
        }
        DistributionSpec::LogNormal { mean, std_dev } => {
            row.kind = "LogNormal".into();
            (row.mean, row.std_dev) = (Some(*mean), Some(*std_dev));
        }
        DistributionSpec::StudentT { mean, scale, df } => {
            row.kind = "StudentT".into();
            (row.mean, row.scale, row.df) = (Some(*mean), Some(*scale), Some(*df));
        }
        DistributionSpec::RegimeSwitching {
            bull_to_bear_prob,
            bear_to_bull_prob,
            ..
        } => {
            row.kind = "RegimeSwitching".into();
            row.bull_to_bear_prob = Some(*bull_to_bear_prob);
            row.bear_to_bull_prob = Some(*bear_to_bull_prob);
        }
        DistributionSpec::Bootstrap { preset, block_size } => {
            row.kind = "Bootstrap".into();
            row.history_preset = Some(preset.clone());
            row.block_size = *block_size;
        }
    }
    let id = row.id;
    graph.distributions.insert(id, row);
    id
}

/// Add a tax config to the caller's library, as `POST /tax-configs` would.
/// Returns its id, which exists only in this graph.
pub(crate) fn create_tax_config(
    graph: &mut ScenarioGraph,
    body: &CreateTaxConfig,
) -> ApiResult<i64> {
    let brackets = taxes::checked(body)?;
    atomically(graph, |g| {
        let name = body.name.trim();
        if name.is_empty() {
            return Err(ApiError::bad_request("a tax config needs a name"));
        }
        if g.tax_configs.values().any(|c| c.config.name == name) {
            return Err(ApiError::Conflict(
                "a tax config with that name already exists".into(),
            ));
        }
        let id = next_library_id(g.tax_configs.keys().copied());
        g.tax_configs.insert(
            id,
            TaxConfigEntry {
                config: TaxConfigRow {
                    id,
                    name: name.to_string(),
                    state_rate: body.state_rate,
                    capital_gains_rate: body.capital_gains_rate,
                    early_withdrawal_penalty_rate: body.early_withdrawal_penalty_rate,
                    standard_deduction: body.standard_deduction,
                    age_65_extra_deduction: body.age_65_extra_deduction,
                },
                brackets: brackets
                    .iter()
                    .map(|b| TaxBracketRow {
                        threshold: b.threshold,
                        rate: b.rate,
                    })
                    .collect(),
            },
        );
        Ok(id)
    })
}

// ── positions ────────────────────────────────────────────────────────────────

/// Add a lot, as `POST /scenarios/{id}/accounts/{account}/positions` would.
/// Returns its new id.
///
/// The route appends at the account's highest `sort_order` + 1, which always
/// sorts last, so appending to the account's list reproduces its order.
pub(crate) fn create_position(
    graph: &mut ScenarioGraph,
    account_id: i64,
    body: &CreatePosition,
) -> ApiResult<i64> {
    atomically(graph, |g| {
        match g.accounts.iter().find(|a| a.id == account_id) {
            Some(a) if a.flavor == "Investment" => {}
            Some(a) => {
                return Err(ApiError::Conflict(format!(
                    "positions can only be held in Investment accounts, not {}",
                    a.flavor
                )));
            }
            None => return Err(ApiError::NotFound("account")),
        }
        let purchase_date = match body.purchase_date.as_deref() {
            Some(d) => purchase_date(d)?,
            None => g.scenario.start_date.clone(),
        };
        if body.units < 0.0 || body.cost_basis < 0.0 {
            return Err(ApiError::bad_request(
                "units and cost_basis must be non-negative",
            ));
        }
        held_asset(g, body.asset_id)?;
        let id = g
            .positions
            .values()
            .flatten()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            + 1;
        g.positions
            .entry(account_id)
            .or_default()
            .push(PositionRow {
                id,
                account_id,
                asset_id: body.asset_id,
                purchase_date,
                units: body.units,
                cost_basis: body.cost_basis,
            });
        Ok(id)
    })
}

/// Apply a lot update, as `PATCH …/positions/{position}` would.
///
/// A lot keeps its place. (The route orders an account's lots by
/// `sort_order` and only then by `purchase_date`, so a new date can only move
/// a lot among lots sharing its `sort_order` — which lots added through the
/// route never do.)
pub(crate) fn update_position(
    graph: &mut ScenarioGraph,
    account_id: i64,
    position_id: i64,
    body: &UpdatePosition,
) -> ApiResult<()> {
    if body.units.is_some_and(|u| u < 0.0) || body.cost_basis.is_some_and(|b| b < 0.0) {
        return Err(ApiError::bad_request(
            "units and cost_basis must be non-negative",
        ));
    }
    let date = body
        .purchase_date
        .as_deref()
        .map(purchase_date)
        .transpose()?;
    atomically(graph, |g| {
        if let Some(asset_id) = body.asset_id {
            held_asset(g, asset_id)?;
        }
        let row = g
            .positions
            .get_mut(&account_id)
            .and_then(|lots| lots.iter_mut().find(|p| p.id == position_id))
            .ok_or(ApiError::NotFound("position"))?;
        if let Some(asset_id) = body.asset_id {
            row.asset_id = asset_id;
        }
        if let Some(date) = date {
            row.purchase_date = date;
        }
        if let Some(units) = body.units {
            row.units = units;
        }
        if let Some(cost_basis) = body.cost_basis {
            row.cost_basis = cost_basis;
        }
        Ok(())
    })
}

/// Remove a lot, as `DELETE …/positions/{position}` would.
pub(crate) fn delete_position(
    graph: &mut ScenarioGraph,
    account_id: i64,
    position_id: i64,
) -> ApiResult<()> {
    let lots = graph
        .positions
        .get_mut(&account_id)
        .ok_or(ApiError::NotFound("position"))?;
    let before = lots.len();
    lots.retain(|p| p.id != position_id);
    if lots.len() == before {
        return Err(ApiError::NotFound("position"));
    }
    // `load` only keys accounts that hold something.
    if lots.is_empty() {
        graph.positions.remove(&account_id);
    }
    Ok(())
}

/// A lot's date, parsed the way the routes parse it.
fn purchase_date(text: &str) -> ApiResult<String> {
    text.parse::<jiff::civil::Date>()
        .map(|d| d.to_string())
        .map_err(|e| ApiError::bad_request(format!("invalid purchase_date '{text}': {e}")))
}

/// The foreign key on `positions.asset_id`, narrowed to this plan.
fn held_asset(graph: &ScenarioGraph, asset_id: i64) -> ApiResult<()> {
    if graph.assets.iter().any(|a| a.id == asset_id) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "asset {asset_id} does not exist in this plan"
        )))
    }
}

fn set_sources(graph: &mut ScenarioGraph, sources: Vec<(i64, String)>) {
    for (amount_id, source) in sources {
        if let Some(row) = graph.amounts.get_mut(&amount_id) {
            row.expression_source = Some(source);
        }
    }
}

// ── the schema's deletes, in memory ─────────────────────────────────────────

/// Rows to delete; [`cascade`] grows it the way `ON DELETE CASCADE` would.
#[derive(Default)]
struct Doomed {
    events: HashSet<i64>,
    triggers: HashSet<i64>,
    effects: HashSet<i64>,
    amounts: HashSet<i64>,
}

/// Delete `doomed` and everything the schema's `ON DELETE CASCADE` foreign keys
/// among events, triggers, effects, amounts and withdrawal rows would take
/// with it, then re-index.
fn cascade(graph: &mut ScenarioGraph, mut doomed: Doomed) {
    let hit = |set: &HashSet<i64>, id: Option<i64>| id.is_some_and(|id| set.contains(&id));
    loop {
        let before = (
            doomed.triggers.len(),
            doomed.effects.len(),
            doomed.amounts.len(),
        );
        for t in graph.triggers.values() {
            if hit(&doomed.events, t.event_id)
                || hit(&doomed.events, t.ref_event_id)
                || hit(&doomed.triggers, t.parent_id)
                || hit(&doomed.triggers, t.start_trigger_id)
                || hit(&doomed.triggers, t.end_trigger_id)
            {
                doomed.triggers.insert(t.id);
            }
        }
        for a in graph.amounts.values() {
            if hit(&doomed.amounts, a.left_id) || hit(&doomed.amounts, a.right_id) {
                doomed.amounts.insert(a.id);
            }
        }
        for f in graph.effects.values() {
            if hit(&doomed.events, f.event_id)
                || hit(&doomed.events, f.target_event_id)
                || hit(&doomed.effects, f.parent_id)
                || hit(&doomed.amounts, f.amount_id)
                || hit(&doomed.amounts, f.down_payment_amount_id)
            {
                doomed.effects.insert(f.id);
            }
        }
        let after = (
            doomed.triggers.len(),
            doomed.effects.len(),
            doomed.amounts.len(),
        );
        if before == after {
            break;
        }
    }

    graph.events.retain(|e| !doomed.events.contains(&e.id));
    graph.triggers.retain(|id, _| !doomed.triggers.contains(id));
    graph.effects.retain(|id, _| !doomed.effects.contains(id));
    graph.amounts.retain(|id, _| !doomed.amounts.contains(id));
    graph
        .withdrawal_sources
        .retain(|id, _| !doomed.effects.contains(id));
    graph
        .withdrawal_items
        .retain(|id, _| !doomed.effects.contains(id));
    graph.reindex();
}

/// `events::collect_orphans`, in memory: drop amounts nothing references and
/// detached sub-conditions nothing links to, until neither sweep finds more.
fn collect_orphans(graph: &mut ScenarioGraph) {
    loop {
        let used: HashSet<i64> = graph
            .effects
            .values()
            .flat_map(|e| [e.amount_id, e.down_payment_amount_id])
            .chain(graph.amounts.values().flat_map(|a| [a.left_id, a.right_id]))
            .flatten()
            .collect();
        let orphans: HashSet<i64> = graph
            .amounts
            .keys()
            .copied()
            .filter(|id| !used.contains(id))
            .collect();
        if orphans.is_empty() {
            break;
        }
        cascade(
            graph,
            Doomed {
                amounts: orphans,
                ..Doomed::default()
            },
        );
    }

    loop {
        let linked: HashSet<i64> = graph
            .triggers
            .values()
            .flat_map(|t| [t.start_trigger_id, t.end_trigger_id])
            .flatten()
            .collect();
        let orphans: HashSet<i64> = graph
            .triggers
            .values()
            .filter(|t| t.event_id.is_none() && t.parent_id.is_none() && !linked.contains(&t.id))
            .map(|t| t.id)
            .collect();
        if orphans.is_empty() {
            break;
        }
        cascade(
            graph,
            Doomed {
                triggers: orphans,
                ..Doomed::default()
            },
        );
    }
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
