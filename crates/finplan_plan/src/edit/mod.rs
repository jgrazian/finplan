//! Plan edits applied to an in-memory [`ScenarioGraph`] instead of the database.
//!
//! Each operation is the in-memory twin of one write route. They agree because
//! they share code, not because they were written alike: the request specs
//! and their checks (`specs::*`, `expressions`, `expression_refs`), the
//! lowering of an event's trees into a [`RowBatch`], and the pure steps the
//! route's SQL would otherwise repeat (a flavor's detail row, a distribution's
//! columns, the refusals and their messages) are plan-crate functions that the
//! route calls to decide what to write and this module calls to decide what to
//! put in the graph. What stays in the route is what only a database can say:
//! ownership, foreign keys across scenarios, unique constraints, and the SQL
//! itself. Compiling the edited graph gives what compiling the plan would give
//! after the route had run, which is what lets a change be previewed from a
//! run's snapshot without writing; the server's `domain::edit_route_tests`
//! hold the two to that.
//!
//! Every operation is atomic: it works on a copy and only replaces `graph` once
//! everything has succeeded, the way the route's transaction would roll back.
//!
//! [`apply`] is the single entry point: one [`EditOp`] per write route the web
//! calls, run on a graph, reporting the id of a row it created.
//!
//! What does not carry over from the database:
//!
//!   * references are checked against this plan only, where a foreign key
//!     accepts a row from any scenario; and
//!   * `updated_at` and other bookkeeping columns are not modelled.

use std::collections::HashSet;

use crate::batch::RowBatch;
use crate::error::{PlanError, PlanResult};
use crate::expression_refs::{self, Entity};
use crate::expressions::validate_tree;
use crate::graph::{
    AccountRow, AssetRow, EventRow, InflationEntry, ParameterRow, PositionRow, ScenarioGraph,
    Table, TaxConfigEntry,
};
use crate::library;
use crate::specs::accounts::{
    self, CreateAccount, CreatePosition, DetailRow, FlavorSpec, UpdateAccount, UpdatePosition,
};
use crate::specs::assets::{self, CreateAsset, UpdateAsset};
use crate::specs::events::{self, EventBody, lower_tree};
use crate::specs::parameters::{self, ParameterBody};
use crate::specs::profiles::CreateProfile;
use crate::specs::scenarios::{UpdateScenario, validate_date};
use crate::specs::taxes::CreateTaxConfig;

/// Run `edit` on a copy of `graph`, keeping the result only if it succeeds.
fn atomically<T>(
    graph: &mut ScenarioGraph,
    edit: impl FnOnce(&mut ScenarioGraph) -> PlanResult<T>,
) -> PlanResult<T> {
    let mut staged = graph.clone();
    let out = edit(&mut staged)?;
    *graph = staged;
    Ok(out)
}

// ── events ───────────────────────────────────────────────────────────────────

/// Add an event, as `POST /scenarios/{id}/events` would. Returns its new id.
pub fn create_event(graph: &mut ScenarioGraph, body: &EventBody) -> PlanResult<i64> {
    validate_tree(graph, &body.effects)?;
    atomically(graph, |g| {
        let name = body.name.trim();
        if g.events.iter().any(|e| e.name == name) {
            return Err(PlanError::Conflict(events::NAME_TAKEN.into()));
        }
        let id = g.next_id(Table::Events);
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
pub fn replace_event(graph: &mut ScenarioGraph, event_id: i64, body: &EventBody) -> PlanResult<()> {
    validate_tree(graph, &body.effects)?;
    atomically(graph, |g| {
        if !g.events.iter().any(|e| e.id == event_id) {
            return Err(PlanError::NotFound("event"));
        }
        let name = body.name.trim();
        if g.events.iter().any(|e| e.id != event_id && e.name == name) {
            return Err(PlanError::Conflict(events::NAME_TAKEN.into()));
        }
        let row = g
            .events
            .iter_mut()
            .find(|e| e.id == event_id)
            .ok_or(PlanError::NotFound("event"))?;
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
pub fn delete_event(graph: &mut ScenarioGraph, event_id: i64) -> PlanResult<()> {
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
            return Err(events::referenced_by(&referrers));
        }

        if !g.events.iter().any(|e| e.id == event_id) {
            return Err(PlanError::NotFound("event"));
        }
        let mut doomed = Doomed::default();
        doomed.events.insert(event_id);
        cascade(g, doomed);
        collect_orphans(g);
        Ok(())
    })
}

fn write_tree(graph: &mut ScenarioGraph, event_id: i64, body: &EventBody) -> PlanResult<()> {
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
pub fn create_asset(graph: &mut ScenarioGraph, body: &CreateAsset) -> PlanResult<i64> {
    // The route checks the profile is the caller's before anything else; the
    // graph carries the caller's whole profile library.
    if let Some(profile_id) = body.return_profile_id
        && !graph.return_profiles.contains_key(&profile_id)
    {
        return Err(PlanError::NotFound("return profile"));
    }
    assets::check_initial_price(body.initial_price)?;
    atomically(graph, |g| {
        let name = body.name.trim();
        if g.assets.iter().any(|a| a.name == name) {
            return Err(PlanError::Conflict(assets::NAME_TAKEN.into()));
        }
        // CHECK constraint on the table.
        if body.tracking_error.is_some_and(|t| t < 0.0) {
            return Err(PlanError::invalid("tracking_error cannot be negative"));
        }
        let id = g.next_id(Table::Assets);
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

pub fn update_asset(
    graph: &mut ScenarioGraph,
    asset_id: i64,
    body: &UpdateAsset,
) -> PlanResult<()> {
    // The route checks the profile is the caller's before anything else; the
    // graph carries the caller's whole profile library.
    if let Some(Some(profile_id)) = body.return_profile_id
        && !graph.return_profiles.contains_key(&profile_id)
    {
        return Err(PlanError::NotFound("return profile"));
    }
    atomically(graph, |g| {
        if !g.assets.iter().any(|a| a.id == asset_id) {
            return Err(PlanError::NotFound("asset"));
        }
        let name = body.name.as_deref().map(str::trim);
        if let Some(name) = name
            && g.assets.iter().any(|a| a.id != asset_id && a.name == name)
        {
            return Err(PlanError::Conflict(assets::NAME_TAKEN.into()));
        }
        // CHECK constraints on the table.
        if let Some(price) = body.initial_price {
            assets::check_initial_price(price)?;
        }
        if body.tracking_error.flatten().is_some_and(|t| t < 0.0) {
            return Err(PlanError::invalid("tracking_error cannot be negative"));
        }
        let renamed = match name {
            Some(name) => expression_refs::rerendered(g, Entity::Asset(asset_id), name)?,
            None => Vec::new(),
        };

        let row = g
            .assets
            .iter_mut()
            .find(|a| a.id == asset_id)
            .ok_or(PlanError::NotFound("asset"))?;
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
pub fn create_account(graph: &mut ScenarioGraph, body: &CreateAccount) -> PlanResult<i64> {
    body.flavor.validate()?;
    atomically(graph, |g| {
        let name = body.name.trim();
        if g.accounts.iter().any(|a| a.name == name) {
            return Err(PlanError::Conflict(accounts::NAME_TAKEN.into()));
        }
        let id = g.next_id(Table::Accounts);
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
pub fn update_account(
    graph: &mut ScenarioGraph,
    account_id: i64,
    body: &UpdateAccount,
) -> PlanResult<()> {
    atomically(graph, |g| {
        let existing_flavor = g
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .map(|a| a.flavor.clone())
            .ok_or(PlanError::NotFound("account"))?;

        body.check(&existing_flavor)?;

        let name = body.name.as_deref().map(str::trim);
        if let Some(name) = name
            && g.accounts
                .iter()
                .any(|a| a.id != account_id && a.name == name)
        {
            return Err(PlanError::Conflict(accounts::NAME_TAKEN.into()));
        }
        let renamed = match name {
            Some(name) => expression_refs::rerendered(g, Entity::Account(account_id), name)?,
            None => Vec::new(),
        };

        let row = g
            .accounts
            .iter_mut()
            .find(|a| a.id == account_id)
            .ok_or(PlanError::NotFound("account"))?;
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
) -> PlanResult<()> {
    let profile = |id: i64| {
        if graph.return_profiles.contains_key(&id) {
            Ok(())
        } else {
            Err(PlanError::invalid(format!(
                "return profile {id} does not exist"
            )))
        }
    };
    match flavor.detail_row(account_id) {
        DetailRow::Bank(row) => {
            profile(row.return_profile_id)?;
            graph.bank.insert(account_id, row);
        }
        DetailRow::Investment(row) => {
            profile(row.cash_return_profile_id)?;
            graph.investment.insert(account_id, row);
        }
        DetailRow::Property(row) => {
            if !graph.assets.iter().any(|a| a.id == row.asset_id) {
                return Err(PlanError::invalid(format!(
                    "asset {} does not exist in this plan",
                    row.asset_id
                )));
            }
            graph.property.insert(account_id, row);
        }
        DetailRow::Liability(row) => {
            if let Some(payer) = row.repay_from_account_id {
                accounts::check_loan_payer(
                    graph
                        .accounts
                        .iter()
                        .find(|a| a.id == payer)
                        .map(|a| a.flavor.as_str()),
                )?;
            }
            graph.liability.insert(account_id, row);
        }
    }
    Ok(())
}

/// Delete an asset, as `DELETE /scenarios/{id}/assets/{asset}` would.
///
/// Stricter than the route, which lets the schema's cascades quietly delete the
/// lots, events and amounts that name the asset: a change batch that deletes it
/// must first remove what still points at it, so nothing disappears unseen.
pub fn delete_asset(graph: &mut ScenarioGraph, asset_id: i64) -> PlanResult<()> {
    atomically(graph, |g| {
        if !g.assets.iter().any(|a| a.id == asset_id) {
            return Err(PlanError::NotFound("asset"));
        }
        expression_refs::refuse_if_used(g, Entity::Asset(asset_id))?;
        let referrers = referrers(g, Held::Asset(asset_id));
        if !referrers.is_empty() {
            return Err(PlanError::Conflict(format!(
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
pub fn delete_account(graph: &mut ScenarioGraph, account_id: i64) -> PlanResult<()> {
    atomically(graph, |g| {
        if !g.accounts.iter().any(|a| a.id == account_id) {
            return Err(PlanError::NotFound("account"));
        }
        expression_refs::refuse_if_used(g, Entity::Account(account_id))?;
        let referrers = referrers(g, Held::Account(account_id));
        if !referrers.is_empty() {
            return Err(PlanError::Conflict(format!(
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
pub fn create_parameter(graph: &mut ScenarioGraph, body: &ParameterBody) -> PlanResult<i64> {
    let name = parameters::validate(body)?.to_owned();
    atomically(graph, |g| {
        if g.parameters.iter().any(|p| p.name == name) {
            return Err(PlanError::Conflict(parameters::NAME_TAKEN.into()));
        }
        let id = g.next_id(Table::Parameters);
        g.parameters.push(parameter_row(id, name, body));
        Ok(id)
    })
}

/// Rewrite a parameter, as `PATCH /scenarios/{id}/parameters/{parameter}`
/// would: a rename rewrites the expressions that use it, and a change of type
/// is refused while anything does.
pub fn update_parameter(
    graph: &mut ScenarioGraph,
    parameter_id: i64,
    body: &ParameterBody,
) -> PlanResult<()> {
    let name = parameters::validate(body)?.to_owned();
    atomically(graph, |g| {
        let old = g
            .parameters
            .iter()
            .find(|p| p.id == parameter_id)
            .ok_or(PlanError::NotFound("parameter"))?;
        parameters::check_retype(g, parameter_id, &old.kind, body)?;
        if g.parameters
            .iter()
            .any(|p| p.id != parameter_id && p.name == name)
        {
            return Err(PlanError::Conflict(parameters::NAME_TAKEN.into()));
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
            .ok_or(PlanError::NotFound("parameter"))?;
        *row = parameter_row(parameter_id, name, body);
        set_sources(g, renamed);
        Ok(())
    })
}

/// Delete a parameter nothing uses, as `DELETE /scenarios/{id}/parameters/{parameter}`.
pub fn delete_parameter(graph: &mut ScenarioGraph, parameter_id: i64) -> PlanResult<()> {
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
pub fn update_scenario(graph: &mut ScenarioGraph, body: &UpdateScenario) -> PlanResult<()> {
    body.check_name()?;
    let (start_date, birth_date) = body.dates()?;
    body.check_duration()?;
    atomically(graph, |g| {
        if let Some(id) = body.tax_config_id
            && g.scenario.tax_config_id != Some(id)
        {
            let TaxConfigEntry {
                config, brackets, ..
            } = g
                .tax_configs
                .get(&id)
                .cloned()
                .ok_or(PlanError::NotFound("tax config"))?;
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
                ..
            } = g
                .inflation_profiles
                .get(&id)
                .cloned()
                .ok_or(PlanError::NotFound("inflation profile"))?;
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

/// Add a return profile to the caller's library, as `POST /return-profiles`
/// would. Returns its id, which exists only in this graph.
pub fn create_return_profile(graph: &mut ScenarioGraph, body: &CreateProfile) -> PlanResult<i64> {
    atomically(graph, |g| {
        let id = g.next_id(Table::ReturnProfiles);
        let sort_order = g
            .return_profiles
            .values()
            .map(|p| p.sort_order)
            .max()
            .map_or(0, |max| max + 1);
        let new = library::new_return_profile(
            body,
            g.return_profiles.values().map(|p| p.name.as_str()),
            id,
            g.next_id(Table::Distributions),
            sort_order,
        )?;
        for row in new.distributions {
            g.distributions.insert(row.id, row);
        }
        g.return_profiles.insert(id, new.profile.row());
        Ok(id)
    })
}

/// Add a tax config to the caller's library, as `POST /tax-configs` would.
/// Returns its id, which exists only in this graph.
pub fn create_tax_config(graph: &mut ScenarioGraph, body: &CreateTaxConfig) -> PlanResult<i64> {
    atomically(graph, |g| {
        let id = g.next_id(Table::TaxConfigs);
        let config = library::new_tax_config(
            body,
            g.tax_configs.values().map(|c| c.config.name.as_str()),
            id,
        )?;
        g.tax_configs.insert(id, config.entry());
        Ok(id)
    })
}

// ── ordering ─────────────────────────────────────────────────────────────────

/// The order `requested` asks for, reconciled against what the collection
/// actually holds.
///
/// Named rows come first, in the order named; anything the caller did not name
/// keeps its place behind them. An id that is not in the collection is dropped
/// rather than refused: a list a beat out of date — a row deleted in another
/// tab, one belonging to a scenario the caller does not own — should still
/// reorder under the user's hand rather than fail there.
pub fn reconcile(current: &[i64], requested: &[i64]) -> Vec<i64> {
    let known: HashSet<i64> = current.iter().copied().collect();
    let mut placed: HashSet<i64> = HashSet::new();
    let mut order: Vec<i64> = requested
        .iter()
        .copied()
        .filter(|id| known.contains(id) && placed.insert(*id))
        .collect();
    order.extend(current.iter().copied().filter(|id| !placed.contains(id)));
    order
}

/// Put the plan's events in the order `ids` names, as
/// `POST /scenarios/{id}/events/reorder` would: [`reconcile`]d against the
/// events, and only if that moves anything, every event's `sort_order` becomes
/// its place. Never refused.
pub fn reorder_events(graph: &mut ScenarioGraph, ids: &[i64]) {
    let current: Vec<i64> = ranked(graph.events.iter().map(|e| (e.sort_order, e.id)));
    renumber(&current, ids, |id, rank| {
        if let Some(e) = graph.events.iter_mut().find(|e| e.id == id) {
            e.sort_order = rank;
        }
    });
    sort_events(graph);
}

/// Put the plan's assets in the order `ids` names; see [`reorder_events`].
pub fn reorder_assets(graph: &mut ScenarioGraph, ids: &[i64]) {
    let current: Vec<i64> = ranked(graph.assets.iter().map(|a| (a.sort_order, a.id)));
    renumber(&current, ids, |id, rank| {
        if let Some(a) = graph.assets.iter_mut().find(|a| a.id == id) {
            a.sort_order = rank;
        }
    });
    graph.assets.sort_by_key(|a| (a.sort_order, a.id));
}

/// Put the plan's accounts in the order `ids` names; see [`reorder_events`].
pub fn reorder_accounts(graph: &mut ScenarioGraph, ids: &[i64]) {
    let current: Vec<i64> = ranked(graph.accounts.iter().map(|a| (a.sort_order, a.id)));
    renumber(&current, ids, |id, rank| {
        if let Some(a) = graph.accounts.iter_mut().find(|a| a.id == id) {
            a.sort_order = rank;
        }
    });
    graph.accounts.sort_by_key(|a| (a.sort_order, a.id));
}

/// Put one account's lots in the order `ids` names, as
/// `POST …/accounts/{account}/positions/reorder` would. A lot's place is its
/// place in the account's list (the graph does not carry the column the route
/// numbers), so the list itself is rearranged. An account with no lots, or
/// none by that id, has nothing to reorder, and that is not an error.
pub fn reorder_positions(graph: &mut ScenarioGraph, account_id: i64, ids: &[i64]) {
    let Some(lots) = graph.positions.get_mut(&account_id) else {
        return;
    };
    let current: Vec<i64> = lots.iter().map(|p| p.id).collect();
    let order = reconcile(&current, ids);
    if order == current {
        return;
    }
    lots.sort_by_key(|p| order.iter().position(|id| *id == p.id));
}

/// Row ids in the order a list route reads them: by `sort_order`, then id.
fn ranked(rows: impl Iterator<Item = (i64, i64)>) -> Vec<i64> {
    let mut rows: Vec<(i64, i64)> = rows.collect();
    rows.sort();
    rows.into_iter().map(|(_, id)| id).collect()
}

/// `api::apply_order`'s renumbering: nothing when the list would not move,
/// else every row's `sort_order` is its rank in the reconciled order.
pub(crate) fn renumber(current: &[i64], requested: &[i64], mut set: impl FnMut(i64, i64)) {
    let order = reconcile(current, requested);
    if order == current {
        return;
    }
    for (rank, id) in order.into_iter().enumerate() {
        set(id, rank as i64);
    }
}

// ── positions ────────────────────────────────────────────────────────────────

/// Add a lot, as `POST /scenarios/{id}/accounts/{account}/positions` would.
/// Returns its new id.
///
/// The route appends at the account's highest `sort_order` + 1, which always
/// sorts last, so appending to the account's list reproduces its order.
pub fn create_position(
    graph: &mut ScenarioGraph,
    account_id: i64,
    body: &CreatePosition,
) -> PlanResult<i64> {
    atomically(graph, |g| {
        accounts::check_lot_home(
            g.accounts
                .iter()
                .find(|a| a.id == account_id)
                .map(|a| a.flavor.as_str()),
        )?;
        let purchase_date = match body.purchase_date.as_deref() {
            Some(d) => validate_date(d, "purchase_date")?,
            None => g.scenario.start_date.clone(),
        };
        accounts::check_lot_figures(Some(body.units), Some(body.cost_basis))?;
        held_asset(g, body.asset_id)?;
        let id = g.next_id(Table::Positions);
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
pub fn update_position(
    graph: &mut ScenarioGraph,
    account_id: i64,
    position_id: i64,
    body: &UpdatePosition,
) -> PlanResult<()> {
    accounts::check_lot_figures(body.units, body.cost_basis)?;
    let date = body
        .purchase_date
        .as_deref()
        .map(|d| validate_date(d, "purchase_date"))
        .transpose()?;
    atomically(graph, |g| {
        if let Some(asset_id) = body.asset_id {
            held_asset(g, asset_id)?;
        }
        let row = g
            .positions
            .get_mut(&account_id)
            .and_then(|lots| lots.iter_mut().find(|p| p.id == position_id))
            .ok_or(PlanError::NotFound("position"))?;
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
pub fn delete_position(
    graph: &mut ScenarioGraph,
    account_id: i64,
    position_id: i64,
) -> PlanResult<()> {
    let lots = graph
        .positions
        .get_mut(&account_id)
        .ok_or(PlanError::NotFound("position"))?;
    let before = lots.len();
    lots.retain(|p| p.id != position_id);
    if lots.len() == before {
        return Err(PlanError::NotFound("position"));
    }
    // `load` only keys accounts that hold something.
    if lots.is_empty() {
        graph.positions.remove(&account_id);
    }
    Ok(())
}

/// The foreign key on `positions.asset_id`, narrowed to this plan.
fn held_asset(graph: &ScenarioGraph, asset_id: i64) -> PlanResult<()> {
    if graph.assets.iter().any(|a| a.id == asset_id) {
        Ok(())
    } else {
        Err(PlanError::invalid(format!(
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

mod op;
pub use op::{EditOp, EditOutcome, apply};

#[cfg(test)]
mod tests;
