//! The user's library: return profiles, inflation profiles and tax configs.
//!
//! These belong to the person, not to a plan: every plan points into the same
//! library (an asset names a return profile, a scenario names an inflation
//! profile and a tax config), and editing a profile changes every plan that
//! uses it. A [`ScenarioGraph`] therefore carries a *copy* of the library
//! tables it needs — `return_profiles` and `distributions` are serialized with
//! the graph, `tax_configs` and `inflation_profiles` are not — and a scenario
//! row keeps denormalized copies of its own tax config and inflation profile.
//!
//! [`Library`] is the library as one serializable value, for a store that keeps
//! it apart from the plans (the browser's, where a graph is JSON and
//! `tax_configs`/`inflation_profiles` do not survive the round trip):
//!
//!   * [`Library::attach`] fills a graph's library tables and refreshes the
//!     scenario's copies, so a graph read from storage equals what the
//!     server's `db::graph::load` builds;
//!   * [`Library::from_graph`] takes a library back out of an attached graph;
//!   * [`apply_library`] is the in-memory twin of the library write routes
//!     (`api/profiles.rs`, `api/taxes.rs`), as [`crate::edit::apply`] is of the
//!     plan's;
//!   * [`seed`] is the starter library every new account is given.
//!
//! Like the plan edits, a library edit is atomic and does not model
//! bookkeeping: the routes bump `updated_at` on the scenarios a changed
//! profile or tax config touches, and a caller that keeps timestamps does the
//! same. After [`apply_library`], call [`Library::attach`] on every plan, so
//! the plans' copies follow (a deleted inflation profile or tax config
//! leaves the scenarios that used it with none, as `ON DELETE SET NULL`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::edit::{EditOutcome, renumber};
use crate::error::{PlanError, PlanResult};
use crate::graph::{
    DistributionRow, InflationEntry, ReturnProfileRow, ScenarioGraph, TaxBracketRow,
    TaxConfigEntry, TaxConfigRow,
};
use crate::specs::profiles::{
    self, AssetClass, CreateProfile, DistributionSpec, UpdateProfile, check_inflation_kind,
};
use crate::specs::taxes::{self, Bracket, CreateTaxConfig, UpdateTaxConfig};

/// A return profile of the library. `sort_order` is the column its list route
/// orders by (`sort_order, name`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryReturnProfile {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    /// An [`AssetClass`] by name, as stored; null for a hand-made profile.
    pub asset_class: Option<String>,
    /// The root of its distribution, in [`Library::distributions`].
    pub distribution_id: i64,
    pub sort_order: i64,
}

/// An inflation profile of the library, listed by `sort_order, name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryInflationProfile {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub distribution_id: i64,
    pub sort_order: i64,
}

/// A tax config of the library with its bracket table (ascending thresholds),
/// listed by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryTaxConfig {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub state_rate: f64,
    pub capital_gains_rate: f64,
    pub early_withdrawal_penalty_rate: f64,
    pub standard_deduction: f64,
    pub age_65_extra_deduction: f64,
    pub federal_brackets: Vec<Bracket>,
}

/// The user's library as a value. Every list is in id order; the order the
/// routes show them in is [`Library::return_profile_order`] and its siblings.
///
/// `distributions` holds every distribution the profiles use, including the
/// bull and bear children of a regime-switching one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Library {
    #[serde(default)]
    pub return_profiles: Vec<LibraryReturnProfile>,
    #[serde(default)]
    pub inflation_profiles: Vec<LibraryInflationProfile>,
    #[serde(default)]
    pub tax_configs: Vec<LibraryTaxConfig>,
    #[serde(default)]
    pub distributions: Vec<DistributionRow>,
}

/// How the library's list routes order profiles: `ORDER BY sort_order, name`.
/// [`crate::read`] orders its library lists with the same keys.
pub(crate) fn listed(sort_order: i64, name: &str) -> (i64, &str) {
    (sort_order, name)
}

/// How the tax config list route orders: `ORDER BY name`.
pub(crate) fn listed_by_name(name: &str) -> &str {
    name
}

impl LibraryReturnProfile {
    pub(crate) fn row(&self) -> ReturnProfileRow {
        ReturnProfileRow {
            asset_class: self.asset_class.clone(),
            id: self.id,
            name: self.name.clone(),
            description: self.description.clone(),
            distribution_id: self.distribution_id,
            sort_order: self.sort_order,
        }
    }
}

impl LibraryTaxConfig {
    pub(crate) fn entry(&self) -> TaxConfigEntry {
        let mut brackets: Vec<TaxBracketRow> = self
            .federal_brackets
            .iter()
            .map(|b| TaxBracketRow {
                threshold: b.threshold,
                rate: b.rate,
            })
            .collect();
        // The loader reads them `ORDER BY threshold`.
        brackets.sort_by(|a, b| a.threshold.total_cmp(&b.threshold));
        TaxConfigEntry {
            config: TaxConfigRow {
                id: self.id,
                name: self.name.clone(),
                state_rate: self.state_rate,
                capital_gains_rate: self.capital_gains_rate,
                early_withdrawal_penalty_rate: self.early_withdrawal_penalty_rate,
                standard_deduction: self.standard_deduction,
                age_65_extra_deduction: self.age_65_extra_deduction,
            },
            description: self.description.clone(),
            brackets,
        }
    }
}

impl Library {
    /// Fill `graph`'s library tables from this library, and refresh the
    /// scenario's denormalized copies (`tax_config`, `tax_brackets`,
    /// `inflation_profile_name`, `inflation_distribution_id`) from its
    /// `tax_config_id` and `inflation_profile_id`, which leaves the graph as
    /// `db::graph::load` would have built it from a database holding this
    /// library.
    ///
    /// A scenario that names a tax config or inflation profile the library
    /// lacks has the reference cleared, as the foreign keys'
    /// `ON DELETE SET NULL` would have; a loaded graph never holds a dangling
    /// one.
    pub fn attach(&self, graph: &mut ScenarioGraph) {
        graph.return_profiles = self
            .return_profiles
            .iter()
            .map(|p| (p.id, p.row()))
            .collect();
        graph.distributions = self
            .distributions
            .iter()
            .map(|d| (d.id, d.clone()))
            .collect();
        graph.tax_configs = self.tax_configs.iter().map(|t| (t.id, t.entry())).collect();
        graph.inflation_profiles = self
            .inflation_profiles
            .iter()
            .map(|p| {
                (
                    p.id,
                    InflationEntry {
                        name: p.name.clone(),
                        distribution_id: p.distribution_id,
                        description: p.description.clone(),
                        sort_order: p.sort_order,
                    },
                )
            })
            .collect();

        let tax = graph
            .scenario
            .tax_config_id
            .and_then(|id| graph.tax_configs.get(&id))
            .map(|entry| (entry.config.clone(), entry.brackets.clone()));
        match tax {
            Some((config, brackets)) => {
                graph.tax_config = Some(config);
                graph.tax_brackets = brackets;
            }
            None => {
                graph.scenario.tax_config_id = None;
                graph.tax_config = None;
                graph.tax_brackets = Vec::new();
            }
        }

        let inflation = graph
            .scenario
            .inflation_profile_id
            .and_then(|id| graph.inflation_profiles.get(&id))
            .map(|entry| (entry.name.clone(), entry.distribution_id));
        match inflation {
            Some((name, distribution_id)) => {
                graph.inflation_profile_name = Some(name);
                graph.inflation_distribution_id = Some(distribution_id);
            }
            None => {
                graph.scenario.inflation_profile_id = None;
                graph.inflation_profile_name = None;
                graph.inflation_distribution_id = None;
            }
        }
    }

    /// The library an attached graph carries: the inverse of
    /// [`attach`](Self::attach).
    ///
    /// Only a graph that was attached (or loaded from the database) has the
    /// whole library: `tax_configs` and `inflation_profiles` are not
    /// serialized with a graph, and a run snapshot keeps only the profiles it
    /// used.
    pub fn from_graph(graph: &ScenarioGraph) -> Library {
        let mut return_profiles: Vec<LibraryReturnProfile> = graph
            .return_profiles
            .values()
            .map(|p| LibraryReturnProfile {
                id: p.id,
                name: p.name.clone(),
                description: p.description.clone(),
                asset_class: p.asset_class.clone(),
                distribution_id: p.distribution_id,
                sort_order: p.sort_order,
            })
            .collect();
        return_profiles.sort_by_key(|p| p.id);

        let mut inflation_profiles: Vec<LibraryInflationProfile> = graph
            .inflation_profiles
            .iter()
            .map(|(id, p)| LibraryInflationProfile {
                id: *id,
                name: p.name.clone(),
                description: p.description.clone(),
                distribution_id: p.distribution_id,
                sort_order: p.sort_order,
            })
            .collect();
        inflation_profiles.sort_by_key(|p| p.id);

        let mut tax_configs: Vec<LibraryTaxConfig> = graph
            .tax_configs
            .values()
            .map(|t| LibraryTaxConfig {
                id: t.config.id,
                name: t.config.name.clone(),
                description: t.description.clone(),
                state_rate: t.config.state_rate,
                capital_gains_rate: t.config.capital_gains_rate,
                early_withdrawal_penalty_rate: t.config.early_withdrawal_penalty_rate,
                standard_deduction: t.config.standard_deduction,
                age_65_extra_deduction: t.config.age_65_extra_deduction,
                federal_brackets: t
                    .brackets
                    .iter()
                    .map(|b| Bracket {
                        threshold: b.threshold,
                        rate: b.rate,
                    })
                    .collect(),
            })
            .collect();
        tax_configs.sort_by_key(|t| t.id);

        let mut distributions: Vec<DistributionRow> =
            graph.distributions.values().cloned().collect();
        distributions.sort_by_key(|d| d.id);

        Library {
            return_profiles,
            inflation_profiles,
            tax_configs,
            distributions,
        }
    }

    /// Return profile ids as the list route orders them: `sort_order, name`.
    pub fn return_profile_order(&self) -> Vec<i64> {
        let mut rows: Vec<&LibraryReturnProfile> = self.return_profiles.iter().collect();
        rows.sort_by_key(|p| listed(p.sort_order, &p.name));
        rows.into_iter().map(|p| p.id).collect()
    }

    /// Inflation profile ids as the list route orders them: `sort_order, name`.
    pub fn inflation_profile_order(&self) -> Vec<i64> {
        let mut rows: Vec<&LibraryInflationProfile> = self.inflation_profiles.iter().collect();
        rows.sort_by_key(|p| listed(p.sort_order, &p.name));
        rows.into_iter().map(|p| p.id).collect()
    }

    /// Tax config ids as the list route orders them: by name.
    pub fn tax_config_order(&self) -> Vec<i64> {
        let mut rows: Vec<&LibraryTaxConfig> = self.tax_configs.iter().collect();
        rows.sort_by_key(|t| listed_by_name(&t.name));
        rows.into_iter().map(|t| t.id).collect()
    }

    /// Drop the distributions no profile reaches. Deleting or re-shaping a
    /// profile leaves its old distribution rows behind in the database (the
    /// route deletes only the root of a replaced one); a library kept as JSON
    /// has no reason to carry them, so [`apply_library`] sweeps them away. The
    /// plan's snapshot never sees them either way.
    pub fn prune_distributions(&mut self) {
        let mut reached: HashSet<i64> = HashSet::new();
        let mut pending: Vec<i64> = self
            .return_profiles
            .iter()
            .map(|p| p.distribution_id)
            .chain(self.inflation_profiles.iter().map(|p| p.distribution_id))
            .collect();
        while let Some(id) = pending.pop() {
            if reached.insert(id)
                && let Some(row) = self.distributions.iter().find(|d| d.id == id)
            {
                pending.extend(row.bull_id);
                pending.extend(row.bear_id);
            }
        }
        self.distributions.retain(|d| reached.contains(&d.id));
    }

    fn next_distribution_id(&self) -> i64 {
        self.distributions.iter().map(|d| d.id).max().unwrap_or(0) + 1
    }
}

// ── rows shared with the graph's own library edits ───────────────────────────

/// The `distributions` rows for `spec`, children first, numbered from
/// `first_id`; returns the root's id. What each row holds is
/// [`DistributionSpec::columns`], as for the route's insert.
pub(crate) fn distribution_rows(
    spec: &DistributionSpec,
    first_id: i64,
) -> PlanResult<(i64, Vec<DistributionRow>)> {
    let mut next = first_id;
    let mut rows = Vec::new();
    let root = push_distribution(spec, 0, &mut next, &mut rows)?;
    Ok((root, rows))
}

fn push_distribution(
    spec: &DistributionSpec,
    depth: usize,
    next: &mut i64,
    rows: &mut Vec<DistributionRow>,
) -> PlanResult<i64> {
    let columns = spec.columns(depth)?;
    let (bull_id, bear_id) = match columns.regimes {
        Some((bull, bear)) => (
            Some(push_distribution(bull, depth + 1, next, rows)?),
            Some(push_distribution(bear, depth + 1, next, rows)?),
        ),
        None => (None, None),
    };
    let id = *next;
    *next += 1;
    rows.push(columns.into_row(id, bull_id, bear_id));
    Ok(id)
}

/// A return profile and its distribution rows, as creating one writes them.
pub(crate) struct NewReturnProfile {
    pub profile: LibraryReturnProfile,
    pub distributions: Vec<DistributionRow>,
}

/// What `POST /return-profiles` does, short of the write: the checks (the
/// distribution, a name, a name not in `taken`) and the rows. A blank name is
/// refused here, which the route's SQL would let through.
pub(crate) fn new_return_profile<'a>(
    body: &CreateProfile,
    taken: impl IntoIterator<Item = &'a str>,
    id: i64,
    first_distribution_id: i64,
    sort_order: i64,
) -> PlanResult<NewReturnProfile> {
    body.distribution.validate(0)?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(PlanError::invalid("a return profile needs a name"));
    }
    if taken.into_iter().any(|n| n == name) {
        return Err(PlanError::Conflict(profiles::NAME_TAKEN.into()));
    }
    let (distribution_id, distributions) =
        distribution_rows(&body.distribution, first_distribution_id)?;
    Ok(NewReturnProfile {
        profile: LibraryReturnProfile {
            id,
            name: name.to_string(),
            description: body.description.clone(),
            asset_class: body.asset_class.map(|c| c.as_str().to_string()),
            distribution_id,
            sort_order,
        },
        distributions,
    })
}

/// What `POST /tax-configs` does, short of the write: the checks (rates,
/// deductions, brackets, a name, a name not in `taken`) and the row, brackets
/// ascending. A blank name is refused here, which the route's SQL would let
/// through.
pub(crate) fn new_tax_config<'a>(
    body: &CreateTaxConfig,
    taken: impl IntoIterator<Item = &'a str>,
    id: i64,
) -> PlanResult<LibraryTaxConfig> {
    let brackets = taxes::checked(body)?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(PlanError::invalid("a tax config needs a name"));
    }
    if taken.into_iter().any(|n| n == name) {
        return Err(PlanError::Conflict(taxes::NAME_TAKEN.into()));
    }
    Ok(LibraryTaxConfig {
        id,
        name: name.to_string(),
        description: body.description.clone(),
        state_rate: body.state_rate,
        capital_gains_rate: body.capital_gains_rate,
        early_withdrawal_penalty_rate: body.early_withdrawal_penalty_rate,
        standard_deduction: body.standard_deduction,
        age_65_extra_deduction: body.age_65_extra_deduction,
        federal_brackets: brackets,
    })
}

// ── edits ────────────────────────────────────────────────────────────────────

/// One write to the library, by name and body as the web calls the library
/// routes. JSON is `{"op": "update_tax_config", "id": 3, "body": {...}}`.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
#[ts(export)]
pub enum LibraryOp {
    /// `POST /return-profiles`
    CreateReturnProfile { body: CreateProfile },
    /// `PATCH /return-profiles/{id}`
    UpdateReturnProfile { id: i64, body: UpdateProfile },
    /// `DELETE /return-profiles/{id}`
    DeleteReturnProfile { id: i64 },
    /// `POST /return-profiles/reorder`
    ReorderReturnProfiles { ids: Vec<i64> },
    /// `POST /inflation-profiles`
    CreateInflationProfile { body: CreateProfile },
    /// `DELETE /inflation-profiles/{id}`
    DeleteInflationProfile { id: i64 },
    /// `POST /inflation-profiles/reorder`
    ReorderInflationProfiles { ids: Vec<i64> },
    /// `POST /tax-configs`
    CreateTaxConfig { body: CreateTaxConfig },
    /// `PATCH /tax-configs/{id}`
    UpdateTaxConfig { id: i64, body: UpdateTaxConfig },
    /// `DELETE /tax-configs/{id}`
    DeleteTaxConfig { id: i64 },
}

/// Apply `op` to `library`, as the route would to the user's rows, or change
/// nothing and say why. `plans` is every plan the user has: what a database
/// would see through its foreign keys and queries across scenarios, and where
/// a return profile that is still in use is found.
///
/// Returns the id of a row a `Create*` op made (the next above the library's
/// largest, as the database would number it).
///
/// What differs from the routes, all of it stricter: a blank name is refused
/// (the routes' SQL accepts one), a created or replaced distribution gets the
/// full distribution check, and an update's rates are checked as fractions
/// (the routes leave both to the tables' CHECKs).
pub fn apply_library(
    library: &mut Library,
    plans: &[ScenarioGraph],
    op: &LibraryOp,
) -> PlanResult<EditOutcome> {
    let mut staged = library.clone();
    let outcome = match op {
        LibraryOp::CreateReturnProfile { body } => create_return_profile(&mut staged, body)?,
        LibraryOp::UpdateReturnProfile { id, body } => {
            update_return_profile(&mut staged, *id, body)?;
            None
        }
        LibraryOp::DeleteReturnProfile { id } => {
            delete_return_profile(&mut staged, plans, *id)?;
            None
        }
        LibraryOp::ReorderReturnProfiles { ids } => {
            let order = staged.return_profile_order();
            renumber(&order, ids, |id, rank| {
                if let Some(p) = staged.return_profiles.iter_mut().find(|p| p.id == id) {
                    p.sort_order = rank;
                }
            });
            None
        }
        LibraryOp::CreateInflationProfile { body } => create_inflation_profile(&mut staged, body)?,
        LibraryOp::DeleteInflationProfile { id } => {
            let before = staged.inflation_profiles.len();
            staged.inflation_profiles.retain(|p| p.id != *id);
            if staged.inflation_profiles.len() == before {
                return Err(PlanError::NotFound("inflation profile"));
            }
            None
        }
        LibraryOp::ReorderInflationProfiles { ids } => {
            let order = staged.inflation_profile_order();
            renumber(&order, ids, |id, rank| {
                if let Some(p) = staged.inflation_profiles.iter_mut().find(|p| p.id == id) {
                    p.sort_order = rank;
                }
            });
            None
        }
        LibraryOp::CreateTaxConfig { body } => {
            let id = staged.tax_configs.iter().map(|t| t.id).max().unwrap_or(0) + 1;
            let config =
                new_tax_config(body, staged.tax_configs.iter().map(|t| t.name.as_str()), id)?;
            staged.tax_configs.push(config);
            Some(id)
        }
        LibraryOp::UpdateTaxConfig { id, body } => {
            update_tax_config(&mut staged, *id, body)?;
            None
        }
        LibraryOp::DeleteTaxConfig { id } => {
            let before = staged.tax_configs.len();
            staged.tax_configs.retain(|t| t.id != *id);
            if staged.tax_configs.len() == before {
                return Err(PlanError::NotFound("tax config"));
            }
            None
        }
    };
    staged.prune_distributions();
    *library = staged;
    Ok(EditOutcome { id: outcome })
}

/// [`apply_library`], and the plans brought along with it: every plan in
/// `plans` has the new library attached (so its copies of a changed profile or
/// tax config, and its references to a deleted one, follow), and a plan the
/// change touched gets `updated_at = now`, as the routes bump the scenarios a
/// changed profile or tax config reaches. Returns the outcome and the ids of
/// the plans that changed.
///
/// Atomic: on an error neither the library nor any plan is touched.
pub fn apply_library_to_plans(
    library: &mut Library,
    plans: &mut [ScenarioGraph],
    op: &LibraryOp,
    now: Option<&str>,
) -> PlanResult<(EditOutcome, Vec<i64>)> {
    let outcome = apply_library(library, plans, op)?;
    let mut changed = Vec::new();
    for plan in plans.iter_mut() {
        let before =
            serde_json::to_value(&*plan).map_err(|e| PlanError::internal(e.to_string()))?;
        library.attach(plan);
        let after = serde_json::to_value(&*plan).map_err(|e| PlanError::internal(e.to_string()))?;
        if before != after {
            if let Some(now) = now {
                plan.scenario.updated_at = now.to_string();
            }
            changed.push(plan.scenario.id);
        }
    }
    Ok((outcome, changed))
}

fn create_return_profile(library: &mut Library, body: &CreateProfile) -> PlanResult<Option<i64>> {
    let id = library
        .return_profiles
        .iter()
        .map(|p| p.id)
        .max()
        .unwrap_or(0)
        + 1;
    let sort_order = next_sort_order(library.return_profiles.iter().map(|p| p.sort_order));
    let new = new_return_profile(
        body,
        library.return_profiles.iter().map(|p| p.name.as_str()),
        id,
        library.next_distribution_id(),
        sort_order,
    )?;
    library.distributions.extend(new.distributions);
    library.return_profiles.push(new.profile);
    Ok(Some(id))
}

fn create_inflation_profile(
    library: &mut Library,
    body: &CreateProfile,
) -> PlanResult<Option<i64>> {
    check_inflation_kind(&body.distribution)?;
    body.distribution.validate(0)?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(PlanError::invalid("an inflation profile needs a name"));
    }
    if library.inflation_profiles.iter().any(|p| p.name == name) {
        return Err(PlanError::Conflict(profiles::INFLATION_NAME_TAKEN.into()));
    }
    let (distribution_id, rows) =
        distribution_rows(&body.distribution, library.next_distribution_id())?;
    let id = library
        .inflation_profiles
        .iter()
        .map(|p| p.id)
        .max()
        .unwrap_or(0)
        + 1;
    let sort_order = next_sort_order(library.inflation_profiles.iter().map(|p| p.sort_order));
    library.distributions.extend(rows);
    library.inflation_profiles.push(LibraryInflationProfile {
        id,
        name: name.to_string(),
        description: body.description.clone(),
        distribution_id,
        sort_order,
    });
    Ok(Some(id))
}

/// `COALESCE(MAX(sort_order), -1) + 1`: where a new row goes.
fn next_sort_order(existing: impl Iterator<Item = i64>) -> i64 {
    existing.max().map_or(0, |max| max + 1)
}

fn update_return_profile(library: &mut Library, id: i64, body: &UpdateProfile) -> PlanResult<()> {
    if !library.return_profiles.iter().any(|p| p.id == id) {
        return Err(PlanError::NotFound("return profile"));
    }
    if let Some(spec) = &body.distribution {
        spec.validate(0)?;
    }
    let name = body.name.as_deref().map(str::trim);
    if let Some(name) = name {
        if name.is_empty() {
            return Err(PlanError::invalid("a return profile needs a name"));
        }
        if library
            .return_profiles
            .iter()
            .any(|p| p.id != id && p.name == name)
        {
            return Err(PlanError::Conflict(profiles::NAME_TAKEN.into()));
        }
    }

    // A new distribution is added and swapped in, as the route does; the old
    // one is swept by `prune_distributions`.
    let new_root = match &body.distribution {
        Some(spec) => {
            let (root, rows) = distribution_rows(spec, library.next_distribution_id())?;
            library.distributions.extend(rows);
            Some(root)
        }
        None => None,
    };
    let row = library
        .return_profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or(PlanError::NotFound("return profile"))?;
    if let Some(name) = name {
        row.name = name.to_string();
    }
    if let Some(description) = &body.description {
        row.description = Some(description.clone());
    }
    // `Some(None)` is a deliberate unclassify, `None` is silence about the class.
    if let Some(class) = body.asset_class {
        row.asset_class = class.map(AssetClass::as_str).map(str::to_string);
    }
    if let Some(root) = new_root {
        row.distribution_id = root;
    }
    Ok(())
}

fn delete_return_profile(
    library: &mut Library,
    plans: &[ScenarioGraph],
    id: i64,
) -> PlanResult<()> {
    let used = crate::read::profile_users_across(plans, id);
    if !used.is_empty() {
        return Err(PlanError::Conflict(format!(
            "return profile is still used by: {}",
            used.join(", ")
        )));
    }
    let before = library.return_profiles.len();
    library.return_profiles.retain(|p| p.id != id);
    if library.return_profiles.len() == before {
        return Err(PlanError::NotFound("return profile"));
    }
    Ok(())
}

fn update_tax_config(library: &mut Library, id: i64, body: &UpdateTaxConfig) -> PlanResult<()> {
    if !library.tax_configs.iter().any(|t| t.id == id) {
        return Err(PlanError::NotFound("tax config"));
    }
    taxes::validate_deductions([
        ("standard_deduction", body.standard_deduction),
        ("age_65_extra_deduction", body.age_65_extra_deduction),
    ])?;
    taxes::validate_rates([
        ("state_rate", body.state_rate),
        ("capital_gains_rate", body.capital_gains_rate),
        (
            "early_withdrawal_penalty_rate",
            body.early_withdrawal_penalty_rate,
        ),
    ])?;
    let brackets = body
        .federal_brackets
        .as_deref()
        .map(taxes::validate_brackets)
        .transpose()?;
    let name = body.name.as_deref().map(str::trim);
    if let Some(name) = name {
        if name.is_empty() {
            return Err(PlanError::invalid("a tax config needs a name"));
        }
        if library
            .tax_configs
            .iter()
            .any(|t| t.id != id && t.name == name)
        {
            return Err(PlanError::Conflict(taxes::NAME_TAKEN.into()));
        }
    }

    let row = library
        .tax_configs
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or(PlanError::NotFound("tax config"))?;
    if let Some(name) = name {
        row.name = name.to_string();
    }
    if let Some(description) = &body.description {
        row.description = Some(description.clone());
    }
    if let Some(rate) = body.state_rate {
        row.state_rate = rate;
    }
    if let Some(rate) = body.capital_gains_rate {
        row.capital_gains_rate = rate;
    }
    if let Some(rate) = body.early_withdrawal_penalty_rate {
        row.early_withdrawal_penalty_rate = rate;
    }
    if let Some(amount) = body.standard_deduction {
        row.standard_deduction = amount;
    }
    if let Some(amount) = body.age_65_extra_deduction {
        row.age_65_extra_deduction = amount;
    }
    if let Some(brackets) = brackets {
        row.federal_brackets = brackets;
    }
    Ok(())
}

// ── the starter library ──────────────────────────────────────────────────────

/// The starter library every account is given at registration: a small set of
/// market assumptions and a default tax table. They are ordinary rows the user
/// can edit or delete. Ids are 1, 2, … within each list; `sort_order` is zero
/// throughout, so they list alphabetically, as the database's default column
/// does.
///
/// The figures are the long-run historical constants the engine ships with, in
/// `finplan_core::model::market`.
///
/// A return profile's class is what a ticker's asset class is matched against,
/// so it is what lets the web client map `VTI` onto the first of these without
/// reading its name. Two carry none: "Savings Account" describes a bank
/// balance rather than a holding, and "No Growth" is the absence of an
/// assumption — neither should ever be what a ticker resolves to.
pub fn seed() -> Library {
    let mut library = Library::default();

    // (name, description, kind, mean or rate, std_dev, asset class)
    type Seed = (
        &'static str,
        &'static str,
        &'static str,
        f64,
        f64,
        Option<AssetClass>,
    );
    const RETURN_PROFILES: &[Seed] = &[
        (
            "US Total Market",
            "S&P 500, 1928-2024: 9.9% mean, 19.6% sd",
            "Normal",
            0.0990829,
            0.1962,
            Some(AssetClass::UsEquity),
        ),
        (
            "US Small Cap",
            "US small cap, 1928-2024: 11.2% mean",
            "Normal",
            0.112028,
            0.2988,
            Some(AssetClass::UsSmallCap),
        ),
        (
            "US Aggregate Bonds",
            "US aggregate bonds: 3.0% mean",
            "Normal",
            0.0301011,
            0.0574,
            Some(AssetClass::Bonds),
        ),
        (
            "International Developed",
            "Developed ex-US equities: 6.0% mean",
            "Normal",
            0.0602527,
            0.2251,
            Some(AssetClass::IntlEquity),
        ),
        (
            "REITs",
            "US real estate investment trusts: 6.4% mean",
            "Normal",
            0.0642145,
            0.1959,
            Some(AssetClass::Reit),
        ),
        (
            "Cash / T-Bills",
            "US Treasury bills: 3.4% mean",
            "Normal",
            0.0337398,
            0.0308,
            Some(AssetClass::Cash),
        ),
        (
            "Savings Account",
            "A flat 2% nominal yield",
            "Fixed",
            0.02,
            0.0,
            None,
        ),
        ("No Growth", "Holds nominal value", "None", 0.0, 0.0, None),
    ];

    let distribution = |id: i64, kind: &str, mean: f64, std_dev: f64| {
        let (rate, mean, std_dev) = match kind {
            "Fixed" => (Some(mean), None, None),
            "Normal" => (None, Some(mean), Some(std_dev)),
            _ => (None, None, None),
        };
        DistributionRow {
            id,
            kind: kind.to_string(),
            rate,
            mean,
            std_dev,
            scale: None,
            df: None,
            bull_id: None,
            bear_id: None,
            bull_to_bear_prob: None,
            bear_to_bull_prob: None,
            history_preset: None,
            block_size: None,
        }
    };

    for (name, description, kind, mean, std_dev, asset_class) in RETURN_PROFILES {
        let id = library.distributions.len() as i64 + 1;
        library
            .distributions
            .push(distribution(id, kind, *mean, *std_dev));
        library.return_profiles.push(LibraryReturnProfile {
            id,
            name: (*name).to_string(),
            description: Some((*description).to_string()),
            asset_class: asset_class.map(|c| c.as_str().to_string()),
            distribution_id: id,
            sort_order: 0,
        });
    }

    // Inflation: a fixed long-run rate, and a stochastic alternative.
    for (name, description, kind, mean, std_dev) in [
        (
            "US Historical (fixed)",
            "CPI-U 1948-2025 geometric mean, 3.43%",
            "Fixed",
            0.0343436,
            0.0,
        ),
        (
            "US Historical (stochastic)",
            "CPI-U 1948-2025: 3.47% mean, 2.79% sd",
            "Normal",
            0.0347068,
            0.0279436,
        ),
    ] {
        let distribution_id = library.distributions.len() as i64 + 1;
        library
            .distributions
            .push(distribution(distribution_id, kind, mean, std_dev));
        library.inflation_profiles.push(LibraryInflationProfile {
            id: library.inflation_profiles.len() as i64 + 1,
            name: name.to_string(),
            description: Some(description.to_string()),
            distribution_id,
            sort_order: 0,
        });
    }

    // 2024 US federal brackets, single filer.
    library.tax_configs.push(LibraryTaxConfig {
        id: 1,
        name: "US Federal 2024 (single)".to_string(),
        description: Some(
            "2024 federal brackets and standard deduction, 5% state, 15% LTCG".to_string(),
        ),
        state_rate: 0.05,
        capital_gains_rate: 0.15,
        early_withdrawal_penalty_rate: 0.10,
        standard_deduction: 14_600.0,
        age_65_extra_deduction: 1_950.0,
        federal_brackets: [
            (0.0, 0.10),
            (11_600.0, 0.12),
            (47_150.0, 0.22),
            (100_525.0, 0.24),
            (191_950.0, 0.32),
            (243_725.0, 0.35),
            (609_350.0, 0.37),
        ]
        .into_iter()
        .map(|(threshold, rate)| Bracket { threshold, rate })
        .collect(),
    });

    library
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
