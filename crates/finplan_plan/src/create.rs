//! Making a plan: a new one from a request, and a copy of an existing one.
//!
//! The in-memory twins of `POST /scenarios` and `POST /scenarios/{id}/duplicate`.
//! What a database decides for itself is the caller's here: the new plan's id,
//! the time it was made (`now`, in SQLite's `datetime('now')` form,
//! `2026-10-03 14:05:09`, so a plan made here and one made by the server read
//! alike), a name no other plan of the user's has, and any limit on how many
//! plans there may be.

use finplan_core::config::DEFAULT_DEFERRED_TAX_RATE;

use crate::error::{PlanError, PlanResult};
use crate::graph::{ScenarioGraph, ScenarioRow};
use crate::library::Library;
use crate::specs::scenarios::{CreateScenario, validate_date};

/// The owner a plan made here is given: it is the device's. (The row's
/// `user_id` is part of the snapshot, so the same plan owned by a server
/// account hashes differently.)
pub const LOCAL_USER_ID: &str = "local";

/// What `POST /scenarios` creates: a scenario row and nothing else. The plan
/// has no accounts, assets or events, and uses the tax config and inflation
/// profile the body names (none, when it names none; there is no default).
/// The library's tables are attached, as a load would.
///
/// Refused as the route refuses it, in its order: an assumption that is not in
/// `library` ("assumption must belong to your account"), a bad date, a blank
/// name. A horizon outside 1 to 120 years is refused too, which the route
/// leaves to the table's CHECK.
pub fn new_plan(
    body: &CreateScenario,
    library: &Library,
    id: i64,
    now: &str,
) -> PlanResult<ScenarioGraph> {
    let owned = |profile: Option<i64>, tax: Option<i64>| {
        profile.is_none_or(|id| library.inflation_profiles.iter().any(|p| p.id == id))
            && tax.is_none_or(|id| library.tax_configs.iter().any(|t| t.id == id))
    };
    if !owned(body.inflation_profile_id, body.tax_config_id) {
        return Err(PlanError::invalid("assumption must belong to your account"));
    }
    let start_date = validate_date(&body.start_date, "start_date")?;
    let birth_date = body
        .birth_date
        .as_deref()
        .map(|d| validate_date(d, "birth_date"))
        .transpose()?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(PlanError::invalid("scenario name cannot be empty"));
    }
    if !(1..=120).contains(&body.duration_years) {
        return Err(PlanError::invalid(
            "duration_years must be between 1 and 120",
        ));
    }

    let mut graph = empty_graph(ScenarioRow {
        id,
        user_id: LOCAL_USER_ID.to_string(),
        name: name.to_string(),
        description: body.description.clone(),
        start_date,
        birth_date,
        duration_years: body.duration_years,
        inflation_profile_id: body.inflation_profile_id,
        tax_config_id: body.tax_config_id,
        collect_ledger: 1,
        funding_strategy: None,
        funding_bracket_ceiling: None,
        deferred_tax_rate: DEFAULT_DEFERRED_TAX_RATE,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    });
    library.attach(&mut graph);
    Ok(graph)
}

/// What `POST /scenarios/{id}/duplicate` makes: `graph` as a plan of its own,
/// named `name`, with `new_id` and `now` for its id and timestamps.
///
/// The route copies every row under fresh database ids; the rows' ids are
/// only ever a plan's own, so here they are kept, and what refers to them
/// still does. The library tables come along with the graph. A blank name is
/// refused as the route refuses it.
pub fn duplicate(
    graph: &ScenarioGraph,
    new_id: i64,
    name: &str,
    now: &str,
) -> PlanResult<ScenarioGraph> {
    let name = name.trim();
    if name.is_empty() {
        return Err(PlanError::invalid("scenario name cannot be empty"));
    }
    let mut copy = graph.clone();
    copy.scenario.id = new_id;
    copy.scenario.name = name.to_string();
    copy.scenario.created_at = now.to_string();
    copy.scenario.updated_at = now.to_string();
    Ok(copy)
}

/// A plan with no accounts, assets, events or library: just `scenario`.
fn empty_graph(scenario: ScenarioRow) -> ScenarioGraph {
    ScenarioGraph {
        scenario,
        funding_excludes: Vec::new(),
        assets: Vec::new(),
        accounts: Vec::new(),
        bank: Default::default(),
        investment: Default::default(),
        property: Default::default(),
        liability: Default::default(),
        positions: Default::default(),
        return_profiles: Default::default(),
        distributions: Default::default(),
        inflation_profile_name: None,
        inflation_distribution_id: None,
        tax_config: None,
        tax_brackets: Vec::new(),
        tax_configs: Default::default(),
        inflation_profiles: Default::default(),
        events: Vec::new(),
        parameters: Vec::new(),
        triggers: Default::default(),
        trigger_children: Default::default(),
        event_trigger: Default::default(),
        amounts: Default::default(),
        effects: Default::default(),
        event_effects: Default::default(),
        effect_children: Default::default(),
        withdrawal_sources: Default::default(),
        withdrawal_items: Default::default(),
    }
}

/// The library as a graph: a plan with nothing in it but the library's tables
/// attached, which is what the library readers in [`crate::read`]
/// (`return_profiles`, `tax_configs`, ...) read. It is never a plan of the
/// user's.
pub fn library_view(library: &Library) -> ScenarioGraph {
    let mut graph = empty_graph(ScenarioRow {
        id: 0,
        user_id: LOCAL_USER_ID.to_string(),
        name: "library".to_string(),
        description: None,
        start_date: "2000-01-01".to_string(),
        birth_date: None,
        duration_years: 1,
        inflation_profile_id: None,
        tax_config_id: None,
        collect_ledger: 0,
        funding_strategy: None,
        funding_bracket_ceiling: None,
        deferred_tax_rate: DEFAULT_DEFERRED_TAX_RATE,
        created_at: String::new(),
        updated_at: String::new(),
    });
    library.attach(&mut graph);
    graph
}
