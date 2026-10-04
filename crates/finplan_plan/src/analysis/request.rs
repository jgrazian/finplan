//! What a client asks an analysis for.
//!
//! Requests name parameters by the ids [`super::discover`] hands out. Nothing
//! here takes an event id and a target from the client: what a plan can vary is
//! derived from the compiled plan, so a request can only ask for something the
//! engine can actually do.

use finplan_core::analysis::{SolveConstraintMetric, SolveObjective};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::what_if::WhatIfLayer;

/// One axis of a requested sweep, or one parameter a solve may vary.
#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct AxisRequest {
    /// An id from `GET /scenarios/{id}/analysis/parameters`.
    pub parameter_id: String,
    /// Range to cover. Omitted, the parameter's own suggested range is used.
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// Points along the axis. Ignored by a bisecting solve, which chooses its
    /// own probes.
    #[serde(default)]
    pub steps: Option<usize>,
}

/// What to optimise for. Named rather than free-form: a client cannot ask for
/// an objective the solver has no way to evaluate.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ObjectiveRequest {
    /// The largest value of the varied parameter that still clears the
    /// constraint — a maximum sustainable withdrawal.
    MaxParameter,
    /// The smallest such value — an earliest retirement age.
    MinParameter,
    /// The highest median terminal net worth.
    MaxMedianNetWorth,
    /// The highest 5th-percentile terminal net worth: the best floor.
    MaxFloorNetWorth,
}

impl From<ObjectiveRequest> for SolveObjective {
    fn from(value: ObjectiveRequest) -> Self {
        match value {
            ObjectiveRequest::MaxParameter => Self::MaxParameter,
            ObjectiveRequest::MinParameter => Self::MinParameter,
            ObjectiveRequest::MaxMedianNetWorth => Self::MaxMedianNetWorth,
            ObjectiveRequest::MaxFloorNetWorth => Self::MaxFloorNetWorth,
        }
    }
}

/// The outcome measure a solve's constraint is written against.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ConstraintRequest {
    SuccessRate,
    FundingSuccessRate,
}

impl From<ConstraintRequest> for SolveConstraintMetric {
    fn from(value: ConstraintRequest) -> Self {
        match value {
            ConstraintRequest::SuccessRate => Self::SuccessRate,
            ConstraintRequest::FundingSuccessRate => Self::FundingSuccessRate,
        }
    }
}

/// The analysis to run. `kind` selects which of the three, and the fields that
/// do not apply to it are ignored.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[ts(export, optional_fields = nullable)]
pub enum CreateAnalysis {
    /// A grid over one to four parameters, capped by the product of the steps
    /// rather than by the count: the client lays graphs out over the result and
    /// each picks its own one or two axes from it.
    Sweep {
        axes: Vec<AxisRequest>,
        #[serde(default)]
        iterations: Option<usize>,
    },
    /// Every parameter moved on its own, ranked by what it did.
    Sensitivity {
        /// Which parameters to rank. Empty means all of them.
        #[serde(default)]
        parameter_ids: Vec<String>,
        /// Band width as a fraction of each parameter's value: `0.2` for ±20%.
        #[serde(default)]
        fraction: Option<f64>,
        #[serde(default)]
        iterations: Option<usize>,
    },
    /// The best values of one to three parameters, subject to a constraint.
    Solve {
        vary: Vec<AxisRequest>,
        objective: ObjectiveRequest,
        #[serde(default)]
        constraint: Option<ConstraintRequest>,
        /// The floor, as a fraction: `0.95` for "success ≥ 95%".
        min_value: f64,
        #[serde(default)]
        iterations: Option<usize>,
    },
    /// The plan with an ordered stack of overrides applied cumulatively: one
    /// step for the plan and one more per layer.
    WhatIf {
        /// The enabled layers only, in order. At most eight.
        layers: Vec<WhatIfLayer>,
        /// Simulations for the whole stack, split evenly across its steps
        /// (each gets at least the analysis minimum).
        #[serde(default)]
        iterations: Option<usize>,
    },
}
