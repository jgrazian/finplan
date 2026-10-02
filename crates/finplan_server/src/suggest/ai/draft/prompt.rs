//! The drafting agent's prompt and tools.
//!
//! The static half (instructions, the engine's semantics and body types the
//! review already uses, and the extra bodies a draft writes) is byte-identical
//! across drafts and sits in front of the cache breakpoint. The per-draft half
//! ([`render_context`]) is the first user turn: the description, the document
//! manifest, the user's library, the limits for the year and the draft so far,
//! with a breakpoint after it so each later turn reads it from the cache.

use std::fmt::Write as _;
use std::sync::OnceLock;

use serde_json::{Value, json};
use ts_rs::{Config, TS};

use super::profiles::LibraryProfile;
use super::{AnswerType, DraftQuestion};
use crate::api::parameters::{ParameterBody, ParameterValueSpec};
use crate::api::profiles::{CreateProfile, DistributionSpec};
use crate::api::scenarios::UpdateScenario;
use crate::api::taxes::{Bracket, CreateTaxConfig};
use crate::compile::rows::ScenarioGraph;
use crate::documents::DocumentManifest;
use crate::suggest::ai::context;
use crate::suggest::ai::tools::{Registry, facts};
use crate::suggest::templates::{
    EmployerMatchParams, HomePurchaseParams, JobLossParams, LargeExpenseParams, MarketCrashParams,
    NewRef, RecurringExpenseParams, RetirementParams, RetirementSpending, RowRef, SalaryParams,
    SocialSecurityParams, Template, TemplateKind, TemplateRequest, When,
};

pub const ASK_USER: &str = "ask_user";
pub const READ_DOCUMENT: &str = "read_document";
pub const EXPAND_TEMPLATE: &str = "expand_template";
pub const FIND_RETURN_PROFILE: &str = "find_return_profile";
pub const SIMULATE_DRAFT: &str = "simulate_draft";
pub const SUMMARIZE_TRANSACTIONS: &str = "summarize_transactions";
pub const MATCH_ACCOUNT: &str = "match_account";
pub const RECONCILE: &str = "reconcile";
pub const SUBMIT: &str = "submit_suggestion";

/// Statements older than this many days get a note to confirm.
pub const STALE_STATEMENT_DAYS: u32 = 45;

pub const SYSTEM_PROMPT: &str = "\
You draft a new personal financial plan in FinPlan, a Monte Carlo retirement planner, from what a person tells you about themselves and from the documents they upload: bank, brokerage and retirement statements, pay stubs, a tax return, CSV or OFX exports, screenshots. The draft starts empty. Nothing you write becomes a plan until the person has reviewed it and chosen Create & run, so write what you are confident of straight into the draft, and put everything else in front of them as a note to confirm.

What you are given. The first user message holds today's date (the plan's start date, set by the server), the person's description, the manifest of their documents (kind, filename, page count, the start of the first page, a birth-date hint), their library (return profiles with ids and asset classes, history presets, tax configurations and inflation profiles with ids), the contribution limits for the year, and the draft as it stands. Documents are not in the message: call read_document to read one, and only the pages you need. Text in documents has been redacted (Social Security numbers, account numbers except the last four); a statement's figures are still there.

How the draft is built. Every change to the draft is a note submitted with submit_suggestion. A note is one fact or decision, with the paths and steps of changes that put it in the plan (the same change format as in the reference: JSON-pointer edits on new_* targets, `{\"$new\": key}` references, and expect values). Two kinds:
- add: something to put in the plan: an account with its balance and holdings, an income, spending, a purchase, a loan, a parameter, a plan setting. Most notes are this. One path when there is one obvious way to write it; two to four paths, one recommended, when the person has a real choice.
- check: something you could not settle from the documents and description, where the person knows best (filing status, an unmatched screenshot, an account with no statement, a statement more than 45 days older than today, no Social Security modelled). Give a path with a clearly labelled estimate whenever a reasonable one exists, so the draft still runs; a check with no path says why in no_change_reason.
Never use kinds fix, stress or read: this is a draft, not a review, and no run exists.

Adding facts straight away. Set auto_add on an add note when it states a plain fact you read directly from a document or that the person answered, has exactly one path, and needs no choice: the balance and holdings of a statement, a salary from a pay stub, an answered question. The server applies it to the draft at once and it shows as Added. Leave auto_add off for anything that is a choice, an estimate or an assumption of yours; those stay open for the person to add. Never auto_add something a question still blocks.

Where a note goes. Give each note a `column`: portfolio (accounts, holdings, their return assumptions), plan (income, spending, events, parameters, settings) or to_confirm (what needs the person's eye). Check notes are to_confirm. A note you give a `key` (1-32 of a-z, 0-9, -) can be named by a question's `blocks`, or replaced later by a note with `replaces` set to its key; only an open note can be replaced.

Questions. Ask at most three questions in the whole draft, and only what neither the documents nor the description answer and that changes the plan: a bonus, one mix or a mix per fund, a retirement age. Each is ask_user with a short prompt and an answer type: choice (two to six labelled options), money, date or text. Prefer choices. A note that depends on an answer is submitted first, with `blocked_by` naming the question's key (or the question names the note's key in `blocks`); it is written for the answer you think most likely and cannot be added until the question is answered. ask_user ends your turn: the person answers, and you continue with their answers, which you may cite as evidence with ref answer. Submit every note that does not wait on an answer before you ask.

Settings every draft needs. The start date is today and is already set; never change it. Use a change on the `scenario` target to set the birth date (from a document's hint or the description, or ask), duration_years (to about age 95 unless the person says otherwise), inflation_profile_id and tax_config_id (from the library; if none fits the person's filing status and state, create one with a new_tax_config target and say so in a check note). Statement balances are treated as current as of today. If a statement is more than 45 days older than today, still use its figures but file a check note saying which statement is old and that the balance may have moved; do not move the start date.

Holdings and returns. A new asset's name is its ticker symbol alone (VBTLX), and its description is the fund's full name as the statement prints it (Vanguard Total Bond Market Index Fund Admiral Shares); a holding with no ticker gets a short name. Give every holding a return profile by asset class, never one per fund: call find_return_profile for a ticker (or a name, or a class). It returns the user's own profile for the class, else a new_return_profile change to place once for the class. Cash held in a bank or brokerage account uses the cash class. Amounts in accounts come from the statement's balance and positions; a position needs units, cost_basis and purchase_date, and where the statement has no cost basis say so in a check note rather than setting basis equal to value silently.

Facts that are templates. For a salary, an employer match, recurring spending, retirement, a home purchase, Social Security or a stress event, call expand_template: it lowers the fact to the changes that write it, with `$new` references you keep. Place the changes in a step of a note (or adjust them); use a different key_prefix for each expansion in one path so keys never collide.

Numbers and evidence. Every figure comes from a document, the description, an answer, or a tool's result; cite where in `evidence`: document (document_id, page, and the exact text quoted, checked against the document), description (an exact quote), answer (the question key), computed (a tool's name and the tool_use id of the call). An add note needs at least one. Never quote a statutory limit or do payment or growth arithmetic from memory: reference_facts, finance_calc, estimate_social_security and estimate_taxes are exact. Use summarize_transactions to find monthly spending from a transaction export rather than reading it line by line, match_account and reconcile to tie a statement to an account already in the draft.

Tools. read_document(id, pages?), expand_template(kind, params), find_return_profile(ticker | name | asset_class), simulate_draft(steps?) (whole-plan run of the draft with optional steps applied; it answers with the rates or with what stops the draft from running; use it sparingly, after the main accounts, income and spending are in, and once at the end), validate_changes(steps) (a free dry run of a note's steps against the draft as it stands), preflight() (whether the draft can run at all), reference_facts, finance_calc, estimate_social_security, estimate_taxes, goal_seek(parameter, metric, target) (searches one named parameter of the draft, such as a retirement age or a spending figure, for the value at which success_rate or funding_success_rate reaches a target like 0.9; about a dozen simulations, costs 4 previews and may be used twice; use it once the draft can run, for a single question a person will care about, and put its answer in a note), summarize_transactions, match_account, reconcile, ask_user and submit_suggestion. A scan or screenshot has no text: read_document shows you the image; read it, then call read_document again with `extraction` set to a faithful transcription of every figure and label you need (the server keeps that, redacted, and discards the image). Each result of a tool may be cited as computed evidence.

Order of work. Read the manifest and the description; read the documents that hold balances and pay, first pages first; find the birth date, filing state and status; write the settings, then the accounts, then income and spending, then retirement and Social Security; put unresolved items in check notes; ask your questions (after submitting what does not wait); simulate the draft once and, if it cannot run, fix what stops it. Then end your turn with one line saying what the draft holds and what needs the person's attention. A few well-founded notes beat many; one note per account, per income, per spending line.

How a note reads. The title is one specific sentence, at most 120 characters, naming the account, income or setting and the number that matters. The summary is one plain sentence shown open under the title: what the note adds or asks the person to confirm, without repeating the title. The reasoning is shown only when expanded: one to three plain sentences on what you saw and where; do not restate the summary. Use the person's own names, dollar amounts, ages and years, rounded (money to two or three significant figures in prose, exact in the changes). No disclaimers or generic advice.

Write against the draft as it stands: ids in the draft are database ids, and a note that creates accounts, assets or events reports the ids it created when accepted, so later notes can refer to them. A note that is only open (not added) has created nothing yet, so a later note cannot point at what it creates: put dependent changes in the same note.

";

/// The bodies a draft writes beyond what the review's reference already
/// declares. Built once.
pub fn extra_reference() -> &'static str {
    static REFERENCE: OnceLock<String> = OnceLock::new();
    REFERENCE.get_or_init(|| {
        let cfg = Config::new().with_large_int("number");
        let decls = [
            TemplateRequest::decl(&cfg),
            Template::decl(&cfg),
            TemplateKind::decl(&cfg),
            SalaryParams::decl(&cfg),
            EmployerMatchParams::decl(&cfg),
            RecurringExpenseParams::decl(&cfg),
            RetirementParams::decl(&cfg),
            RetirementSpending::decl(&cfg),
            HomePurchaseParams::decl(&cfg),
            SocialSecurityParams::decl(&cfg),
            MarketCrashParams::decl(&cfg),
            LargeExpenseParams::decl(&cfg),
            JobLossParams::decl(&cfg),
            When::decl(&cfg),
            RowRef::decl(&cfg),
            NewRef::decl(&cfg),
            ParameterBody::decl(&cfg),
            ParameterValueSpec::decl(&cfg),
            UpdateScenario::decl(&cfg),
            CreateProfile::decl(&cfg),
            DistributionSpec::decl(&cfg),
            CreateTaxConfig::decl(&cfg),
            Bracket::decl(&cfg),
        ]
        .join("\n");
        format!(
            "Targets a draft may also write (in addition to event, asset and account):\n\
             - {{\"parameter\": id}} and {{\"new_parameter\": \"<key>\"}}: a named plan parameter (a retirement age, a spending figure a formula can use as `$name`). Body: ParameterBody.\n\
             - \"scenario\" (the bare string, not an object) as the target: the plan's own settings, edited by pointer: /birth_date, /duration_years, /inflation_profile_id, /tax_config_id, /description. name and start_date cannot change. Its body is the object shown as the plan settings in the context: id, name, description, start_date, birth_date, duration_years, inflation_profile_id, tax_config_id; UpdateScenario lists the writable fields.\n\
             - {{\"new_return_profile\": \"<key>\"}} and {{\"new_tax_config\": \"<key>\"}}: a row in the person's library, created when the note is applied (op add, path \"\", the whole body: CreateProfile, CreateTaxConfig). Reference it in an id field with {{\"$new\": \"<key>\"}} in the same path. Library rows cannot be edited by a later change.\n\
             In a draft there is no run: statements above about a run, its paths or preview_changes do not apply; use simulate_draft.\n\n\
             Template and library bodies (TypeScript):\n```ts\n{decls}\n```"
        )
    })
}

/// The shared registry tools a draft is given, described for the model in
/// its own tool definitions.
pub fn shared_tools() -> Registry {
    Registry::only(&[
        crate::suggest::ai::tools::VALIDATE,
        crate::suggest::ai::tools::PREFLIGHT,
        crate::suggest::ai::tools::REFERENCE_FACTS,
        crate::suggest::ai::tools::FINANCE_CALC,
        crate::suggest::ai::tools::ESTIMATE_SOCIAL_SECURITY,
        crate::suggest::ai::tools::ESTIMATE_TAXES,
        crate::suggest::ai::tools::GOAL_SEEK,
    ])
}

fn question_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "key": {"type": "string", "description": "1-32 of a-z, 0-9, -; how the answer is cited and how notes refer to the question."},
            "prompt": {"type": "string", "description": "The question in one short sentence, at most 200 characters."},
            "answer_type": {"type": "string", "enum": ["choice", "money", "date", "text"]},
            "options": {
                "type": "array",
                "description": "choice only: two to six options.",
                "items": {
                    "type": "object",
                    "properties": {
                        "value": {"type": "string", "description": "1-40 characters; what comes back."},
                        "label": {"type": "string", "description": "What the person reads, at most 60 characters."}
                    },
                    "required": ["value", "label"]
                }
            },
            "blocks": {
                "type": "array",
                "description": "Keys of already submitted notes that wait on this answer.",
                "items": {"type": "string"}
            }
        },
        "required": ["key", "prompt", "answer_type"]
    })
}

fn draft_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": ASK_USER,
            "description": "Ask the person up to three questions you cannot answer from their documents and description, in total across the draft. Ends your turn: the draft waits for their answers, then you continue with them. Submit every note that does not depend on an answer first, and notes that do depend on one with blocked_by (or list their keys in `blocks`).",
            "input_schema": {
                "type": "object",
                "properties": {
                    "questions": {"type": "array", "items": question_schema(), "minItems": 1, "maxItems": 3}
                },
                "required": ["questions"]
            }
        }),
        json!({
            "name": READ_DOCUMENT,
            "description": "Read an uploaded document's (redacted) text, page by page. Without `pages` you get the first pages (a long document is cut, and the reply says which pages remain). For a scan or screenshot with no text, the reply shows you the image: read it, then call again with `extraction` to keep your transcription as the document's text and discard the image.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "description": "The document's id from the manifest."},
                    "pages": {"type": "string", "description": "1-based pages, e.g. \"2\" or \"2-4\"."},
                    "extraction": {"type": "string", "description": "Only for a scan or screenshot you have seen: everything you can read in it that the plan needs, as plain text."}
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": EXPAND_TEMPLATE,
            "description": "Lower a plain fact (a salary, an employer match, recurring spending, retirement, a home purchase, Social Security, a stress event) to the changes that write it in the plan, with the `$new` keys they create. Place the changes in a step of a note, adjusting if needed. See TemplateRequest and the params types in the reference.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["salary", "employer_match", "recurring_expense", "retirement", "home_purchase", "social_security", "market_crash", "large_expense", "job_loss"]},
                    "key_prefix": {"type": "string", "description": "Prepended to every key the template creates; use a different one per expansion in a path."},
                    "params": {"type": "object", "description": "The template's parameters (the type named for the kind in the reference)."}
                },
                "required": ["kind", "params"]
            }
        }),
        json!({
            "name": FIND_RETURN_PROFILE,
            "description": "Which return assumption a holding gets. Classifies a ticker (or a fund name, or an asset class) into a broad class and returns the person's own profile for that class, else a new_return_profile change for the engine's history for it (one per class, reuse its key for every fund of the class), else what to do when nothing fits. Never create a profile per fund.",
            "input_schema": super::profiles::schema()
        }),
        json!({
            "name": SIMULATE_DRAFT,
            "description": "Simulate the whole draft as it stands, with the steps you pass (optional, in order) applied on top, without saving anything: success rate, funding success rate and final net worth, or the reason the draft cannot run yet (a step that fails, or a compile error). Spends one of the limited previews.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "steps": {
                        "type": "array",
                        "description": "Optional steps, each a list of changes, applied in order before simulating.",
                        "items": {"type": "array", "items": crate::suggest::ai::prompt::change_schema()}
                    }
                }
            }
        }),
        json!({
            "name": SUMMARIZE_TRANSACTIONS,
            "description": "Monthly spending by category from a transaction export or statement (CSV, OFX, statement pages), with the typical month and outliers flagged, computed on the server. Use it for the plan's spending instead of reading transactions.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "document_id": {"type": "integer"},
                    "months": {"type": "integer", "description": "Only the most recent N months."}
                },
                "required": ["document_id"]
            }
        }),
        json!({
            "name": MATCH_ACCOUNT,
            "description": "Match a statement to an account already in the draft by institution, last four digits and balance; returns candidates with why each matched.",
            "input_schema": {
                "type": "object",
                "properties": {"document_id": {"type": "integer"}},
                "required": ["document_id"]
            }
        }),
        json!({
            "name": RECONCILE,
            "description": "A structured difference between a document's figures and the draft's: balances, share counts. Use it to see what a statement changes in accounts you already added.",
            "input_schema": {
                "type": "object",
                "properties": {"document_id": {"type": "integer"}},
                "required": ["document_id"]
            }
        }),
    ]
}

fn evidence_schema() -> Value {
    let mut schema = crate::suggest::ai::prompt::evidence_schema();
    // A draft has no run: only the sources it has.
    schema["items"]["properties"]["ref"]["enum"] =
        json!(["document", "description", "answer", "computed"]);
    schema["description"] = json!(
        "Where the note's facts come from: the document and exact text, the description quote, the answered question, or the tool call. An add note needs at least one."
    );
    schema
}

fn submit_tool() -> Value {
    json!({
        "name": SUBMIT,
        "description": "Add one note to the draft. The server checks every path's steps against the draft as it stands and the evidence against the documents, your answers and your tool calls, and either accepts the note (returning the diffs, and the ids of what an auto_add note created) or rejects it with the problems to fix. Accepted notes cannot be edited; replace an open one with `replaces`.",
        "input_schema": {
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": ["add", "check"]},
                "section": {"type": "string", "enum": ["portfolio", "plan"]},
                "column": {"type": "string", "enum": ["portfolio", "plan", "to_confirm"], "description": "Where the note groups on the Review board; defaults from section, and to_confirm for check notes."},
                "key": {"type": ["string", "null"], "description": "1-32 of a-z, 0-9, -; unique in the draft; lets a question's `blocks` or a later `replaces` name this note."},
                "title": {"type": "string", "description": "One specific sentence, at most 120 characters."},
                "summary": {"type": "string", "description": "One plain sentence, at most 200 characters, shown open under the title: what the note adds or asks the person to confirm. Do not repeat the title."},
                "reasoning": {"type": "string", "description": "The working behind the summary, shown when the person expands it: one to three plain sentences, at most 1200 characters, with what you saw and where. Do not restate the summary."},
                "evidence": evidence_schema(),
                "paths": crate::suggest::ai::prompt::paths_schema(),
                "no_change_reason": {
                    "type": ["string", "null"],
                    "description": "Only for a check note with no paths: one line on why there is nothing to change even as a labelled estimate."
                },
                "auto_add": {
                    "type": "boolean",
                    "description": "Apply the note to the draft now. Only for one plain fact, one path, read from a document or answered by the person, with evidence, not blocked."
                },
                "blocked_by": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Keys of questions this note waits on; each must be a question you have asked (ask_user, earlier in the same message or before)."
                },
                "replaces": {"type": ["string", "null"], "description": "The key of an open note of yours this one supersedes; it is removed."}
            },
            "required": ["kind", "section", "title", "summary", "reasoning", "evidence", "paths"]
        }
    })
}

/// The tools a drafting request offers: the shared registry, the draft's own,
/// then `submit_suggestion`.
pub fn tools(registry: &Registry) -> Value {
    let mut definitions = registry.definitions();
    definitions.extend(draft_tool_definitions());
    definitions.push(submit_tool());
    Value::Array(definitions)
}

// ── the per-draft context ───────────────────────────────────────────────────

/// A tax configuration of the user's, for the context.
#[derive(Debug, Clone)]
pub struct LibraryTax {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub state_rate: f64,
    pub capital_gains_rate: f64,
    pub early_withdrawal_penalty_rate: f64,
    pub standard_deduction: f64,
    pub age_65_extra_deduction: f64,
    pub brackets: usize,
}

/// An inflation profile of the user's, for the context.
#[derive(Debug, Clone)]
pub struct LibraryInflation {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
}

/// A note already in the draft, for the context of a resumed job.
#[derive(Debug, Clone)]
pub struct NoteLine {
    pub key: Option<String>,
    pub kind: String,
    pub title: String,
    /// `added`, `open` or `waiting on <keys>`.
    pub state: String,
}

/// Everything the opening message renders.
pub struct ContextParts<'a> {
    pub today: &'a str,
    pub description: &'a str,
    pub manifest: &'a [DocumentManifest],
    pub graph: &'a ScenarioGraph,
    pub profiles: &'a [LibraryProfile],
    /// `(id, name, first year, years of history)` per shipped history preset.
    pub presets: &'a [(String, String, i32, usize)],
    pub taxes: &'a [LibraryTax],
    pub inflation: &'a [LibraryInflation],
    pub notes: &'a [NoteLine],
    pub questions: &'a [DraftQuestion],
}

fn pct(v: f64) -> String {
    format!("{:.2}%", v * 100.0)
}

fn money(v: f64) -> String {
    let digits = format!("{:.0}", v.abs());
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{}${out}", if v < 0.0 { "-" } else { "" })
}

/// The opening user message's text: the per-draft context.
pub fn render_context(parts: &ContextParts<'_>) -> String {
    let mut out = String::new();
    let year: i32 = parts
        .today
        .get(0..4)
        .and_then(|y| y.parse().ok())
        .unwrap_or(facts::DEFAULT_YEAR);
    let _ = writeln!(
        out,
        "<today>{}</today>\nThe plan starts today. Statements more than {STALE_STATEMENT_DAYS} days before it are stale.\n",
        parts.today
    );
    let _ = writeln!(out, "<description>");
    if parts.description.trim().is_empty() {
        let _ = writeln!(out, "(none: work from the documents)");
    } else {
        let _ = writeln!(out, "{}", parts.description.trim());
    }
    let _ = writeln!(out, "</description>\n");

    let _ = writeln!(out, "<documents>");
    if parts.manifest.is_empty() {
        let _ = writeln!(out, "None uploaded.");
    }
    for d in parts.manifest {
        let _ = writeln!(
            out,
            "- #{} {:?} ({}), {} page{}, {}{}{}",
            d.id,
            d.filename,
            serde_json::to_value(d.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            d.pages,
            if d.pages == 1 { "" } else { "s" },
            serde_json::to_value(d.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            d.birth_date_hint
                .as_deref()
                .map(|b| format!(", birth date hint {b}"))
                .unwrap_or_default(),
            d.note
                .as_deref()
                .map(|n| format!(" ({n})"))
                .unwrap_or_default(),
        );
        if d.snippet.is_empty() {
            let _ = writeln!(
                out,
                "  first page: (no text; read_document shows the image)"
            );
        } else {
            let _ = writeln!(out, "  first page: {}", d.snippet);
        }
    }
    let _ = writeln!(out, "</documents>\n");

    let _ = writeln!(out, "<library>");
    let _ = writeln!(out, "Return profiles (asset class in brackets):");
    for p in parts.profiles {
        let _ = writeln!(
            out,
            "- #{} {}{}{}",
            p.id,
            p.name,
            p.asset_class
                .map(|c| format!(" [{}]", c.as_str()))
                .unwrap_or_default(),
            p.description
                .as_deref()
                .map(|d| format!(": {d}"))
                .unwrap_or_default()
        );
    }
    let _ = writeln!(
        out,
        "History presets the engine ships (for new_return_profile Bootstrap): {}.",
        parts
            .presets
            .iter()
            .map(|(id, name, start, n)| format!("{id} ({name}, {n} years from {start})"))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let _ = writeln!(out, "Tax configurations:");
    for t in parts.taxes {
        let _ = writeln!(
            out,
            "- #{} {}: {} federal brackets, standard deduction {} (+{} from 65), state {}, long-term capital gains {}, early-withdrawal penalty {}{}",
            t.id,
            t.name,
            t.brackets,
            money(t.standard_deduction),
            money(t.age_65_extra_deduction),
            pct(t.state_rate),
            pct(t.capital_gains_rate),
            pct(t.early_withdrawal_penalty_rate),
            t.description
                .as_deref()
                .map(|d| format!(" ({d})"))
                .unwrap_or_default()
        );
    }
    let _ = writeln!(out, "Inflation profiles:");
    for i in parts.inflation {
        let _ = writeln!(
            out,
            "- #{} {}{}",
            i.id,
            i.name,
            i.description
                .as_deref()
                .map(|d| format!(": {d}"))
                .unwrap_or_default()
        );
    }
    let _ = writeln!(out, "</library>\n");

    if let Some(limits) = facts::limits(year) {
        let _ = writeln!(
            out,
            "<limits year=\"{year}\" status=\"{}\">\n401(k)/403(b) employee deferral {}, catch-up at 50+ {}, at 60-63 {}; total additions {}; IRA {} (+{} at 50+); HSA {} self, {} family (+{} at 55+). Call reference_facts for anything else.\n</limits>\n",
            limits.status,
            money(limits.deferral),
            money(limits.catch_up_50),
            money(limits.catch_up_60_63),
            money(limits.annual_additions),
            money(limits.ira),
            money(limits.ira_catch_up_50),
            money(limits.hsa_self),
            money(limits.hsa_family),
            money(limits.hsa_catch_up_55),
        );
    }

    context::render_plan(&mut out, parts.graph);
    let _ = writeln!(
        out,
        "\nPlan settings (the `scenario` target's body): {}",
        crate::suggest::read::scenario(parts.graph)
    );

    if !parts.notes.is_empty() {
        let _ = writeln!(out, "\n<notes_so_far>");
        for n in parts.notes {
            let _ = writeln!(
                out,
                "- [{}{}] {} ({})",
                n.kind,
                n.key
                    .as_deref()
                    .map(|k| format!(" `{k}`"))
                    .unwrap_or_default(),
                n.title,
                n.state
            );
        }
        let _ = writeln!(out, "</notes_so_far>");
    }
    if !parts.questions.is_empty() {
        let _ = writeln!(out, "\n<questions_so_far>");
        for q in parts.questions {
            let _ = writeln!(
                out,
                "- `{}` ({}): {}{}",
                q.key,
                match q.answer_type {
                    AnswerType::Choice => "choice",
                    AnswerType::Money => "money",
                    AnswerType::Date => "date",
                    AnswerType::Text => "text",
                },
                q.prompt,
                q.answer
                    .as_ref()
                    .map(|a| format!(" -> answered: {a}"))
                    .unwrap_or_else(|| " -> not answered yet".into())
            );
        }
        let _ = writeln!(out, "</questions_so_far>");
    }
    out
}

/// What the first user turn asks, after the context.
pub fn task(resumed: bool) -> &'static str {
    if resumed {
        "Continue drafting this plan."
    } else {
        "Draft this person's plan from the description and documents above."
    }
}
