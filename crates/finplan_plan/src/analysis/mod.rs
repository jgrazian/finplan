//! Parameter sweeps, sensitivity rankings, goal seeks and what-if stacks, as
//! pure computation over a plan.
//!
//! A host (the server's job table, a browser's worker) does the orchestration:
//! who may ask, queueing, progress, cancellation, storing an answer. What it
//! hands to this module is a [`ScenarioGraph`](crate::graph::ScenarioGraph) and
//! a request, and what it gets back is a response body. The three steps are
//! separate so a host can sit between them:
//!
//! 1. [`discover`] lists what the plan can vary ([`AnalysisParameter`]); the
//!    request names parameters by those ids.
//! 2. [`prepare`] checks a [`CreateAnalysis`] against the plan, lowers it onto
//!    the compiled config and costs it, giving an [`AnalysisSpec`] whose
//!    [`budget`](AnalysisSpec::budget) is the progress denominator.
//! 3. [`run`] evaluates the spec against an [`McRunner`], which is the one place
//!    simulations happen: a rayon-backed runner on the server, a sequential one
//!    (or a coordinator over workers) in the browser. [`analyze`] is 2 and 3 in
//!    one call.
//!
//! The runner trait lives in `finplan_core::analysis`, beside the sweep and
//! solve loops it drives, and is re-exported here.
//!
//! A sweep axis, a sensitivity row and a solve's varied parameter are all the
//! same thing — a number in the plan that could have been different — so they
//! are discovered once, from the compiled scenario ([`params`]), and every kind
//! picks from that one list.

pub mod params;
pub mod prepare;
pub mod request;
pub mod results;
pub mod run;

pub use finplan_core::analysis::{McRunner, ProgressRunner, StatsRun, SweepProgress};
pub use params::{ParamKind, PlanParameter, parameters};
pub use prepare::{
    ANALYSIS_SEED, AnalysisSpec, Limits, MAX_ANALYSIS_ITERATIONS, MIN_ITERATIONS, Prepared,
    iterations_or_default, prepare,
};
pub use request::{AxisRequest, ConstraintRequest, CreateAnalysis, ObjectiveRequest};
pub use results::{
    AnalysisOutcome, AnalysisParameter, AnalysisPoint, SensitivityResults, SensitivityRow,
    SolveOutcome, SolveStep, SweepAxis, SweepCell, SweepResults, WhatIfFan, WhatIfOutcome,
    WhatIfStep,
};
pub use run::{AnalysisError, analyze, run};

use crate::compile;
use crate::error::PlanResult;
use crate::graph::ScenarioGraph;

/// Everything `graph`'s plan could vary, with the range each axis defaults to:
/// the body of `GET /scenarios/{id}/analysis/parameters`.
pub fn discover(graph: &ScenarioGraph) -> PlanResult<Vec<AnalysisParameter>> {
    let compiled = compile::compile(graph)?;
    Ok(parameters(&compiled).iter().map(Into::into).collect())
}

#[cfg(test)]
mod tests;
