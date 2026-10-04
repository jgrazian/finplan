//! The plan-shaped GET bodies, answered from a plan held on the device.
//!
//! [`read_json`] is one entry for every `finplan_plan::read` function the web's
//! `PlanApi` needs, selected by a tagged [`ReadQuery`]; [`read_library_json`]
//! answers the library lists (return profiles, inflation profiles, tax configs)
//! with usage counted across all of the device's plans, as the server counts
//! across a user's.
//!
//! What each query returns (the generated types in `web/lib/api/generated`):
//!
//! | query | returns |
//! |---|---|
//! | `scenario` | `Scenario` |
//! | `accounts` / `account` | `Account[]` / `Account` |
//! | `positions` | `Position[]` |
//! | `assets` / `asset` | `Asset[]` / `Asset` |
//! | `events` / `event` | `Event[]` / `Event` |
//! | `parameters` | `NamedParameter[]` |
//! | `compile_report` | `CompileReport` |
//! | `preflight` | `PreflightReport` |
//! | `validate_expression` | `ExpressionValidation` |
//! | `history_presets` | `HistoryPreset[]` |
//! | `return_profiles` / `return_profile` | `Profile[]` / `Profile` |
//! | `inflation_profiles` | `Profile[]` |
//! | `tax_configs` / `tax_config` | `TaxConfig[]` / `TaxConfig` |

use finplan_plan::PlanError;
use finplan_plan::create::library_view;
use finplan_plan::expressions::ExpressionValidationRequest;
use finplan_plan::graph::ScenarioGraph;
use finplan_plan::library::Library;
use finplan_plan::read::{self, ScenarioExtras};
use finplan_plan::specs::scenarios::ScenarioStatus;
use serde::Deserialize;
use ts_rs::TS;

use crate::error::{EngineResult, parse, to_json};

/// What a scenario's response carries that its graph does not: the slug the
/// server assigns, whether it is a draft, and its latest run's result. All
/// optional; a local plan's slug is its id.
#[derive(Debug, Default, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ScenarioExtrasArg {
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub status: Option<ScenarioStatus>,
    #[serde(default)]
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub last_success_rate: Option<f64>,
}

/// One read of a plan. JSON is `{"query": "account", "id": 3}`.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "query", rename_all = "snake_case")]
#[ts(export)]
pub enum ReadQuery {
    /// `GET /scenarios/{id}`; `extras` fills what the graph lacks.
    Scenario {
        #[serde(default)]
        #[ts(optional)]
        extras: Option<ScenarioExtrasArg>,
    },
    /// `GET …/accounts`
    Accounts,
    /// `GET …/accounts/{id}`
    Account { id: i64 },
    /// `GET …/accounts/{account_id}/positions`
    Positions { account_id: i64 },
    /// `GET …/assets`
    Assets,
    /// `GET …/assets/{id}`
    Asset { id: i64 },
    /// `GET …/events`
    Events,
    /// `GET …/events/{id}`
    Event { id: i64 },
    /// `GET …/parameters`
    Parameters,
    /// `POST …/compile`
    CompileReport,
    /// `GET …/preflight`
    Preflight,
    /// `POST …/expressions/validate`
    ValidateExpression {
        request: ExpressionValidationRequest,
    },
    /// `GET /history-presets`
    HistoryPresets,
}

/// Answer `query` about the plan. The library is attached to the plan first.
pub fn read_json(graph: &str, library: &str, query: &str) -> EngineResult<String> {
    let mut graph: ScenarioGraph = parse("plan", graph)?;
    let library: Library = parse("library", library)?;
    let query: ReadQuery = parse("query", query)?;
    library.attach(&mut graph);
    match query {
        ReadQuery::Scenario { extras } => {
            let extras = extras.unwrap_or_default();
            to_json(&read::scenario(
                &graph,
                ScenarioExtras {
                    slug: extras.slug.unwrap_or_else(|| graph.scenario.id.to_string()),
                    status: extras.status.unwrap_or(ScenarioStatus::Active),
                    last_run_at: extras.last_run_at,
                    last_success_rate: extras.last_success_rate,
                },
            ))
        }
        ReadQuery::Accounts => to_json(&read::accounts(&graph)?),
        ReadQuery::Account { id } => to_json(&read::account(&graph, id)?),
        ReadQuery::Positions { account_id } => to_json(&read::positions(&graph, account_id)),
        ReadQuery::Assets => to_json(&read::assets(&graph)),
        ReadQuery::Asset { id } => to_json(&read::asset(&graph, id)?),
        ReadQuery::Events => to_json(&read::events(&graph)?),
        ReadQuery::Event { id } => to_json(&read::event(&graph, id)?),
        ReadQuery::Parameters => to_json(&read::parameters(&graph)?),
        ReadQuery::CompileReport => to_json(&read::compile_report(&graph)?),
        ReadQuery::Preflight => to_json(&read::preflight(&graph)),
        ReadQuery::ValidateExpression { request } => {
            to_json(&read::validate_expression(&graph, &request.effect)?)
        }
        ReadQuery::HistoryPresets => to_json(&read::history_presets()),
    }
}

/// One read of the library. JSON is `{"query": "tax_config", "id": 3}`.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "query", rename_all = "snake_case")]
#[ts(export)]
pub enum LibraryReadQuery {
    /// `GET /return-profiles`
    ReturnProfiles,
    /// `GET /return-profiles/{id}`
    ReturnProfile { id: i64 },
    /// `GET /inflation-profiles`
    InflationProfiles,
    /// `GET /tax-configs`
    TaxConfigs,
    /// `GET /tax-configs/{id}`
    TaxConfig { id: i64 },
}

/// Answer `query` about the library. `plans` is every plan of the device
/// (`ScenarioGraph[]`), which is where a return profile's `used_by` is counted.
pub fn read_library_json(library: &str, plans: &str, query: &str) -> EngineResult<String> {
    let library: Library = parse("library", library)?;
    let plans: Vec<ScenarioGraph> = parse("plans", plans)?;
    let query: LibraryReadQuery = parse("query", query)?;
    let view = library_view(&library);
    let used_by = |id| read::profile_users_across(&plans, id);
    match query {
        LibraryReadQuery::ReturnProfiles => {
            to_json(&read::return_profiles_with_usage(&view, used_by)?)
        }
        LibraryReadQuery::ReturnProfile { id } => {
            let profile = read::return_profiles_with_usage(&view, used_by)?
                .into_iter()
                .find(|profile| profile.id == id)
                .ok_or(PlanError::NotFound("return profile"))?;
            to_json(&profile)
        }
        LibraryReadQuery::InflationProfiles => to_json(&read::inflation_profiles(&view)?),
        LibraryReadQuery::TaxConfigs => to_json(&read::tax_configs(&view)),
        LibraryReadQuery::TaxConfig { id } => to_json(&read::tax_config(&view, id)?),
    }
}
