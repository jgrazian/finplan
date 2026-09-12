//! Parameter sweeps, sensitivity rankings and goal seeks.
//!
//! These share the shape of a run — expensive, CPU-bound, worth watching while
//! it works — but not its lifetime. A run's results are the record of a plan
//! and are stored; an analysis is a question asked of a plan, cheap to ask
//! again, and its answer is only interesting while the question is on screen.
//! So jobs live in memory here rather than in SQLite: nothing to migrate,
//! nothing to garbage-collect out of the database, and a restart simply means
//! pressing Run again.
//!
//! [`cache`] is the one exception. A sweep's grid is the whole Analysis screen
//! and minutes of CPU behind it, so the newest one per scenario is written back
//! as a document and restored on load — see that module for why only the
//! newest, and why a sweep and not the other two.
//!
//! [`parameters`] is the other half. A sweep axis, a sensitivity row and a
//! solve's varied parameter are all the same thing — a number in the plan that
//! could have been different — so they are discovered once, from the compiled
//! scenario, and every mode picks from that one list.

pub mod cache;
pub mod jobs;
pub mod params;
pub mod results;

pub use jobs::{AnalysisJobs, JobHandle, JobKind, JobStatus, JobView, Outcome};
pub use params::{ParamKind, PlanParameter, parameters};
