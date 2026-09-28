//! Structured edits to a saved plan, as a suggestion carries them.
//!
//! A [`Change`] is one RFC 6902-style operation (`replace`, `add`, `remove`)
//! at an RFC 6901 pointer into one resource's body — the *same* body its GET
//! route returns, so whoever wrote the change (usually a model that just read
//! those routes) writes in the vocabulary it read. `expect` carries the value
//! being replaced: it is how a suggestion made against an older plan is caught
//! as stale, and what the "−" side of its diff shows.
//!
//! [`resolve`] is pure over a [`ScenarioGraph`] — a live load or a run's
//! snapshot — and does no I/O. It groups changes by target, reads each
//! target's current body, applies the changes in order, checks `expect`, and
//! lowers the result onto the typed bodies the write routes take
//! ([`ResolvedChange`]). Writing those, to SQL or into a graph, is someone
//! else's job; so is compiling the result.
//!
//! GET shape -> write body, per target:
//!
//! | target    | patched body (GET)            | lowered to                                    |
//! |-----------|-------------------------------|-----------------------------------------------|
//! | event     | `events::Event`               | `EventBody` (PUT: whole event)                |
//! | new_event | `EventBody` (the `add` value) | `EventBody` (POST)                            |
//! | new_asset | `assets::Asset` minus `id`    | `CreateAsset` (POST)                          |
//! | new_account | `accounts::Account` minus   | `CreateAccount` (POST) + a `CreatePosition`   |
//! |           | `id`, lots without ids        | per lot                                       |
//! | asset     | `assets::Asset`               | `UpdateAsset` (PATCH: changed fields only)    |
//! | account   | `accounts::Account`           | `UpdateAccount` (PATCH: changed top-level     |
//! |           |                               | fields; the whole flavor if any of it moved)  |
//! |           | `…/positions/*`               | `CreatePosition`/`UpdatePosition`/delete, by  |
//! |           |                               | position `id`                                 |
//! | new_parameter | `ParameterBody` (the `add` value) | `ParameterBody` (POST)                    |
//! | parameter | `id` + `ParameterBody`        | `ParameterBody` (PATCH: whole parameter)      |
//! | scenario  | the scenario's settings       | `UpdateScenario` (PATCH: changed fields only) |
//! | new_return_profile | `CreateProfile` (the `add` value) | `CreateProfile` (POST, user-level) |
//! | new_tax_config | `CreateTaxConfig` (the `add` value) | `CreateTaxConfig` (POST, user-level) |
//!
//! `id` fields are read-only, as is an account's `flavor` tag and a scenario's
//! `name` and `start_date` (the server owns a plan's start date). `remove` at
//! the root deletes the resource, except for the scenario, which cannot go.
//!
//! The two user-level kinds are rows of the caller's libraries rather than of
//! the plan: they are written when the batch is applied, and are referred to
//! by `{"$new": key}` from `return_profile_id` / `cash_return_profile_id` (and
//! a scenario's `tax_config_id`) like anything else a batch creates.
//!
//! Entities the batch creates are named by a key (`{"new_asset": "vti"}`), and
//! any id field of any body in the batch may point at one with
//! `{"$new": "vti"}` instead of a number. [`resolve`] checks those references
//! and validates each body with a temporary id standing in; [`apply`] writes the
//! batch in dependency order, substituting each created row's real id as it is
//! returned.

pub mod ai;
mod apply;
mod diff;
mod pointer;
mod read;
pub mod rules;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::api::accounts::{
    CreateAccount, CreatePosition, FlavorSpec, UpdateAccount, UpdatePosition,
};
use crate::api::assets::{CreateAsset, UpdateAsset};
use crate::api::events::EventBody;
use crate::api::parameters::ParameterBody;
use crate::api::profiles::CreateProfile;
use crate::api::scenarios::UpdateScenario;
use crate::api::taxes::CreateTaxConfig;
use crate::compile::rows::ScenarioGraph;

pub use apply::{
    Step, StepProblems, Stepped, apply_steps_sql, apply_to_graph, apply_to_sql, assumptions_named,
    plan_problem, profiles_named, resolve_steps,
};
pub use diff::Names;

// ── wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ChangeOp {
    Replace,
    Add,
    Remove,
}

/// The resource a change edits. The `new_*` targets name a resource the same
/// batch creates: its first change must be `add` at `""` with the whole body,
/// and later changes sharing the key patch that body. Keys are unique across
/// all the kinds, and `{"$new": "<key>"}` in any id field of the batch refers
/// to the resource created under that key.
///
/// `scenario` is the plan's own settings (birth date, duration, inflation
/// profile, tax config). `new_return_profile` and `new_tax_config` create rows
/// in the caller's libraries, which no later batch can edit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChangeTarget {
    Event(i64),
    Asset(i64),
    Account(i64),
    Parameter(i64),
    NewEvent(String),
    NewAsset(String),
    NewAccount(String),
    NewParameter(String),
    Scenario,
    NewReturnProfile(String),
    NewTaxConfig(String),
}

/// What a `$new` key names, or an id field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RefKind {
    Event,
    Asset,
    Account,
    Parameter,
    ReturnProfile,
    TaxConfig,
}

impl ChangeTarget {
    /// The key and kind of a resource the batch creates.
    fn created(&self) -> Option<(&str, RefKind)> {
        match self {
            ChangeTarget::NewEvent(key) => Some((key, RefKind::Event)),
            ChangeTarget::NewAsset(key) => Some((key, RefKind::Asset)),
            ChangeTarget::NewAccount(key) => Some((key, RefKind::Account)),
            ChangeTarget::NewParameter(key) => Some((key, RefKind::Parameter)),
            ChangeTarget::NewReturnProfile(key) => Some((key, RefKind::ReturnProfile)),
            ChangeTarget::NewTaxConfig(key) => Some((key, RefKind::TaxConfig)),
            _ => None,
        }
    }
}

/// One edit to one resource.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Change {
    pub op: ChangeOp,
    pub target: ChangeTarget,
    /// RFC 6901 pointer into the target's GET body; `""` is the whole resource.
    #[serde(default)]
    pub path: String,
    /// The value currently at `path`, as the author read it. Absent skips the
    /// staleness check; an explicit `null` expects null.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "unknown")]
    pub expect: Option<Value>,
    /// The new value for `replace` and `add`; an explicit `null` writes null.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "unknown")]
    pub value: Option<Value>,
}

/// A field that is present — even as `null` — deserializes to `Some`.
fn present<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(de).map(Some)
}

/// Why a batch of changes cannot be applied. `change` indexes the request's
/// `changes` array.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum ChangeProblem {
    /// `expect` does not match the plan: it changed since the author read it.
    Stale {
        change: usize,
        path: String,
        // Boxed: two inline `Value`s make every `Result<_, ChangeProblem>`
        // carry 176 bytes on the happy path too.
        #[ts(type = "unknown")]
        expected: Box<Value>,
        #[ts(type = "unknown")]
        actual: Box<Value>,
    },
    /// The path is malformed, read-only, or names nothing.
    BadPath {
        change: usize,
        path: String,
        reason: String,
    },
    /// The patched body no longer describes a valid resource.
    InvalidBody {
        change: usize,
        target: ChangeTarget,
        message: String,
    },
    UnknownTarget {
        change: usize,
        target: ChangeTarget,
    },
    UnsupportedOp {
        change: usize,
        reason: String,
    },
    /// Two resources the batch creates share a key.
    DuplicateKey {
        change: usize,
        key: String,
    },
    /// `{"$new": key}` names nothing the batch creates.
    UnknownReference {
        change: usize,
        key: String,
    },
    /// `{"$new": key}` sits in a field that holds another kind of id, or no id
    /// at all (`expected` is then null).
    WrongReferenceKind {
        change: usize,
        key: String,
        field: String,
        expected: Option<RefKind>,
        found: RefKind,
    },
    /// Resources the batch creates refer to each other in a loop, so none can
    /// be written first.
    ReferenceCycle {
        change: usize,
        keys: Vec<String>,
    },
}

/// One line of a server-rendered diff: `from` is absent for an addition, `to`
/// for a removal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiffLine {
    pub label: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

// ── resolution ──────────────────────────────────────────────────────────────

/// A change batch lowered onto the typed write bodies, one entry per write, in
/// the order the batch first named each target.
///
/// Bodies that refer to resources the batch creates carry a temporary negative
/// id where the reference was; [`apply`] substitutes the real ones.
#[derive(Debug)]
pub enum ResolvedChange {
    CreateEvent {
        key: String,
        body: EventBody,
    },
    CreateAsset {
        key: String,
        body: CreateAsset,
    },
    CreateAccount {
        key: String,
        body: CreateAccount,
        /// Lots opened in the new account, once it exists.
        positions: Vec<CreatePosition>,
    },
    ReplaceEvent {
        id: i64,
        body: EventBody,
    },
    DeleteEvent {
        id: i64,
    },
    UpdateAsset {
        id: i64,
        body: UpdateAsset,
    },
    DeleteAsset {
        id: i64,
    },
    UpdateAccount {
        id: i64,
        body: UpdateAccount,
    },
    DeleteAccount {
        id: i64,
    },
    CreateParameter {
        key: String,
        body: ParameterBody,
    },
    ReplaceParameter {
        id: i64,
        body: ParameterBody,
    },
    DeleteParameter {
        id: i64,
    },
    UpdateScenario {
        body: UpdateScenario,
    },
    /// A row of the caller's return profile library.
    CreateReturnProfile {
        key: String,
        body: CreateProfile,
    },
    /// A row of the caller's tax config library.
    CreateTaxConfig {
        key: String,
        body: CreateTaxConfig,
    },
    CreatePosition {
        account_id: i64,
        body: CreatePosition,
    },
    UpdatePosition {
        account_id: i64,
        position_id: i64,
        body: UpdatePosition,
    },
    DeletePosition {
        account_id: i64,
        position_id: i64,
    },
}

/// A target's body before and after the batch; `None` is "does not exist".
/// `after` keeps `{"$new": key}` references as written.
#[derive(Debug, Clone)]
struct Delta {
    target: ChangeTarget,
    before: Option<Value>,
    after: Option<Value>,
    /// The last change naming the target: the one that left its body in the
    /// state the writes describe, which a refusal is attributed to.
    last: usize,
}

#[derive(Debug)]
pub struct Resolved {
    /// Every write, in the order the batch first named each target, with
    /// temporary ids standing in for references.
    pub changes: Vec<ResolvedChange>,
    deltas: Vec<Delta>,
    /// Indices into `deltas` in the order they must be written.
    order: Vec<usize>,
    /// Names of what earlier batches created, for the diff.
    names: HashMap<String, String>,
}

impl Resolved {
    /// The diff to show for this batch. `names` resolves ids to labels; a
    /// return profile it does not know renders as `profile #id`.
    pub fn diff(&self, names: &Names) -> Vec<DiffLine> {
        // A reference reads as the name the batch gives the new resource.
        let created: HashMap<String, String> = self
            .deltas
            .iter()
            .filter_map(|d| {
                let (key, _) = d.target.created()?;
                let name = d.after.as_ref()?.get("name")?.as_str()?;
                Some((key.to_string(), name.trim().to_string()))
            })
            .chain(self.names.clone())
            .collect();
        let shown = |v: &Option<Value>| {
            v.as_ref().map(|v| {
                substitute(v, &|key| {
                    Some(Value::String(
                        created.get(key).cloned().unwrap_or_else(|| key.to_string()),
                    ))
                })
            })
        };
        self.deltas
            .iter()
            .map(|delta| Delta {
                before: shown(&delta.before),
                after: shown(&delta.after),
                ..delta.clone()
            })
            .flat_map(|delta| diff::lines(&delta, names))
            .collect()
    }

    /// See [`referenced_profiles`].
    pub fn referenced_profiles(&self) -> BTreeSet<i64> {
        referenced_profiles(&self.changes)
    }
}

/// Every return profile id the resolved writes point at.
///
/// A run snapshot prunes profiles nothing used, so a change mapping an asset
/// onto a new profile names one the snapshot lacks. `resolve` does not treat
/// that as an error; the caller loads whichever of these the graph is missing
/// before compiling.
pub fn referenced_profiles(changes: &[ResolvedChange]) -> BTreeSet<i64> {
    let mut ids = BTreeSet::new();
    let mut flavor = |flavor: &FlavorSpec| match flavor {
        FlavorSpec::Bank {
            return_profile_id, ..
        } => {
            ids.insert(*return_profile_id);
        }
        FlavorSpec::Investment {
            cash_return_profile_id,
            ..
        } => {
            ids.insert(*cash_return_profile_id);
        }
        _ => {}
    };
    let mut assets = BTreeSet::new();
    for change in changes {
        match change {
            ResolvedChange::UpdateAsset { body, .. } => {
                assets.extend(body.return_profile_id.flatten());
            }
            ResolvedChange::CreateAsset { body, .. } => assets.extend(body.return_profile_id),
            ResolvedChange::UpdateAccount { body, .. } => {
                if let Some(spec) = &body.flavor {
                    flavor(spec);
                }
            }
            ResolvedChange::CreateAccount { body, .. } => flavor(&body.flavor),
            _ => {}
        }
    }
    ids.extend(assets);
    ids
}

/// Resources an earlier batch of the same suggestion already created, by key:
/// what `{"$new": key}` and a `new_*` target with that key refer to in a later
/// batch. Ids are the rows' real ids in whichever plan they were written to.
pub type Created = BTreeMap<String, CreatedRef>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreatedRef {
    pub kind: RefKind,
    pub id: i64,
}

/// Apply `changes` to the bodies read from `graph`.
///
/// All or nothing: every problem found is returned, and a target stops
/// processing at its first problem, since later changes to it would be read
/// against a body the author did not expect.
pub fn resolve(graph: &ScenarioGraph, changes: &[Change]) -> Result<Resolved, Vec<ChangeProblem>> {
    resolve_with(graph, changes, &Created::new())
}

/// [`resolve`], for a batch that follows earlier ones: `seeded` holds what
/// they created, as ids in `graph`. A `new_*` target with a seeded key edits
/// that resource, and a `{"$new": key}` naming one refers to it.
pub fn resolve_with(
    graph: &ScenarioGraph,
    changes: &[Change],
    seeded: &Created,
) -> Result<Resolved, Vec<ChangeProblem>> {
    let mut order: Vec<ChangeTarget> = Vec::new();
    let mut seen = HashSet::new();
    for change in changes {
        if seen.insert(change.target.clone()) {
            order.push(change.target.clone());
        }
    }

    let mut problems = Vec::new();
    let mut deltas: Vec<Delta> = Vec::new();
    // Keys this batch creates, in the order it names them.
    let mut keys: Vec<(String, RefKind)> = Vec::new();

    for target in order {
        let indices: Vec<usize> = (0..changes.len())
            .filter(|i| changes[*i].target == target)
            .collect();
        let first = indices[0];
        let last = *indices.last().expect("a target has at least one change");
        // What the target is in `graph`: an existing row, or nothing yet.
        let existing = match existing_id(&target, seeded) {
            Ok(existing) => existing,
            Err(()) => {
                problems.push(ChangeProblem::UnknownTarget {
                    change: first,
                    target,
                });
                continue;
            }
        };
        let before = if target == ChangeTarget::Scenario {
            Some(read::scenario(graph))
        } else {
            existing.and_then(|(kind, id)| match kind {
                RefKind::Event => read::event(graph, id),
                RefKind::Asset => read::asset(graph, id),
                RefKind::Account => read::account(graph, id),
                RefKind::Parameter => read::parameter(graph, id),
                // Library rows are not part of the plan: a later batch cannot
                // edit what an earlier one created there.
                RefKind::ReturnProfile | RefKind::TaxConfig => None,
            })
        };
        if existing.is_some() && before.is_none() {
            problems.push(ChangeProblem::UnknownTarget {
                change: first,
                target,
            });
            continue;
        }
        if existing.is_none()
            && let Some((key, kind)) = target.created()
        {
            if keys.iter().any(|(k, _)| k == key) {
                problems.push(ChangeProblem::DuplicateKey {
                    change: first,
                    key: key.to_string(),
                });
                continue;
            }
            keys.push((key.to_string(), kind));
        }

        match patch(&target, before.clone(), changes, &indices) {
            Ok(after) => deltas.push(Delta {
                target,
                before,
                after,
                last,
            }),
            Err(problem) => problems.push(problem),
        }
    }

    // Keys whose resource survives the batch; a reference to one created and
    // then removed again names nothing.
    let live_keys: HashMap<&str, RefKind> = keys
        .iter()
        .filter(|(key, _)| {
            deltas
                .iter()
                .any(|d| d.target.created().is_some_and(|(k, _)| k == key) && d.after.is_some())
        })
        .map(|(key, kind)| (key.as_str(), *kind))
        .collect();
    let temporary: HashMap<&str, i64> = keys
        .iter()
        .enumerate()
        .map(|(i, (key, _))| (key.as_str(), -(i as i64) - 1))
        .collect();

    // Check every reference, then lower each target with temporary ids
    // standing in for what the batch creates.
    let creates_holdings = deltas.iter().any(|d| {
        d.after.is_some()
            && existing_id(&d.target, seeded) == Ok(None)
            && matches!(
                d.target,
                ChangeTarget::NewAsset(_) | ChangeTarget::NewAccount(_)
            )
    });
    // Expressions may name a parameter the batch creates (`$cash_floor`): they
    // are checked against the plan with those parameters added.
    let expression_graph = with_new_parameters(graph, &deltas, seeded);
    let mut resolved = Resolved {
        changes: Vec::new(),
        deltas: Vec::new(),
        order: Vec::new(),
        names: seeded
            .iter()
            .filter_map(|(key, created)| {
                let name = match created.kind {
                    RefKind::Event => graph
                        .events
                        .iter()
                        .find(|e| e.id == created.id)?
                        .name
                        .clone(),
                    RefKind::Asset => graph
                        .assets
                        .iter()
                        .find(|a| a.id == created.id)?
                        .name
                        .clone(),
                    RefKind::Account => graph
                        .accounts
                        .iter()
                        .find(|a| a.id == created.id)?
                        .name
                        .clone(),
                    RefKind::Parameter => graph
                        .parameters
                        .iter()
                        .find(|p| p.id == created.id)?
                        .name
                        .clone(),
                    RefKind::ReturnProfile => graph.return_profiles.get(&created.id)?.name.clone(),
                    RefKind::TaxConfig => graph.tax_configs.get(&created.id)?.config.name.clone(),
                };
                Some((key.clone(), name))
            })
            .collect(),
    };
    let mut depends: Vec<Vec<String>> = Vec::new();
    for delta in deltas {
        let mut refs = Vec::new();
        if let Some(after) = &delta.after {
            collect_refs(after, "", &mut refs);
        }
        let mut ok = true;
        let mut needs = Vec::new();
        for found in &refs {
            let change = origin(changes, &delta.target, &found.key);
            let problem = match found.check() {
                Err(()) => Some(ChangeProblem::UnknownReference {
                    change,
                    key: found.key.clone(),
                }),
                Ok(field_kind) => {
                    let actual = live_keys
                        .get(found.key.as_str())
                        .copied()
                        .or_else(|| seeded.get(&found.key).map(|c| c.kind));
                    match actual {
                        None => Some(ChangeProblem::UnknownReference {
                            change,
                            key: found.key.clone(),
                        }),
                        Some(kind) if Some(kind) != field_kind => {
                            Some(ChangeProblem::WrongReferenceKind {
                                change,
                                key: found.key.clone(),
                                field: found.field.clone(),
                                expected: field_kind,
                                found: kind,
                            })
                        }
                        Some(_) => None,
                    }
                }
            };
            match problem {
                Some(problem) => {
                    problems.push(problem);
                    ok = false;
                }
                None => {
                    if live_keys.contains_key(found.key.as_str()) {
                        needs.push(found.key.clone());
                    }
                }
            }
        }
        if !ok {
            continue;
        }

        let stand_in = |key: &str| {
            temporary
                .get(key)
                .copied()
                .or_else(|| seeded.get(key).map(|c| c.id))
                .map(Value::from)
        };
        let after = delta.after.as_ref().map(|a| substitute(a, &stand_in));
        let target = effective_target(&delta.target, seeded);
        match lower(
            &expression_graph,
            &target,
            delta.before.as_ref(),
            after.as_ref(),
            !creates_holdings,
        ) {
            Ok(writes) => {
                if !writes.is_empty() {
                    resolved.changes.extend(writes);
                    resolved.deltas.push(delta);
                    depends.push(needs);
                }
            }
            Err(message) => problems.push(ChangeProblem::InvalidBody {
                change: delta.last,
                target: delta.target,
                message,
            }),
        }
    }

    if problems.is_empty() {
        match write_order(&resolved.deltas, &depends, seeded) {
            Ok(order) => resolved.order = order,
            Err(problem) => problems.push(problem),
        }
    }
    if problems.is_empty() {
        Ok(resolved)
    } else {
        Err(problems)
    }
}

/// `graph`, with the parameters the batch creates added, for checking the
/// expressions that may name them. Borrowed unchanged when it creates none.
fn with_new_parameters<'a>(
    graph: &'a ScenarioGraph,
    deltas: &[Delta],
    seeded: &Created,
) -> std::borrow::Cow<'a, ScenarioGraph> {
    let mut with = std::borrow::Cow::Borrowed(graph);
    for delta in deltas {
        let (ChangeTarget::NewParameter(_), Some(after)) = (&delta.target, &delta.after) else {
            continue;
        };
        if existing_id(&delta.target, seeded) != Ok(None) {
            continue;
        }
        // A body that does not lower is reported by its own delta.
        if let Ok(body) = serde_json::from_value::<ParameterBody>(project(after, |k| k != "id")) {
            let _ = crate::domain::edit::create_parameter(with.to_mut(), &body);
        }
    }
    with
}

/// The existing row a target edits, if any: `Ok(None)` for a resource the
/// batch creates, `Err` for a `new_*` key an earlier batch created as another
/// kind.
fn existing_id(target: &ChangeTarget, seeded: &Created) -> Result<Option<(RefKind, i64)>, ()> {
    Ok(match target {
        ChangeTarget::Event(id) => Some((RefKind::Event, *id)),
        ChangeTarget::Asset(id) => Some((RefKind::Asset, *id)),
        ChangeTarget::Account(id) => Some((RefKind::Account, *id)),
        ChangeTarget::Parameter(id) => Some((RefKind::Parameter, *id)),
        ChangeTarget::Scenario => None,
        _ => {
            let (key, kind) = target.created().expect("the new_* targets");
            match seeded.get(key) {
                Some(created) if created.kind == kind => Some((kind, created.id)),
                Some(_) => return Err(()),
                None => None,
            }
        }
    })
}

/// The target as the writes see it: a `new_*` key an earlier batch created is
/// that row.
fn effective_target(target: &ChangeTarget, created: &Created) -> ChangeTarget {
    match existing_id(target, created) {
        Ok(Some((RefKind::Event, id))) => ChangeTarget::Event(id),
        Ok(Some((RefKind::Asset, id))) => ChangeTarget::Asset(id),
        Ok(Some((RefKind::Account, id))) => ChangeTarget::Account(id),
        Ok(Some((RefKind::Parameter, id))) => ChangeTarget::Parameter(id),
        _ => target.clone(),
    }
}

/// One `{"$new": key}` in a body, and the field it sits in.
struct FoundRef {
    key: String,
    field: String,
    well_formed: bool,
}

impl FoundRef {
    /// The kind of id its field holds (`None`: not an id field), or `Err`
    /// for a malformed reference.
    fn check(&self) -> Result<Option<RefKind>, ()> {
        if !self.well_formed {
            return Err(());
        }
        Ok(field_kind(&self.field))
    }
}

/// The kind of id a field holds, by name — the spec types name every id field
/// after what it points at.
fn field_kind(field: &str) -> Option<RefKind> {
    if field == "asset_id" {
        Some(RefKind::Asset)
    } else if field == "parameter_id" {
        Some(RefKind::Parameter)
    } else if field == "tax_config_id" {
        Some(RefKind::TaxConfig)
    } else if field.ends_with("return_profile_id") {
        Some(RefKind::ReturnProfile)
    } else if field.ends_with("account_id") || field == "exclude_accounts" {
        Some(RefKind::Account)
    } else if field.ends_with("event_id") {
        Some(RefKind::Event)
    } else {
        None
    }
}

const REF: &str = "$new";

/// The key an object names if it is a reference: exactly `{"$new": "<key>"}`.
fn ref_key(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    if object.len() != 1 {
        return None;
    }
    object.get(REF)?.as_str()
}

fn collect_refs(value: &Value, field: &str, out: &mut Vec<FoundRef>) {
    match value {
        Value::Object(object) if object.contains_key(REF) => out.push(FoundRef {
            key: match &object[REF] {
                Value::String(key) => key.clone(),
                other => other.to_string(),
            },
            field: field.to_string(),
            well_formed: ref_key(value).is_some(),
        }),
        Value::Object(object) => {
            for (key, child) in object {
                collect_refs(child, key, out);
            }
        }
        // A list's items sit in the list's field (`exclude_accounts`).
        Value::Array(items) => {
            for item in items {
                collect_refs(item, field, out);
            }
        }
        _ => {}
    }
}

/// `value` with every reference replaced by what `by_key` gives for its key;
/// a key it has nothing for is left as written.
fn substitute(value: &Value, by_key: &impl Fn(&str) -> Option<Value>) -> Value {
    if let Some(key) = ref_key(value)
        && let Some(replacement) = by_key(key)
    {
        return replacement;
    }
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(k, v)| (k.clone(), substitute(v, by_key)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|v| substitute(v, by_key)).collect()),
        other => other.clone(),
    }
}

/// The change that wrote a reference: the first of the target's changes whose
/// value holds it, else the target's last change.
fn origin(changes: &[Change], target: &ChangeTarget, key: &str) -> usize {
    let holds = |value: &Value| {
        let mut refs = Vec::new();
        collect_refs(value, "", &mut refs);
        refs.iter().any(|r| r.key == key)
    };
    let mine: Vec<usize> = (0..changes.len())
        .filter(|i| &changes[*i].target == target)
        .collect();
    mine.iter()
        .copied()
        .find(|i| changes[*i].value.as_ref().is_some_and(holds))
        .or(mine.last().copied())
        .unwrap_or_default()
}

/// Stage of a write: what it may refer to has to exist before it runs.
fn stage(delta: &Delta, seeded: &Created) -> u8 {
    let creates = existing_id(&delta.target, seeded) == Ok(None);
    match (&delta.target, &delta.after) {
        (ChangeTarget::NewReturnProfile(_) | ChangeTarget::NewTaxConfig(_), Some(_)) if creates => {
            0
        }
        (ChangeTarget::NewParameter(_), Some(_)) if creates => 1,
        (ChangeTarget::NewAsset(_), Some(_)) if creates => 2,
        (ChangeTarget::NewAccount(_), Some(_)) if creates => 3,
        (_, None) => 7,
        (ChangeTarget::NewEvent(_), Some(_)) if creates => 5,
        (ChangeTarget::Event(_) | ChangeTarget::NewEvent(_), Some(_)) => 6,
        _ => 4,
    }
}

/// The order to write the deltas in: by stage — new library rows (return
/// profiles, tax configs), new parameters, new assets, new accounts, edits to
/// assets, accounts, parameters and the scenario, new events, edits to events,
/// deletes — and within a stage, the batch's order except that a new resource
/// follows the new resources it refers to.
fn write_order(
    deltas: &[Delta],
    depends: &[Vec<String>],
    seeded: &Created,
) -> Result<Vec<usize>, ChangeProblem> {
    let key_of = |i: usize| {
        deltas[i]
            .target
            .created()
            .filter(|_| existing_id(&deltas[i].target, seeded) == Ok(None))
            .map(|(k, _)| k.to_string())
    };
    let mut order = Vec::with_capacity(deltas.len());
    for current in 0..=7 {
        let mut pending: Vec<usize> = (0..deltas.len())
            .filter(|i| stage(&deltas[*i], seeded) == current)
            .collect();
        while !pending.is_empty() {
            let placed: HashSet<String> = order.iter().filter_map(|i| key_of(*i)).collect();
            let waiting: HashSet<String> = pending.iter().filter_map(|i| key_of(*i)).collect();
            let ready = pending.iter().position(|i| {
                depends[*i]
                    .iter()
                    .all(|k| placed.contains(k) || !waiting.contains(k))
            });
            match ready {
                Some(at) => order.push(pending.remove(at)),
                None => {
                    return Err(ChangeProblem::ReferenceCycle {
                        change: pending
                            .iter()
                            .map(|i| deltas[*i].last)
                            .min()
                            .unwrap_or_default(),
                        keys: pending.iter().filter_map(|i| key_of(*i)).collect(),
                    });
                }
            }
        }
    }
    Ok(order)
}

/// Apply one target's changes, in order, to its JSON body.
fn patch(
    target: &ChangeTarget,
    before: Option<Value>,
    changes: &[Change],
    indices: &[usize],
) -> Result<Option<Value>, ChangeProblem> {
    let mut doc = before.clone();
    let original_id = before.as_ref().and_then(|b| b.get("id")).cloned();

    for &i in indices {
        let change = &changes[i];
        let bad_path = |reason: &str| ChangeProblem::BadPath {
            change: i,
            path: change.path.clone(),
            reason: reason.to_string(),
        };
        let unsupported = |reason: &str| ChangeProblem::UnsupportedOp {
            change: i,
            reason: reason.to_string(),
        };
        let tokens = pointer::parse(&change.path).map_err(|e| bad_path(&e))?;
        let needs_value = || {
            change
                .value
                .clone()
                .ok_or_else(|| unsupported(&format!("{:?} needs a value", change.op)))
        };
        if change.op == ChangeOp::Add && change.expect.is_some() {
            return Err(unsupported("add has no current value to expect"));
        }

        if doc.is_none() {
            // Nothing exists (yet, or any more): only the add that creates a
            // new resource fits.
            if before.is_some() {
                return Err(unsupported("an earlier change removed this resource"));
            }
            let Some((_, kind)) = target.created() else {
                return Err(unsupported("nothing to change"));
            };
            let what = match kind {
                RefKind::Event => "event",
                RefKind::Asset => "asset",
                RefKind::Account => "account",
                RefKind::Parameter => "parameter",
                RefKind::ReturnProfile => "return profile",
                RefKind::TaxConfig => "tax config",
            };
            if !(change.op == ChangeOp::Add && tokens.is_empty()) {
                return Err(unsupported(&format!(
                    "a new {what}'s first change must be add at \"\" with its whole body"
                )));
            }
            let mut value = needs_value()?;
            let Some(object) = value.as_object_mut() else {
                return Err(unsupported(&format!(
                    "a new {what}'s body must be an object"
                )));
            };
            object.remove("id");
            doc = Some(value);
            continue;
        }
        let current = doc.as_mut().expect("checked above");

        if let Some(expected) = &change.expect {
            let actual = pointer::get(current, &tokens)
                .ok_or_else(|| bad_path("nothing at this path to compare with expect"))?;
            if !pointer::approx_eq(expected, actual) {
                return Err(ChangeProblem::Stale {
                    change: i,
                    path: change.path.clone(),
                    expected: Box::new(expected.clone()),
                    actual: Box::new(actual.clone()),
                });
            }
        }

        if tokens.is_empty() {
            match change.op {
                ChangeOp::Remove => doc = None,
                ChangeOp::Replace => {
                    let mut value = needs_value()?;
                    let Some(object) = value.as_object_mut() else {
                        return Err(unsupported("a resource body must be an object"));
                    };
                    match (object.get("id"), &original_id) {
                        (Some(id), Some(original)) if id != original => {
                            return Err(bad_path("id is read-only"));
                        }
                        (_, Some(original)) => {
                            object.insert("id".into(), original.clone());
                        }
                        (Some(_), None) => {
                            object.remove("id");
                        }
                        _ => {}
                    }
                    *current = value;
                }
                ChangeOp::Add => {
                    return Err(unsupported(
                        "add at \"\" creates a resource; target the matching new_* kind instead",
                    ));
                }
            }
            continue;
        }

        if let Some(reason) = read_only(target, &tokens) {
            return Err(bad_path(reason));
        }
        let applied = match change.op {
            ChangeOp::Replace => pointer::replace(current, &tokens, needs_value()?),
            ChangeOp::Add => pointer::add(current, &tokens, needs_value()?),
            ChangeOp::Remove => pointer::remove(current, &tokens).map(drop),
        };
        applied.map_err(|e| bad_path(&e))?;
    }
    Ok(doc)
}

fn read_only(target: &ChangeTarget, tokens: &[String]) -> Option<&'static str> {
    let token = |i: usize| tokens.get(i).map(String::as_str);
    let account = matches!(
        target,
        ChangeTarget::Account(_) | ChangeTarget::NewAccount(_)
    );
    match (token(0), tokens.len()) {
        (Some("id"), 1) => Some("id is read-only"),
        (Some("name"), _) if *target == ChangeTarget::Scenario => {
            Some("a plan's name is not edited here")
        }
        (Some("start_date"), _) if *target == ChangeTarget::Scenario => {
            Some("a plan's start date is set by the server")
        }
        (Some("flavor"), 1) if account => {
            Some("an account's flavor cannot change; create a new account instead")
        }
        (Some("positions"), 3) if account && token(2) == Some("id") => {
            Some("a position's id is read-only")
        }
        _ => None,
    }
}

/// Lower a target's before/after bodies onto the typed writes. An unchanged
/// target lowers to nothing. `check_expressions` validates event bodies'
/// expressions against `graph`; off when the batch creates something they
/// might name, in which case the edit that writes the event checks them.
fn lower(
    graph: &ScenarioGraph,
    target: &ChangeTarget,
    before: Option<&Value>,
    after: Option<&Value>,
    check_expressions: bool,
) -> Result<Vec<ResolvedChange>, String> {
    if let (Some(before), Some(after)) = (before, after)
        && pointer::approx_eq(before, after)
    {
        return Ok(Vec::new());
    }
    let event = |value: &Value| event_body(graph, value, check_expressions);
    Ok(match (target, after) {
        (
            ChangeTarget::NewEvent(_)
            | ChangeTarget::NewAsset(_)
            | ChangeTarget::NewAccount(_)
            | ChangeTarget::NewParameter(_)
            | ChangeTarget::NewReturnProfile(_)
            | ChangeTarget::NewTaxConfig(_),
            None,
        ) => Vec::new(),
        (ChangeTarget::NewParameter(key), Some(after)) => vec![ResolvedChange::CreateParameter {
            key: key.clone(),
            body: serde_json::from_value(project(after, |k| k != "id"))
                .map_err(|e| e.to_string())?,
        }],
        (ChangeTarget::Parameter(id), None) => vec![ResolvedChange::DeleteParameter { id: *id }],
        (ChangeTarget::Parameter(id), Some(after)) => vec![ResolvedChange::ReplaceParameter {
            id: *id,
            body: serde_json::from_value(project(after, |k| k != "id"))
                .map_err(|e| e.to_string())?,
        }],
        (ChangeTarget::Scenario, None) => return Err("a plan cannot be removed here".into()),
        (ChangeTarget::Scenario, Some(after)) => {
            let before = before.expect("the scenario exists");
            for field in ["name", "start_date"] {
                if before.get(field) != after.get(field) {
                    return Err(format!("{field} cannot change here"));
                }
            }
            let fields = changed_fields(before, after, &["id", "name", "start_date"])?;
            let body = serde_json::from_value(Value::Object(fields)).map_err(|e| e.to_string())?;
            vec![ResolvedChange::UpdateScenario { body }]
        }
        (ChangeTarget::NewReturnProfile(key), Some(after)) => {
            let body: CreateProfile =
                serde_json::from_value(project(after, |k| k != "id")).map_err(|e| e.to_string())?;
            body.distribution.validate(0).map_err(|e| e.to_string())?;
            vec![ResolvedChange::CreateReturnProfile {
                key: key.clone(),
                body,
            }]
        }
        (ChangeTarget::NewTaxConfig(key), Some(after)) => {
            let body: CreateTaxConfig =
                serde_json::from_value(project(after, |k| k != "id")).map_err(|e| e.to_string())?;
            crate::api::taxes::checked(&body).map_err(|e| e.to_string())?;
            vec![ResolvedChange::CreateTaxConfig {
                key: key.clone(),
                body,
            }]
        }
        (ChangeTarget::NewEvent(key), Some(after)) => vec![ResolvedChange::CreateEvent {
            key: key.clone(),
            body: event(after)?,
        }],
        (ChangeTarget::NewAsset(key), Some(after)) => vec![ResolvedChange::CreateAsset {
            key: key.clone(),
            body: serde_json::from_value(after.clone()).map_err(|e| e.to_string())?,
        }],
        (ChangeTarget::NewAccount(key), Some(after)) => lower_new_account(key, after)?,
        (ChangeTarget::Event(id), None) => vec![ResolvedChange::DeleteEvent { id: *id }],
        (ChangeTarget::Event(id), Some(after)) => vec![ResolvedChange::ReplaceEvent {
            id: *id,
            body: event(after)?,
        }],
        (ChangeTarget::Asset(id), None) => vec![ResolvedChange::DeleteAsset { id: *id }],
        (ChangeTarget::Asset(id), Some(after)) => {
            let fields = changed_fields(before.expect("assets exist"), after, &["id"])?;
            let body = serde_json::from_value(Value::Object(fields)).map_err(|e| e.to_string())?;
            vec![ResolvedChange::UpdateAsset { id: *id, body }]
        }
        (ChangeTarget::Account(id), None) => vec![ResolvedChange::DeleteAccount { id: *id }],
        (ChangeTarget::Account(id), Some(after)) => {
            lower_account(*id, before.expect("accounts exist"), after)?
        }
    })
}

fn event_body(graph: &ScenarioGraph, value: &Value, check: bool) -> Result<EventBody, String> {
    let body: EventBody = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if check {
        crate::api::expressions::validate_tree(graph, &body.effects).map_err(|e| e.to_string())?;
    }
    Ok(body)
}

/// A new account: the account itself, then one lot per `positions` entry.
fn lower_new_account(key: &str, after: &Value) -> Result<Vec<ResolvedChange>, String> {
    let positions = match after.get("positions") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                if item.get("id").is_some_and(|id| !id.is_null()) {
                    return Err("a new account's positions have no ids yet".to_string());
                }
                serde_json::from_value::<CreatePosition>(item.clone())
                    .map_err(|e| format!("new position: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("positions must be a list".into()),
    };
    let body: CreateAccount =
        serde_json::from_value(project(after, |k| k != "positions" && k != "id"))
            .map_err(|e| e.to_string())?;
    body.flavor.validate().map_err(|e| e.to_string())?;
    if !positions.is_empty() && !matches!(body.flavor, FlavorSpec::Investment { .. }) {
        return Err(format!(
            "positions can only be held in Investment accounts, not {}",
            body.flavor.name()
        ));
    }
    Ok(vec![ResolvedChange::CreateAccount {
        key: key.to_string(),
        body,
        positions,
    }])
}

/// The top-level fields of `after` that differ from `before`, skipping `skip`.
/// PATCH bodies read `null` as "unchanged", so clearing a field is refused
/// rather than silently dropped — except where the body can say it.
fn changed_fields(
    before: &Value,
    after: &Value,
    skip: &[&str],
) -> Result<Map<String, Value>, String> {
    const CLEARABLE: &[&str] = &["return_profile_id"];
    let mut fields = Map::new();
    let after = after.as_object().ok_or("the body must be an object")?;
    for (key, value) in after {
        if skip.contains(&key.as_str()) {
            continue;
        }
        let old = before.get(key).unwrap_or(&Value::Null);
        if pointer::approx_eq(old, value) {
            continue;
        }
        if value.is_null() && !CLEARABLE.contains(&key.as_str()) {
            return Err(format!("{key} cannot be cleared"));
        }
        fields.insert(key.clone(), value.clone());
    }
    for key in before.as_object().into_iter().flat_map(|b| b.keys()) {
        if !after.contains_key(key) && !skip.contains(&key.as_str()) {
            return Err(format!("{key} cannot be removed"));
        }
    }
    Ok(fields)
}

/// `UpdateAccount`'s own fields; every other top-level field bar `id` and
/// `positions` is the flattened flavor detail.
const ACCOUNT_FIELDS: &[&str] = &["name", "description", "sort_order"];

/// The object's fields that `keep` accepts.
fn project(value: &Value, keep: impl Fn(&str) -> bool) -> Value {
    Value::Object(
        value
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(k, _)| keep(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    )
}

fn lower_account(id: i64, before: &Value, after: &Value) -> Result<Vec<ResolvedChange>, String> {
    if before.get("flavor") != after.get("flavor") {
        return Err("an account's flavor cannot change; create a new account instead".into());
    }
    let is_flavor = |k: &str| !ACCOUNT_FIELDS.contains(&k) && k != "id" && k != "positions";

    // Name, description and order patch field by field; the flavor detail is
    // replaced wholesale, so any change to it sends all of it.
    let mut fields = changed_fields(
        &project(before, |k| ACCOUNT_FIELDS.contains(&k)),
        &project(after, |k| ACCOUNT_FIELDS.contains(&k)),
        &[],
    )?;
    let flavor = project(after, is_flavor);
    if !pointer::approx_eq(&project(before, is_flavor), &flavor) {
        fields.extend(flavor.as_object().cloned().unwrap_or_default());
    }

    let mut writes = Vec::new();
    if !fields.is_empty() {
        let body: UpdateAccount =
            serde_json::from_value(Value::Object(fields)).map_err(|e| e.to_string())?;
        if let Some(flavor) = &body.flavor {
            flavor.validate().map_err(|e| e.to_string())?;
        }
        writes.push(ResolvedChange::UpdateAccount { id, body });
    }
    writes.extend(lower_positions(id, before, after)?);
    Ok(writes)
}

/// Lots are matched by `id`: one the batch dropped is deleted, one without an
/// id is new, one whose fields moved is patched.
fn lower_positions(
    account_id: i64,
    before: &Value,
    after: &Value,
) -> Result<Vec<ResolvedChange>, String> {
    let list = |v: &Value| -> Result<Vec<Value>, String> {
        match v.get("positions") {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) => Ok(items.clone()),
            Some(_) => Err("positions must be a list".into()),
        }
    };
    let (old, new) = (list(before)?, list(after)?);
    let id_of = |v: &Value| v.get("id").and_then(Value::as_i64);
    let old_ids: Vec<i64> = old.iter().filter_map(id_of).collect();

    let kept: Vec<i64> = new.iter().filter_map(id_of).collect();
    if let Some(unknown) = kept.iter().find(|id| !old_ids.contains(id)) {
        return Err(format!("no position with id {unknown} in this account"));
    }
    let retained: Vec<i64> = old_ids
        .iter()
        .copied()
        .filter(|id| kept.contains(id))
        .collect();
    if retained != kept {
        return Err("reordering positions is not supported here".into());
    }

    let mut writes = Vec::new();
    for item in &new {
        match id_of(item) {
            Some(position_id) => {
                let previous = old
                    .iter()
                    .find(|p| id_of(p) == Some(position_id))
                    .expect("checked above");
                let fields = changed_fields(previous, item, &["id"])?;
                if !fields.is_empty() {
                    let body = serde_json::from_value(Value::Object(fields))
                        .map_err(|e| format!("position {position_id}: {e}"))?;
                    writes.push(ResolvedChange::UpdatePosition {
                        account_id,
                        position_id,
                        body,
                    });
                }
            }
            None => {
                let body = serde_json::from_value(item.clone())
                    .map_err(|e| format!("new position: {e}"))?;
                writes.push(ResolvedChange::CreatePosition { account_id, body });
            }
        }
    }
    for position_id in old_ids.into_iter().filter(|id| !kept.contains(id)) {
        writes.push(ResolvedChange::DeletePosition {
            account_id,
            position_id,
        });
    }
    Ok(writes)
}
