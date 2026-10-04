//! The static half of every review request: instructions, how the model
//! behaves, the body types changes are written against, and the tools.
//!
//! Everything here is byte-identical across requests and reviews, so it sits
//! in front of the prompt-cache breakpoint. Keep it deterministic — no dates,
//! ids, or map iteration of unordered collections.

use std::sync::OnceLock;

use serde_json::{Value, json};
use ts_rs::{Config, TS};

use super::tools::Registry;
use finplan_plan::rules::{Evidence, Kind, Section};
use finplan_plan::specs::RepaymentSpec;
use finplan_plan::specs::accounts::{
    Account, ContributionPeriod, CreateAccount, FlavorSpec, Position, TaxStatus,
};
use finplan_plan::specs::assets::{Asset, CreateAsset};
use finplan_plan::specs::events::EventBody;
use finplan_plan::specs::{
    AmountMode, AmountSpec, AssetRef, Comparison, EffectSpec, FinancingSpec, IncomeType, Interval,
    LotMethod, OffsetUnit, TriggerSpec, WithdrawalSourcesSpec, WithdrawalStrategy,
};
use finplan_plan::suggest::{Change, ChangeOp, ChangeTarget};

pub const PREVIEW_TOOL: &str = super::tools::PREVIEW;
pub const SUBMIT_TOOL: &str = "submit_suggestion";

/// How the model writes everything — its reasoning, notes, summaries and
/// chat answers: compact and technical, so a reasoning model spends its
/// output budget on the problem rather than on prose, and the user reads
/// plain, exact sentences. Sent to every review, chat and drafting request,
/// ahead of their own instructions.
pub const WRITING_STYLE: &str = "\
Writing style. Write everything about 80% of the way to ASD-STE100 Simplified Technical English: your reasoning and thinking, note titles, summaries and reasoning, path and step text, and chat answers. Use short declarative sentences, one idea per sentence, active voice, present tense, one word for one meaning, no filler and no restating. Use figures, account and event names, and the plan's own terms. Length limits and formats set elsewhere still apply.";

pub const SYSTEM_PROMPT: &str = concat!("\
You review a personal financial plan built in FinPlan, a Monte Carlo retirement planner, and write short notes for its Review tab. The user message holds the plan, the results of one simulation run of it, the notes FinPlan's built-in rules already wrote, the standard checks (plan_checks), any notes still open on the board from earlier reviews (open_notes), and the notes the user dismissed (dismissed_notes).

Standard checks. plan_checks lists every check a review makes, with what it looks for and the path it offers. A check marked ran is a rule that already looked at this run: \"found nothing\" means it looked and the plan passes, and its notes, if it wrote any, are in existing_notes. A check marked yours is your checklist: work through each one that applies to this plan, and write a note where it finds something material. A check marked before every run is preflight; the preflight tool lists what it raised.

Each note is one idea, of one kind:
- fix: something in the plan is likely wrong or costly, and the correction can be expressed as changes to the plan.
- check: an input that may be wrong or missing, where the user knows best. Offer a path with a clearly labelled estimate whenever a reasonable one exists; a check with no path must say why in no_change_reason.
- stress: a scenario worth testing, such as a market drop in the first retired year, high inflation, or lost income. Its changes add events; the app applies a stress note to a copy of the plan, never to the plan itself.
- read: an observation about the results. No paths.

Paths and steps. A note's changes are organised as paths: each path is one course of action the user can choose, and the user follows at most one. Offer two to four paths when there is a real trade-off (for example: spend less, retire later, or hold more cash), mark exactly one recommended, and make each path self-contained. When there is one obvious fix, offer a single path. Each path is one to four ordered steps; each step is a self-contained change the user could stop after (for example: first fix the payment, then add the missing income). Steps apply in order: a step's changes, and their `expect` values, read the plan as the earlier steps of its path left it, and a later step can use what an earlier step created.
Each note belongs to one section: portfolio (accounts, holdings, return assumptions), plan (events: income, spending, purchases, timing), or results (what the run shows).

What matters, in order. Choose notes, and the changes in them, by this priority, and give each note its motive:
1. correctness: the plan misstates how FinPlan works or contains an evident modeling error (a mortgage payment indexed to inflation, a sweep that sells investments while the cash is already there).
2. realism: something the person almost certainly has or will face is missing or unrealistic (no Social Security after decades of earnings, a house that never appreciates, idle cash far above any need, spending that never changes in retirement).
3. risk: where and why the plan fails, when the failure is material.
4. optimization: a better outcome from the same facts, only when the effect is material.
The aim is the simulation that makes the most sense given the person's starting point, not the one that scores best. Prefer changes that make the plan reflect the person's likely reality over changes that squeeze the metric. Never propose tax-lot, withdrawal-order or strategy tweaks, or small parameter nudges, whose simulated effect is marginal: a result like funding 99.90% -> 99.95%, or a median that moves by a fraction of a percent, is itself the sign the note is not worth the user's attention. The server rejects risk and optimization paths below a materiality floor (the rejection states it and the measured effect). Correctness and realism notes are exempt, because they fix the model of reality whatever the metric does, but they are still previewed.

Say what is missing with a change, not only in prose. Missing income or spending is a new_event (an Income or Expense with the trigger, amount and account you would expect), referring by a $new reference (see Changes in the reference) to anything the path creates. When the right figure is a real-world fact the plan does not hold, propose a conservative, round estimate and label it: put \"estimated\" in the step title, state the method in one sentence of the reasoning, and name the figure to use instead. For Social Security: claim at 67, inflation-adjusted, the amount from estimate_social_security over the plan's own earnings (its method stated in the reasoning), and the person's SSA statement as the figure to use instead. Leave a check without a path only when no reasonable estimate exists (a true cost basis), and give no_change_reason.

How a note reads. The title is one specific sentence, at most 120 characters, naming the account, event or asset and the number that matters. The summary is one plain sentence, shown open under the title: what you found and what it means for this plan, without repeating the title. The reasoning is shown only when the user expands it: two to four plain sentences of working, with what you saw and where, the figures and how you reached them, and what the change does; do not restate the summary. Use the plan's own names, dollar amounts, ages and years. Round: money to two or three significant figures ($11.8M, $137k, $2,400/month), rates to one decimal unless a smaller difference is the point. Leave out disclaimers, hedging boilerplate and generic financial advice; the app shows its own disclaimer.

Numbers. Every number in a note must come from the plan, the run, a preview you ran, a tool's result, or arithmetic on those; list where in `evidence`, citing a tool's result as `computed` with the tool_use id of the call. Before submitting, run preview_changes on each path (all of its steps, in order) and quote the simulated effect (for example: funding 90.6% -> 91.8%) in that path's reasoning or the note's. Give a path an `estimate` only when you did not preview it, and call it an estimate. When a preview reports paired: false, the two runs used different random draws, so present the comparison as approximate.

Changes are JSON-pointer edits written against exactly the bodies shown in the user message. Set `expect` to the current value you are replacing, copied from the plan. Prefer the smallest change that expresses the idea.

Do not redo a check marked ran, and do not repeat a note still open on the board, even reworded, and never raise again a note the user dismissed or the concern behind it. A few sharp notes beat many; skip anything minor. Submit each note with submit_suggestion. If a submission is rejected, fix the problem it names or drop the note. When you are done, end your turn with a one-line summary.

", super::tools::guide!());

/// How the simulation behaves, as far as a reviewer needs to know. Checked
/// against finplan_core; update alongside the engine.
const SEMANTICS: &str = "\
How FinPlan simulates (facts about this engine, not general finance):
1. Each return profile draws its own annual return every year, independently of every other profile: there is no correlation between profiles. Assets on the same profile share each year's draw (plus their tracking error, if set). Two equity funds on different profiles can crash in different years, which overstates diversification.
2. Return and inflation profiles are annual nominal rates. Inflation is drawn separately from returns.
3. Federal tax bracket thresholds are indexed each year to the path's simulated inflation (lagged one year, like IRS indexing), so they hold steady in real terms. The standard deduction, plus its 65+ extra from the tax year the person turns 65, shields each year's first ordinary income and is indexed the same way. State tax and long-term capital gains are flat rates. Withdrawals from tax-deferred accounts before age 59.5 pay the early-withdrawal penalty.
4. An InflationAdjusted amount grows its inner amount with cumulative sampled inflation since the plan start. A Fixed amount stays nominal.
5. Liability balances are negative. AdjustBalance with a positive amount on a liability adds debt. A trigger 'balance >= 0' on a liability means the loan is paid off. Payments are events (a CashTransfer into the liability) unless the loan has a repayment term.
6. A property account holds an asset whose value follows that asset's return profile. An asset with no return profile keeps its nominal price forever.
7. A Sweep liquidates its stated amount from its sources into the target account every time it fires, however much cash the target already holds. Strategy sources draw from investment accounts ordered by tax status: PenaltyAware takes taxable, then tax-free, then tax-deferred before age 59.5, and taxable, tax-deferred, tax-free after. Sales realize capital gains in taxable accounts and ordinary income in tax-deferred ones.
8. Success rate: the share of iterations whose final net worth (all accounts, property included, liabilities subtracted) is above zero. Funding success rate: the share with no settled cash shortfall and no failed event. A plan can succeed on paper while running out of cash when illiquid property carries its final net worth.
9. Yearly cash flows and balances in the run section come from one representative path, ranked by final nominal net worth (the median path unless noted); they are not per-year medians. Funding diagnostics cover every iteration.
10. preview_changes re-runs the same run's random draws with the edit applied (paired: true), unless the edit changes which return or inflation profiles are in use (paired: false).

Changes. `target` is exactly one of {\"event\": id}, {\"asset\": id}, {\"account\": id}, {\"new_event\": \"<key>\"}, {\"new_asset\": \"<key>\"} or {\"new_account\": \"<key>\"}. `path` is an RFC 6901 pointer into that resource's body; \"\" is the whole resource. remove at \"\" deletes the resource (events only). To create an event, asset or account, use the matching new_* target with a key of your choice (unique across all three kinds within the path), op add, path \"\", and a whole body as value (EventBody, CreateAsset or CreateAccount); later changes with the same key, in the same or a later step of the path, patch it. Anywhere an id is expected, {\"$new\": \"<key>\"} refers to the resource created under that key earlier in the path. Positions are edited under an account at /positions/<index>/<field>; `id` fields and an account's `flavor` tag are read-only. Ids in the plan are database ids; use them as given.";

/// The TypeScript declarations of every body a change can touch, generated
/// from the server's own types so they can never drift from what `resolve`
/// accepts.
fn declarations() -> String {
    let cfg = Config::new().with_large_int("number");
    [
        EventBody::decl(&cfg),
        TriggerSpec::decl(&cfg),
        EffectSpec::decl(&cfg),
        AmountSpec::decl(&cfg),
        WithdrawalSourcesSpec::decl(&cfg),
        WithdrawalStrategy::decl(&cfg),
        FinancingSpec::decl(&cfg),
        AssetRef::decl(&cfg),
        Comparison::decl(&cfg),
        OffsetUnit::decl(&cfg),
        Interval::decl(&cfg),
        AmountMode::decl(&cfg),
        IncomeType::decl(&cfg),
        LotMethod::decl(&cfg),
        Asset::decl(&cfg),
        CreateAsset::decl(&cfg),
        Account::decl(&cfg),
        CreateAccount::decl(&cfg),
        FlavorSpec::decl(&cfg),
        RepaymentSpec::decl(&cfg),
        Position::decl(&cfg),
        TaxStatus::decl(&cfg),
        ContributionPeriod::decl(&cfg),
        Change::decl(&cfg),
        ChangeOp::decl(&cfg),
        ChangeTarget::decl(&cfg),
        Evidence::decl(&cfg),
        Kind::decl(&cfg),
        Section::decl(&cfg),
    ]
    .join("\n")
}

/// The second system block: semantics plus the body types. Built once.
pub fn reference() -> &'static str {
    static REFERENCE: OnceLock<String> = OnceLock::new();
    REFERENCE.get_or_init(|| {
        format!(
            "{SEMANTICS}\n\nBody types (TypeScript, generated from the server):\n```ts\n{}\n```",
            declarations()
        )
    })
}

/// What the first user turn asks, after the plan and run.
pub fn task(max_suggestions: usize) -> String {
    format!(
        "Review this plan and its run. Submit at most {max_suggestions} notes that the checks marked ran and the open notes above do not already cover, most important first: correctness, then realism, then material risk, then material optimization. Fewer, well-founded notes beat more. Preview each path of a note (all of its steps) before submitting it."
    )
}

pub(crate) fn change_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "op": {"type": "string", "enum": ["replace", "add", "remove"]},
            "target": {
                "type": ["object", "string"],
                "description": "Exactly one key: {\"event\": id} | {\"asset\": id} | {\"account\": id} | {\"parameter\": id} | {\"new_event\": \"<key>\"} | {\"new_asset\": \"<key>\"} | {\"new_account\": \"<key>\"} | {\"new_parameter\": \"<key>\"} | {\"new_return_profile\": \"<key>\"} | {\"new_tax_config\": \"<key>\"}. Parameters are for reviews and drafts alike: {\"parameter\": id} changes one the plan lists (as a goal_seek answer is applied), {\"new_parameter\": \"<key>\"} adds one. Drafts only: the bare string \"scenario\" edits the plan's own settings, and the library targets (new_return_profile, new_tax_config) are for the drafting reference.",
                "properties": {
                    "event": {"type": "integer"},
                    "asset": {"type": "integer"},
                    "account": {"type": "integer"},
                    "parameter": {"type": "integer"},
                    "new_event": {"type": "string"},
                    "new_asset": {"type": "string"},
                    "new_account": {"type": "string"},
                    "new_parameter": {"type": "string"},
                    "new_return_profile": {"type": "string"},
                    "new_tax_config": {"type": "string"}
                }
            },
            "path": {"type": "string", "description": "RFC 6901 pointer into the target's body; \"\" is the whole resource."},
            "expect": {"description": "The current value at path, copied from the plan. Omit for add."},
            "value": {"description": "The new value, for replace and add. In any id field, {\"$new\": \"<key>\"} names a resource created earlier in the path."}
        },
        "required": ["op", "target", "path"]
    })
}

pub(super) fn evidence_schema() -> Value {
    json!({
        "type": "array",
        "description": "Where the note's numbers come from.",
        "items": {
            "type": "object",
            "properties": {
                "ref": {"type": "string", "enum": ["ledger", "account_series", "stat", "diagnostic", "computed", "document", "answer", "description"]},
                "year": {"type": "integer", "description": "ledger: the calendar year"},
                "event_id": {"type": ["integer", "null"], "description": "ledger: narrow to an event"},
                "account_id": {"type": ["integer", "null"], "description": "ledger or account_series: the account"},
                "date": {"type": "string", "description": "account_series: the YYYY-MM-DD point"},
                "value": {"type": "number", "description": "account_series, stat, diagnostic: the number"},
                "name": {"type": "string", "description": "stat: what the number is"},
                "field": {"type": "string", "description": "diagnostic: the funding_diagnostics field"},
                "tool": {"type": "string", "description": "computed: the tool that produced the figure"},
                "call_id": {"type": "string", "description": "computed: the tool_use id of that call"},
                "document_id": {"type": "integer", "description": "document: the uploaded document, when the session has any"},
                "page": {"type": "integer", "description": "document: the 1-based page"},
                "excerpt": {"type": "string", "description": "document, description: the exact text quoted, at most 400 characters; checked against the source"},
                "question_key": {"type": "string", "description": "answer: the key of a question the user answered"}
            },
            "required": ["ref"]
        }
    })
}

fn estimate_schema() -> Value {
    json!({
        "type": ["object", "null"],
        "description": "Only when the path was not previewed: your estimate of the edited run's rates, as fractions.",
        "properties": {
            "success_rate": {"type": ["number", "null"]},
            "funding_success_rate": {"type": ["number", "null"]}
        }
    })
}

fn step_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "key": {"type": "string", "description": "Unique in the path: 1-32 of a-z, 0-9, -."},
            "title": {"type": "string", "description": "What the step does, at most 80 characters."},
            "reasoning": {"type": ["string", "null"]},
            "changes": {"type": "array", "items": change_schema(), "minItems": 1}
        },
        "required": ["key", "title", "changes"]
    })
}

pub(super) fn paths_schema() -> Value {
    json!({
        "type": "array",
        "description": "The courses of action, at most 4; empty for read notes, and for a check only with no_change_reason. Exactly one recommended when there are several.",
        "maxItems": 4,
        "items": {
            "type": "object",
            "properties": {
                "key": {"type": "string", "description": "Unique in the note: 1-32 of a-z, 0-9, -."},
                "label": {"type": "string", "description": "The choice in a few words, at most 80 characters."},
                "reasoning": {"type": ["string", "null"], "description": "Why this path rather than the others, with its simulated effect."},
                "recommended": {"type": "boolean"},
                "estimate": estimate_schema(),
                "steps": {
                    "type": "array",
                    "description": "1 to 4 ordered steps, each one self-contained change the user could stop after.",
                    "minItems": 1,
                    "maxItems": 4,
                    "items": step_schema()
                }
            },
            "required": ["key", "label", "recommended", "steps"]
        }
    })
}

fn submit_tool() -> Value {
    json!(
        {
            "name": SUBMIT_TOOL,
            "description": "Add one note to the Review tab. The server checks every path's steps against the plan and the evidence against the run, and either accepts the note (returning the diffs the user will see) or rejects it with the problems to fix. Submit only finished notes. In a chat, a note already on the board that is open with nothing applied can be rewritten in place: submit the whole revised note with `replaces` set to its id.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["fix", "check", "stress", "read"]},
                    "section": {"type": "string", "enum": ["portfolio", "plan", "results"]},
                    "motive": {
                        "type": "string",
                        "enum": ["correctness", "realism", "risk", "optimization"],
                        "description": "Why the note matters, by the priority in the instructions. risk and optimization paths must clear the materiality floor."
                    },
                    "title": {"type": "string", "description": "One specific sentence, at most 120 characters."},
                    "summary": {"type": "string", "description": "One plain sentence, at most 200 characters, shown open under the title: what you found and what it means for this plan. Do not repeat the title."},
                    "reasoning": {"type": "string", "description": "The working behind the summary, shown when the user expands it: two to four plain sentences, at most 1200 characters, with the figures, how they were reached and what to check. Do not restate the summary."},
                    "evidence": evidence_schema(),
                    "paths": paths_schema(),
                    "no_change_reason": {
                        "type": ["string", "null"],
                        "description": "Only for a check note with no paths: one line on why the issue cannot be expressed as a plan change, even as a labelled estimate. Appended to the reasoning."
                    },
                    "replaces": {
                        "type": ["integer", "null"],
                        "description": "Only in a chat: the id (#N) of an open note on the board, with nothing applied, that this submission rewrites in place — a different figure, option or wording. The note keeps its id and thread; send the complete revised note, not just what changed. Omit to add a new note."
                    },
                },
                "required": ["kind", "section", "motive", "title", "summary", "reasoning", "evidence", "paths"]
            }
        }
    )
}

/// The tools a loop offers: the registry's, then `submit_suggestion`.
pub fn tools(registry: &Registry) -> Value {
    let mut definitions = registry.definitions();
    definitions.push(submit_tool());
    Value::Array(definitions)
}
