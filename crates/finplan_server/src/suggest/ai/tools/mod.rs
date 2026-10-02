//! The model's shared tools, as a registry: one [`ToolSpec`] per tool (name,
//! description, JSON schema, handler), grouped, with one dispatcher. The review
//! loop (`super`) serves the whole registry beside its own `submit_suggestion`;
//! a loop with other needs — the drafting agent — picks a subset with
//! [`Registry::only`] or [`Registry::group`], adds its own tools next to them,
//! and hands whatever it does not own to [`Registry::dispatch`].
//!
//! Groups, one module each:
//!
//! - [`plan`]: `preview_changes`, `preview_paths`, `validate_changes`,
//!   `preflight` — the plan and simulations of it, through a [`ToolHost`].
//!   Simulation-backed tools spend the loop's preview budget, per simulation.
//! - [`runs`]: `inspect_path`, `failure_profile` — what a stored run shows.
//! - [`goal_seek`]: `goal_seek` — a parameter searched for the value that
//!   reaches a target, paid for from the preview budget and capped per session.
//! - [`facts`], [`calc`], [`social_security`], [`taxes`]: deterministic
//!   calculators with no host at all, so the model quotes figures it did not
//!   have to recall or work out.
//!
//! A handler answers with a [`ToolOutput`]: the text the model reads, and what
//! the loop must do about it (previews spent, previews that came back clean
//! and so count for the submission check). The loop keeps the text of every
//! successful call by its `tool_use` id, which is what `Evidence::Computed`
//! cites.

pub mod calc;
pub mod facts;
pub mod goal_seek;
mod plan;
mod runs;
pub mod social_security;
pub mod taxes;

use finplan_core::model::TaxConfig;
use serde_json::{Value, json};

use super::BoxFuture;
use crate::observability::{AiTool, AiToolOutcome};
use crate::suggest::{Change, ChangeProblem, DiffLine};

pub use plan::{MAX_CHANGES, batch_key};
pub use runs::PathRank;

/// The prompt paragraph describing the tools, for any loop serving them all.
/// Loops serving a subset say so after it.
macro_rules! guide {
    () => {
        "\
Tools. Besides preview_changes and submit_suggestion you have:
- validate_changes(steps): a dry run that only checks a path's steps against the plan (stale `expect`, bad pointers, invalid bodies) and returns their diffs. It costs no preview; use it before spending one.
- preview_paths(paths): previews two to four paths in one call, on the same random draws, so the comparison is fair. Each path spends one preview from the same budget.
- preflight(): whether the plan can run at all, and what the app flags about it.
- inspect_path(rank, years?) and failure_profile(): the cash flows and balances of a stored path (worst, p10, p25 or median) and where and when failing iterations fail. Ground risk notes in them rather than in guesses.
- reference_facts(topic, year): 401(k), IRA and HSA limits, RMD start age, standard deduction, brackets, Social Security bend points, wage base and full retirement age, and (approximate) state income tax rates. Never quote a statutory figure from memory; call this. Its `status` says published, projected or approximate; say so when the figure is not published.
- finance_calc(op): loan payments, future and present value, pay period to annual and back, age to date and back. Never do this arithmetic yourself.
- estimate_social_security(birth_year, claim_age, earnings or current_salary): the benefit by the statutory formula. Use it for every Social Security estimate, and state its method.
- estimate_taxes(income, filing_status, state): one year of tax on one income, and what the plan's own tax settings would charge.
- goal_seek(parameter, metric, target): searches one named plan parameter (a retirement age, a spending amount) for the value at which success_rate or funding_success_rate reaches the target (0.9 for 90%), and returns that value and the rate achieved. It runs about a dozen simulations, costs 4 previews, and may be used twice in a session: spend it on the one question where a value matters (\"retire at 43 reaches 90%\"), then turn the answer into a path and preview that. A name it does not know is answered with the plan's parameters.
Every figure a tool returns is kept for the session and may be cited as evidence with ref \"computed\", the tool's name as `tool` and the tool_use id of the call as `call_id`. Quote figures from the tool that produced them; a number from nowhere is rejected."
    };
}
pub(crate) use guide;

/// [`guide!`] as a constant.
pub const GUIDE: &str = guide!();

/// The message a tool the host cannot serve answers with.
pub(crate) fn unavailable(tool: &str) -> String {
    format!("{tool} is not available here")
}

/// What a tool needs from the loop's host: the plan the model is working on,
/// and the simulations of it. The review's host is bound to one run; a draft's
/// has no run, and leaves the run-based methods at their defaults.
pub trait ToolHost: Send + Sync {
    /// Simulate `changes` (a path's steps, in order, as one batch); the
    /// `Preview` JSON the preview endpoint returns, or why it could not run.
    fn preview<'a>(&'a self, changes: Vec<Change>) -> BoxFuture<'a, Result<Value, String>>;

    /// Check a path's steps against the plan, each read after the ones before
    /// it: every step's diff lines on success, otherwise the index of the step
    /// that failed and its problems.
    fn resolve_steps(
        &self,
        steps: &[Vec<Change>],
    ) -> Result<Vec<Vec<DiffLine>>, (usize, Vec<ChangeProblem>)>;

    /// The plan's preflight report (`/scenarios/{id}/preflight`) as JSON.
    fn preflight(&self) -> Result<Value, String> {
        Err(unavailable("preflight"))
    }

    /// A stored path of the base run, rendered as text; `years` narrows the
    /// rows to a span of calendar years.
    fn inspect_path<'a>(
        &'a self,
        _rank: PathRank,
        _years: Option<(i64, i64)>,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Err(unavailable("inspect_path")) })
    }

    /// The plan's tax settings, for `estimate_taxes`.
    fn plan_tax_config(&self) -> Option<TaxConfig> {
        None
    }

    /// Search a plan parameter for the value that reaches a target (see
    /// [`goal_seek`]): the found value and the metric achieved, or why it
    /// could not run. The plan is the one the host serves, as it stands.
    fn goal_seek<'a>(
        &'a self,
        _request: goal_seek::GoalSeekRequest,
    ) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async { Err(unavailable("goal_seek")) })
    }
}

/// What one dispatch is given besides the call.
pub struct ToolEnv<'a> {
    pub host: &'a dyn ToolHost,
    /// Previews the loop may still spend.
    pub previews_left: u32,
    /// Goal seeks the session may still run.
    pub goal_seeks_left: u32,
    /// The run's failure aggregates, rendered (see `ReviewContext`), when there
    /// is a run.
    pub failure_profile: Option<&'a Value>,
}

/// What one call came to.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// The tool result the model reads.
    pub text: String,
    pub is_error: bool,
    pub outcome: AiToolOutcome,
    /// preview: whether the edit shares the base run's draws.
    pub paired: Option<bool>,
    /// Problems found with the call's changes.
    pub problems: usize,
    /// The kinds of those problems, or why the input was refused, for logs.
    pub problem_kinds: Vec<&'static str>,
    /// Simulations run, to count against the loop's preview budget.
    pub previews_spent: u32,
    /// Goal seeks run, to count against the session's few.
    pub goal_seeks_spent: u32,
    /// Path batches whose preview came back clean, keyed by [`batch_key`], with
    /// what it returned: "exactly this was previewed" for the submission check.
    pub previewed: Vec<(String, Value)>,
}

impl ToolOutput {
    pub(crate) fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
            outcome: AiToolOutcome::Ok,
            paired: None,
            problems: 0,
            problem_kinds: Vec::new(),
            previews_spent: 0,
            goal_seeks_spent: 0,
            previewed: Vec::new(),
        }
    }

    pub(crate) fn error(outcome: AiToolOutcome, text: impl Into<String>) -> Self {
        Self {
            is_error: true,
            outcome,
            ..Self::ok(text)
        }
    }

    /// From a calculator's answer: JSON on success, its message otherwise.
    pub(crate) fn from_result(result: Result<Value, String>) -> Self {
        match result {
            Ok(value) => Self::ok(value.to_string()),
            Err(message) => Self::error(AiToolOutcome::Invalid, message),
        }
    }
}

/// Which module a tool belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// The plan and simulations of it.
    Plan,
    /// A stored run.
    Runs,
    /// Deterministic calculators; no host.
    Calculators,
}

/// One tool: its definition and what serves it.
#[derive(Debug)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
    pub group: Group,
    /// Its calls count under this label in logs and metrics.
    pub metric: AiTool,
}

pub const PREVIEW: &str = "preview_changes";
pub const PREVIEW_PATHS: &str = "preview_paths";
pub const VALIDATE: &str = "validate_changes";
pub const PREFLIGHT: &str = "preflight";
pub const INSPECT_PATH: &str = "inspect_path";
pub const FAILURE_PROFILE: &str = "failure_profile";
pub const REFERENCE_FACTS: &str = "reference_facts";
pub const FINANCE_CALC: &str = "finance_calc";
pub const ESTIMATE_SOCIAL_SECURITY: &str = "estimate_social_security";
pub const ESTIMATE_TAXES: &str = "estimate_taxes";
pub const GOAL_SEEK: &str = "goal_seek";

/// Every shared tool, in the order the model sees them.
pub const SPECS: &[ToolSpec] = &[
    ToolSpec {
        name: PREVIEW,
        description: "Simulate the plan with one path's steps applied in order, against the same run's random draws, and compare with the run. Returns the diff the user would see, any problems with the changes (stale expect, bad path, invalid body), and base vs edited statistics: success_rate, funding_success_rate, real (today's dollars) final net worth quantiles, and funding diagnostics. Nothing is saved. Preview every path of a note before submitting it, with exactly the steps you will submit; the number of previews per review is limited.",
        schema: plan::preview_schema,
        group: Group::Plan,
        metric: AiTool::Preview,
    },
    ToolSpec {
        name: VALIDATE,
        description: "Check a path's steps against the plan without simulating: each step is read after the ones before it. Returns each step's diff, or the first failing step and its problems (stale expect, bad pointer, invalid body, unknown reference). Free: it does not use a preview. Use it to get a path right before previewing it.",
        schema: plan::preview_schema,
        group: Group::Plan,
        metric: AiTool::Validate,
    },
    ToolSpec {
        name: PREVIEW_PATHS,
        description: "Preview two to four paths of one note in a single call, each simulated against the same run's random draws so the comparison is fair. Each path spends one preview from the same budget as preview_changes. Returns each path's Preview, in order. A path previewed here counts as previewed for submission when its steps are submitted unchanged.",
        schema: plan::preview_paths_schema,
        group: Group::Plan,
        metric: AiTool::PreviewPaths,
    },
    ToolSpec {
        name: PREFLIGHT,
        description: "Whether the plan can run at all, with the issues the app flags: an error that stops the run (invalid_plan), and warnings such as missing spending, an asset with no return assumption, no birth date or no inflation. Free.",
        schema: plan::empty_schema,
        group: Group::Plan,
        metric: AiTool::Preflight,
    },
    ToolSpec {
        name: INSPECT_PATH,
        description: "Cash flows and account balances year by year on one stored path of the run: worst (the lowest stored percentile), p10, p25 or median. The run stores only the percentiles it was started with, so the path returned is the nearest one stored, and the reply says which. Optionally narrow to years [from, to]. Ground risk notes in a failing path's actual years. Free.",
        schema: runs::inspect_schema,
        group: Group::Runs,
        metric: AiTool::InspectPath,
    },
    ToolSpec {
        name: FAILURE_PROFILE,
        description: "Across every iteration that failed the funding check: when the first shortfall came (by year and age), which account ran dry, which events failed, when liquid balances ran out, and how deep and long the deficits were, each with its share of the failures. Aggregates only; per-iteration paths are not stored. Free.",
        schema: plan::empty_schema,
        group: Group::Runs,
        metric: AiTool::FailureProfile,
    },
    ToolSpec {
        name: REFERENCE_FACTS,
        description: "Statutory reference figures as a fixed table: 401(k)/403(b) deferral and catch-ups, IRA and HSA limits, RMD start age by birth year, standard deduction and federal brackets by filing status, Social Security bend points, wage base and full retirement age, and state income tax rates. Years 2024 to 2026. Every answer says `status`: published, projected or approximate (state rates always are). Use this instead of recalling a limit.",
        schema: facts::schema,
        group: Group::Calculators,
        metric: AiTool::ReferenceFacts,
    },
    ToolSpec {
        name: FINANCE_CALC,
        description: "Exact arithmetic: pmt (loan payment), future_value, present_value, period_to_annual and annual_to_period (weekly, biweekly, semimonthly, monthly), age_to_date and date_to_age. Use it for every payment, growth or conversion figure in a note.",
        schema: calc::schema,
        group: Group::Calculators,
        metric: AiTool::FinanceCalc,
    },
    ToolSpec {
        name: ESTIMATE_SOCIAL_SECURITY,
        description: "A retirement benefit by the statutory formula: AIME from an earnings history (wage-indexed, top 35 years) or from a flat current salary (method stated in the reply), PIA by bend points, then the early or delayed claiming adjustment. Returns AIME, PIA, the monthly benefit at claim_age and at 62, full retirement age and 70, and the assumptions. The person's own SSA statement replaces it.",
        schema: social_security::schema,
        group: Group::Calculators,
        metric: AiTool::SocialSecurity,
    },
    ToolSpec {
        name: ESTIMATE_TAXES,
        description: "One year of tax on one income through the same tax code the simulation uses: federal (standard deduction, the year's brackets), state (approximate), payroll tax, and the net. When the plan has tax settings, also what the plan itself would charge on that income. Use it to reconcile a pay stub with the plan.",
        schema: taxes::schema,
        group: Group::Calculators,
        metric: AiTool::Taxes,
    },
    ToolSpec {
        name: GOAL_SEEK,
        description: "Search one named plan parameter for the value at which the success rate (or funding success rate) reaches a target: the earliest retirement age that reaches 90%, the most spending that still does. Runs the app's goal seek (about a dozen fixed-seed simulations) on the plan as it stands. Costs 4 previews from the same budget and may be called twice in a session, so ask the one question that matters. Returns the value found (with its units), the rate achieved there, the plan's rate as it stands, and, when nothing in range reaches the target, the closest value. Turn the answer into a path (change the parameter to that value) and preview that path before submitting it.",
        schema: goal_seek::schema,
        group: Group::Plan,
        metric: AiTool::GoalSeek,
    },
];

/// A set of the shared tools a loop serves.
#[derive(Debug, Clone)]
pub struct Registry {
    specs: Vec<&'static ToolSpec>,
}

impl Registry {
    /// Every shared tool: what the review serves.
    pub fn all() -> Self {
        Self {
            specs: SPECS.iter().collect(),
        }
    }

    /// The tools of one group.
    pub fn group(group: Group) -> Self {
        Self {
            specs: SPECS.iter().filter(|s| s.group == group).collect(),
        }
    }

    /// Exactly these tools, in the registry's own order. A name that is not a
    /// tool is a programming error.
    pub fn only(names: &[&str]) -> Self {
        debug_assert!(
            names.iter().all(|n| SPECS.iter().any(|s| s.name == *n)),
            "unknown tool in {names:?}"
        );
        Self {
            specs: SPECS.iter().filter(|s| names.contains(&s.name)).collect(),
        }
    }

    /// This set without these tools.
    pub fn without(mut self, names: &[&str]) -> Self {
        self.specs.retain(|s| !names.contains(&s.name));
        self
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.specs.iter().map(|s| s.name).collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.specs.iter().any(|s| s.name == name)
    }

    /// The tool definitions as the API takes them, in a fixed order: the
    /// `tools` array's bytes stay identical between requests.
    pub fn definitions(&self) -> Vec<Value> {
        self.specs
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "description": s.description,
                    "input_schema": (s.schema)(),
                })
            })
            .collect()
    }

    /// Serve one call. `None` when the name is not in this registry, so a loop
    /// can fall through to its own tools.
    pub async fn dispatch(
        &self,
        name: &str,
        input: &Value,
        env: &ToolEnv<'_>,
    ) -> Option<(AiTool, ToolOutput)> {
        let spec = self.specs.iter().find(|s| s.name == name)?;
        let output = match spec.name {
            PREVIEW => plan::preview(input, env).await,
            PREVIEW_PATHS => plan::preview_paths(input, env).await,
            VALIDATE => plan::validate(input, env),
            PREFLIGHT => plan::preflight(env),
            INSPECT_PATH => runs::inspect_path(input, env).await,
            FAILURE_PROFILE => runs::failure_profile(env),
            GOAL_SEEK => goal_seek::run(input, env).await,
            REFERENCE_FACTS => ToolOutput::from_result(facts::run(input)),
            FINANCE_CALC => ToolOutput::from_result(calc::run(input)),
            ESTIMATE_SOCIAL_SECURITY => ToolOutput::from_result(social_security::run(input)),
            ESTIMATE_TAXES => {
                ToolOutput::from_result(taxes::run(input, env.host.plan_tax_config().as_ref()))
            }
            _ => return None,
        };
        Some((spec.metric, output))
    }
}
