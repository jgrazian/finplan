//! The drafting agent: a tool-using conversation that writes a new plan from a
//! person's description and documents, as notes in a draft scenario.
//!
//! It is the review loop's sibling ([`super::generate`]) with different needs:
//!
//! - it suspends. `ask_user` registers up to three questions and ends the turn;
//!   [`run`] returns [`DraftStop::Suspended`] with the conversation so far as a
//!   [`Transcript`], and a later call resumes it with the answers;
//! - the transcript keeps documents by reference: the result of a tool that
//!   returned document text or figures is stored as a placeholder and its call,
//!   and [`Transcript::restore`] runs the calls again on resume, so a statement
//!   is never sitting in a stored conversation;
//! - its notes are written as they are accepted, through [`DraftHost::add_note`],
//!   not returned at the end, so the draft fills while the model works, and a
//!   note marked `auto_add` is applied to the draft at once;
//! - its gates differ: no run to check numbers against and no materiality
//!   floor; evidence is checked against document text, answers, the
//!   description and tool calls; a note is checked against the draft's own
//!   notes for duplicates.
//!
//! Like the review loop this module never touches the database or the engine:
//! everything it needs of the server is [`DraftHost`].

pub mod profiles;
pub mod prompt;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::RangeInclusive;
use std::time::Instant;

use base64ct::{Base64, Encoding};
use openrouter_rs::api::chat::CacheControl;
use openrouter_rs::api::messages::{
    AnthropicContentPart, AnthropicMessage, AnthropicMessageContent, AnthropicRole,
    AnthropicSystemTextBlock,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

pub use profiles::LibraryProfile;
pub use prompt::{
    ContextParts, LibraryInflation, LibraryTax, NoteLine, STALE_STATEMENT_DAYS, render_context,
    shared_tools,
};

use super::tools::{ToolEnv, ToolHost};
use super::{
    AiClient, AiDraft, AiError, AiPath, AiStep, Computed, MAX_EVIDENCE, MAX_NO_CHANGE_REASON,
    MAX_REASONING, MAX_TITLE, Observer, ReviewContext, SubmittedPath, ToolReport, Usage, add_usage,
    check_source_evidence, context, reports_cost, stop_label, turn_report,
};
use crate::api::suggestion_paths::{self, PathShape, StepShape, valid_key};
use crate::api::suggestions::DraftColumn;
use crate::documents::store::DocumentPage;
use crate::observability::{AiTool, AiToolOutcome};
use crate::suggest::rules::{Evidence, Kind, Section};
use crate::suggest::templates::TemplateRequest;
use crate::suggest::{Change, ChangeProblem, ChangeTarget};

/// Questions a whole draft may ask.
pub const MAX_QUESTIONS: usize = 3;
/// Notes a draft may hold at most; a guard, not a target.
pub const MAX_DRAFT_NOTES: usize = 40;
/// Pages one `read_document` returns when it is not told which.
const DEFAULT_PAGES: usize = 8;
/// Characters one `read_document` returns at most.
const MAX_READ_CHARS: usize = 60_000;
const MAX_PROMPT: usize = 200;
const MAX_OPTION_LABEL: usize = 60;
const MAX_OPTION_VALUE: usize = 40;
const MAX_TEXT_ANSWER: usize = 500;
/// Most changes in one step of a draft note, as in a review.
use super::tools::MAX_CHANGES;

// ── questions and answers ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AnswerType {
    /// One of `options`.
    Choice,
    /// A dollar amount.
    Money,
    /// A calendar date, `YYYY-MM-DD`.
    Date,
    /// Free text.
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QuestionOption {
    /// What comes back as the answer.
    pub value: String,
    /// What the person reads.
    pub label: String,
}

/// One of the agent's questions, and the person's answer once given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftQuestion {
    pub key: String,
    pub prompt: String,
    pub answer_type: AnswerType,
    /// The choices, for a `choice` question.
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    /// Keys of notes that wait on this question.
    #[serde(default)]
    pub blocks: Vec<String>,
    /// The answer once given: an option's value, a number of dollars, a date
    /// or text. Null while the question is open.
    #[serde(default)]
    #[ts(type = "string | number | null")]
    pub answer: Option<Value>,
}

impl DraftQuestion {
    pub fn is_open(&self) -> bool {
        self.answer.is_none()
    }
}

/// An answer as the person gave it, checked against its question and
/// normalized: a choice must be one of the options, money a finite number of
/// dollars (a string like "$1,200" is read), a date a real `YYYY-MM-DD`, text
/// non-empty and short.
pub fn validate_answer(question: &DraftQuestion, given: &Value) -> Result<Value, String> {
    match question.answer_type {
        AnswerType::Choice => {
            let value = given.as_str().unwrap_or_default();
            if question.options.iter().any(|o| o.value == value) {
                Ok(json!(value))
            } else {
                Err(format!(
                    "`{}`: pick one of {}",
                    question.key,
                    question
                        .options
                        .iter()
                        .map(|o| o.value.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
        }
        AnswerType::Money => {
            let amount = match given {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s
                    .trim()
                    .trim_start_matches('$')
                    .replace(',', "")
                    .parse::<f64>()
                    .ok(),
                _ => None,
            };
            match amount {
                Some(a) if a.is_finite() && a >= 0.0 => Ok(json!(a)),
                _ => Err(format!(
                    "`{}`: answer with an amount of dollars, zero or more",
                    question.key
                )),
            }
        }
        AnswerType::Date => given
            .as_str()
            .and_then(|s| s.trim().parse::<jiff::civil::Date>().ok())
            .map(|d| json!(d.to_string()))
            .ok_or_else(|| format!("`{}`: answer with a date, YYYY-MM-DD", question.key)),
        AnswerType::Text => {
            let text = given.as_str().map(str::trim).unwrap_or_default();
            if text.is_empty() || text.chars().count() > MAX_TEXT_ANSWER {
                Err(format!(
                    "`{}`: answer in 1 to {MAX_TEXT_ANSWER} characters",
                    question.key
                ))
            } else {
                Ok(json!(text))
            }
        }
    }
}

// ── the conversation, stored ────────────────────────────────────────────────

/// A tool call whose result is document-derived: kept by reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRef {
    pub tool: String,
    pub input: Value,
}

/// A result of the last assistant message not yet sent back.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pending {
    Result {
        part: AnthropicContentPart,
    },
    /// The result of an `ask_user` call: the answers, once there are any.
    Ask {
        tool_use_id: String,
    },
}

/// The conversation after the opening message, with what it takes to carry on
/// after a suspension.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Transcript {
    #[serde(default)]
    pub messages: Vec<AnthropicMessage>,
    #[serde(default)]
    pub pending: Vec<Pending>,
    /// Calls whose results are document-derived, by `tool_use` id.
    #[serde(default)]
    pub refs: BTreeMap<String, ToolRef>,
    /// Successful tool calls by `tool_use` id, to their tool: what
    /// `Evidence::Computed` may cite.
    #[serde(default)]
    pub computed: BTreeMap<String, String>,
    /// Everything spent so far in the job, across suspensions.
    #[serde(default)]
    pub usage: Usage,
}

const OMITTED: &str = "[document content is not kept in the stored conversation; it is read again from the draft's documents when the conversation resumes]";
const QUOTED: &str =
    "[document text: kept in the document and on the note, not in the conversation]";

impl Transcript {
    /// The copy to write down: results of referenced calls replaced by a
    /// placeholder, and a scan's transcription (which is the document's text,
    /// kept there) and the quotes of a submitted note (kept on the note) left
    /// out of the calls that supplied them.
    pub fn stored(&self) -> Transcript {
        let mut copy = self.clone();
        let omit = |part: &mut AnthropicContentPart, refs: &BTreeMap<String, ToolRef>| match part {
            AnthropicContentPart::ToolResult {
                tool_use_id,
                content,
                ..
            } if refs.contains_key(tool_use_id) => {
                *content = Some(AnthropicMessageContent::Text(OMITTED.into()));
            }
            AnthropicContentPart::ToolUse { name, input, .. } if name == prompt::READ_DOCUMENT => {
                if let Some(Value::Object(map)) = input
                    && map.contains_key("extraction")
                {
                    map.insert("extraction".into(), json!(QUOTED));
                }
            }
            // A note's quotes of a document stay on the note; the conversation
            // keeps only that it quoted.
            AnthropicContentPart::ToolUse { name, input, .. } if name == prompt::SUBMIT => {
                let evidence = input
                    .as_mut()
                    .and_then(|i| i.get_mut("evidence"))
                    .and_then(Value::as_array_mut);
                for entry in evidence.into_iter().flatten() {
                    if entry.get("ref").and_then(Value::as_str) == Some("document")
                        && let Some(excerpt) = entry.get_mut("excerpt")
                    {
                        *excerpt = json!(QUOTED);
                    }
                }
            }
            _ => {}
        };
        for message in &mut copy.messages {
            if let AnthropicMessageContent::Parts(parts) = &mut message.content {
                for part in parts {
                    omit(part, &self.refs);
                }
            }
        }
        for pending in &mut copy.pending {
            if let Pending::Result { part } = pending {
                omit(part, &self.refs);
            }
        }
        copy
    }

    /// Put document-derived results back: each referenced call is run again
    /// (from the draft's documents as they stand now) and its result replaces
    /// the placeholder. A document that is gone reads as such.
    pub async fn restore(&mut self, host: &dyn DraftHost) {
        let mut results: HashMap<String, String> = HashMap::new();
        for (id, call) in &self.refs {
            let text = match document_call(host, &call.tool, &call.input, true).await {
                Ok((text, _)) => text,
                Err(error) => format!("[no longer available: {error}]"),
            };
            results.insert(id.clone(), text);
        }
        let put = |part: &mut AnthropicContentPart| {
            if let AnthropicContentPart::ToolResult {
                tool_use_id,
                content,
                ..
            } = part
                && let Some(text) = results.get(tool_use_id)
                && matches!(content, Some(AnthropicMessageContent::Text(t)) if t == OMITTED)
            {
                *content = Some(AnthropicMessageContent::Text(text.clone()));
            }
        };
        for message in &mut self.messages {
            if let AnthropicMessageContent::Parts(parts) = &mut message.content {
                parts.iter_mut().for_each(put);
            }
        }
        for pending in &mut self.pending {
            if let Pending::Result { part } = pending {
                put(part);
            }
        }
    }
}

// ── what the loop needs of the server ───────────────────────────────────────

/// A document as `read_document` finds it.
pub enum DocumentRead {
    /// Redacted text, by page.
    Text {
        filename: String,
        kind: String,
        /// Pages the document has.
        pages_total: usize,
        pages: Vec<DocumentPage>,
    },
    /// A scan or screenshot: the held original, for the model to read.
    Held {
        filename: String,
        mime: String,
        bytes: Vec<u8>,
    },
}

/// The document tools the server computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentTool {
    SummarizeTransactions,
    MatchAccount,
    Reconcile,
}

/// A note of the draft as it stands, for duplicate checks and replacement.
#[derive(Debug, Clone)]
pub struct NoteSummary {
    pub key: Option<String>,
    pub kind: Kind,
    pub title: String,
    pub changes: Vec<Change>,
    /// Open notes can be replaced; applied ones cannot.
    pub open: bool,
}

/// An accepted note, ready to be stored.
#[derive(Debug, Clone)]
pub struct NewNote {
    pub draft: AiDraft,
    pub key: Option<String>,
    pub blocked_by: Vec<String>,
    pub column: DraftColumn,
    pub auto_add: bool,
    /// The key of an open note this one removes.
    pub replaces: Option<String>,
}

/// What became of a note the host stored.
#[derive(Debug, Clone)]
pub struct AddedNote {
    pub id: i64,
    /// Applied to the draft.
    pub added: bool,
    /// Why an `auto_add` note was left open instead.
    pub not_added: Option<String>,
    /// What the applied note created: `{key, kind, id}` per resource, so later
    /// notes can point at them.
    pub created: Vec<Value>,
    /// What the draft holds now.
    pub counts: Value,
}

/// The server, as the drafting loop sees it: the plan and its tools, the
/// documents, the library, and where notes go.
pub trait DraftHost: ToolHost {
    /// Whether the draft (and this job) still exist; false once the draft is
    /// deleted or another job replaced this one.
    fn alive(&self) -> super::BoxFuture<'_, bool>;

    /// A short line for the polling client: what the agent is doing.
    fn progress(&self, line: String) -> super::BoxFuture<'_, ()>;

    /// A document of this draft, or why it cannot be read.
    fn read_document(
        &self,
        id: i64,
        pages: Option<RangeInclusive<u32>>,
    ) -> super::BoxFuture<'_, Result<DocumentRead, String>>;

    /// Keep the model's reading of a scan as the document's text (redacted);
    /// returns the document's pages now.
    fn store_extraction(
        &self,
        id: i64,
        text: String,
    ) -> super::BoxFuture<'_, Result<Vec<String>, String>>;

    /// `summarize_transactions`, `match_account` or `reconcile` on a document.
    fn document_tool(
        &self,
        tool: DocumentTool,
        id: i64,
        months: Option<u32>,
    ) -> super::BoxFuture<'_, Result<Value, String>>;

    /// The user's return profiles, for `find_return_profile`.
    fn return_profiles(&self) -> Vec<LibraryProfile>;

    /// `simulate_draft`: the draft with `steps` applied, simulated.
    fn simulate(&self, steps: Vec<Vec<Change>>) -> super::BoxFuture<'_, Result<Value, String>>;

    /// The draft's notes as they stand.
    fn notes(&self) -> super::BoxFuture<'_, Vec<NoteSummary>>;

    /// Make open notes wait on a question.
    fn block_notes(
        &self,
        question: String,
        notes: Vec<String>,
    ) -> super::BoxFuture<'_, Result<(), String>>;

    /// Store an accepted note, replacing the one it names, and apply it if it
    /// is `auto_add`. An `Err` is the server's fault, not the model's.
    fn add_note(&self, note: NewNote) -> super::BoxFuture<'_, Result<AddedNote, String>>;
}

// ── running the loop ────────────────────────────────────────────────────────

/// What one segment of the conversation starts from.
pub struct DraftInput {
    /// The opening message's context ([`render_context`]).
    pub context: String,
    /// What the person wrote; what `Evidence::Description` is checked against.
    pub description: String,
    /// Each document's id and stored text (pages separated by form feeds), for
    /// evidence checks. `None` for a document with no text.
    pub documents: Vec<(i64, Option<String>)>,
    /// Every question asked so far in the job, answered or not.
    pub questions: Vec<DraftQuestion>,
}

/// What resuming after answers adds to the conversation.
pub struct Resume {
    /// The draft as it stands, for the model to continue from.
    pub state: String,
    /// Notes the answers unblocked.
    pub unblocked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DraftStop {
    /// The model ended its turn.
    Finished,
    /// `ask_user` was called: the job waits for answers.
    Suspended,
    /// The draft's turn budget is spent.
    TurnLimit,
    MaxTokens,
    Refused {
        category: Option<String>,
    },
    /// A later request failed after progress was made.
    Interrupted {
        error: String,
    },
    Unexpected {
        stop_reason: String,
    },
    /// The draft was deleted while the model worked.
    Cancelled,
}

pub fn stop_tag(stop: &DraftStop) -> &'static str {
    match stop {
        DraftStop::Finished => "finished",
        DraftStop::Suspended => "suspended",
        DraftStop::TurnLimit => "turn_limit",
        DraftStop::MaxTokens => "max_tokens",
        DraftStop::Refused { .. } => "refused",
        DraftStop::Interrupted { .. } => "interrupted",
        DraftStop::Unexpected { .. } => "unexpected",
        DraftStop::Cancelled => "cancelled",
    }
}

#[derive(Debug, Clone)]
pub struct DraftOutcome {
    pub stop: DraftStop,
    /// The conversation to store (already in its by-reference form).
    pub transcript: Transcript,
    /// Every question of the job, with the ones this segment asked.
    pub questions: Vec<DraftQuestion>,
    pub model: Option<String>,
    /// The model's closing line, if it wrote one.
    pub summary: Option<String>,
    /// Requests made by this segment alone.
    pub segment_turns: u32,
}

/// One tool call's result: the text the model reads, what else goes with it
/// (an image), and whether it failed.
struct Served {
    text: String,
    attachments: Vec<AnthropicContentPart>,
    is_error: bool,
    report: ToolReport,
}

impl Served {
    fn ok(text: impl Into<String>, tool: AiTool) -> Self {
        Self {
            text: text.into(),
            attachments: Vec::new(),
            is_error: false,
            report: ToolReport::new(tool, AiToolOutcome::Ok),
        }
    }

    fn error(text: impl Into<String>, tool: AiTool, outcome: AiToolOutcome) -> Self {
        Self {
            text: text.into(),
            attachments: Vec::new(),
            is_error: true,
            report: ToolReport::new(tool, outcome),
        }
    }

    fn part(self, tool_use_id: &str) -> AnthropicContentPart {
        let content = if self.attachments.is_empty() {
            AnthropicMessageContent::Text(self.text)
        } else {
            let mut parts = vec![AnthropicContentPart::text(self.text)];
            parts.extend(self.attachments);
            AnthropicMessageContent::Parts(parts)
        };
        AnthropicContentPart::ToolResult {
            tool_use_id: tool_use_id.to_owned(),
            content: Some(content),
            is_error: self.is_error.then_some(true),
            cache_control: None,
        }
    }
}

/// A note the session knows of, for keys and duplicates.
struct KnownNote {
    key: Option<String>,
    open: bool,
    existing: context::Existing,
}

struct Session<'a> {
    client: &'a AiClient,
    host: &'a dyn DraftHost,
    /// For the shared evidence checks: documents, answers, the description.
    context: ReviewContext,
    computed: HashMap<String, Computed>,
    questions: Vec<DraftQuestion>,
    notes: Vec<KnownNote>,
    usage: Usage,
    refs: BTreeMap<String, ToolRef>,
    /// `ask_user` was served: end the segment after this reply's calls.
    suspend: bool,
    rejected: u32,
}

/// Run the drafting conversation until the model ends its turn, asks a
/// question, or a limit is hit. `transcript` is empty for a new job and the
/// stored one for a resumed job, which `resume` continues; the host has
/// already stored every note the model got accepted by the time this returns.
/// `Err` only when the first request of the segment fails or cannot be read.
pub async fn run(
    client: &AiClient,
    host: &dyn DraftHost,
    observer: &dyn Observer,
    input: DraftInput,
    mut transcript: Transcript,
    resume: Option<Resume>,
) -> Result<DraftOutcome, AiError> {
    let settings = client.settings();
    let started = Instant::now();
    tracing::info!(
        event = "draft_ai.started",
        model = %settings.model,
        resumed = resume.is_some(),
        max_turns = settings.max_turns,
        max_previews = settings.max_previews,
        max_tokens = settings.max_tokens,
        turns_so_far = transcript.usage.turns,
        zdr = client.zero_data_retention(),
        context_bytes = input.context.len(),
    );

    transcript.restore(host).await;
    let known = host.notes().await;
    let mut context = ReviewContext::for_draft(input.context.clone())
        .with_documents(input.documents.iter().map(|(id, t)| (*id, t.as_deref())))
        .with_answers(
            input
                .questions
                .iter()
                .filter(|q| !q.is_open())
                .map(|q| q.key.as_str()),
        )
        .with_description(input.description.clone());
    // The answers are keys, the description a quote source: neither is empty
    // just because the person wrote nothing.
    if input.description.trim().is_empty() {
        context.description = None;
    }

    let mut session = Session {
        client,
        host,
        context,
        computed: transcript
            .computed
            .iter()
            .map(|(id, tool)| {
                (
                    id.clone(),
                    Computed {
                        tool: tool.clone(),
                        output: String::new(),
                    },
                )
            })
            .collect(),
        questions: input.questions.clone(),
        notes: known
            .iter()
            .map(|n| KnownNote {
                key: n.key.clone(),
                open: n.open,
                existing: existing(n.kind, &n.title, &n.changes),
            })
            .collect(),
        usage: std::mem::take(&mut transcript.usage),
        refs: std::mem::take(&mut transcript.refs),
        suspend: false,
        rejected: 0,
    };

    // The opening message, its cache breakpoint after the context.
    let mut opening_context = AnthropicContentPart::text(input.context.as_str());
    if let AnthropicContentPart::Text { cache_control, .. } = &mut opening_context {
        *cache_control = Some(CacheControl::ephemeral());
    }
    let mut messages = vec![AnthropicMessage::with_parts(
        AnthropicRole::User,
        vec![
            opening_context,
            AnthropicContentPart::text(prompt::task(resume.is_some())),
        ],
    )];
    messages.append(&mut transcript.messages);
    if let Some(resume) = &resume {
        messages.push(resume_message(
            &mut transcript.pending,
            &input.questions,
            resume,
        ));
    }

    let mut model = None;
    let mut summary = None;
    let mut segment_turns = 0u32;

    let stop = 'turns: loop {
        if !host.alive().await {
            break DraftStop::Cancelled;
        }
        if session.usage.turns >= settings.max_turns {
            break DraftStop::TurnLimit;
        }
        let request = client.request_with(
            system_blocks(),
            super::typed_tools(&prompt::tools(&client.registry)),
            &messages,
        );
        let reply = match request {
            Ok(request) => client.create(&request, observer).await,
            Err(error) => Err(error),
        };
        let (reply, seconds) = match reply {
            Ok(reply) => reply,
            Err(error) if segment_turns == 0 => {
                log_finished(&session, "failed", started);
                return Err(error);
            }
            Err(error) => {
                break DraftStop::Interrupted {
                    error: error.to_string(),
                };
            }
        };
        session.usage.turns += 1;
        segment_turns += 1;
        let price = if reports_cost(&reply) {
            None
        } else {
            client.price().await
        };
        let turn = turn_report(session.usage.turns, seconds, &reply, price);
        add_usage(&mut session.usage, &turn);
        observer.turn(&turn);
        model = reply.model.clone().or(model);
        tracing::info!(
            event = "draft_ai.turn",
            turn = turn.turn,
            latency_ms = (turn.seconds * 1000.0) as u64,
            stop_reason = stop_label(reply.stop_reason.as_deref().unwrap_or_default()),
            input_tokens = turn.input_tokens,
            output_tokens = turn.output_tokens,
            cache_read_input_tokens = turn.cache_read_input_tokens,
            cache_creation_input_tokens = turn.cache_creation_input_tokens,
            cost_usd = turn.cost.map(|(c, _)| c),
        );

        let stop_reason = reply.stop_reason.clone().unwrap_or_default();
        let content = reply.content;
        let said: Vec<&str> = content
            .iter()
            .filter_map(|part| match part {
                AnthropicContentPart::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if !said.is_empty() {
            summary = Some(said.join("\n").trim().to_owned()).filter(|s| !s.is_empty());
        }

        match stop_reason.as_str() {
            "end_turn" | "stop_sequence" => break DraftStop::Finished,
            "refusal" => {
                break DraftStop::Refused {
                    category: reply
                        .extra
                        .get("stop_details")
                        .and_then(|d| d.get("category"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                };
            }
            "max_tokens" => break DraftStop::MaxTokens,
            "tool_use" => {}
            "" if segment_turns == 1 => {
                return Err(AiError::Malformed("reply has no stop reason".into()));
            }
            other => {
                break DraftStop::Unexpected {
                    stop_reason: other.to_owned(),
                };
            }
        }

        // Questions first, so a note in the same message can wait on one; the
        // results still go back in call order.
        let calls: Vec<(&String, &String, Value)> = content
            .iter()
            .filter_map(|part| match part {
                AnthropicContentPart::ToolUse {
                    id, name, input, ..
                } => Some((id, name, input.clone().unwrap_or(Value::Null))),
                _ => None,
            })
            .collect();
        if calls.is_empty() {
            break DraftStop::Unexpected {
                stop_reason: "tool_use without tool calls".into(),
            };
        }
        let mut served: Vec<Option<Pending>> = (0..calls.len()).map(|_| None).collect();
        for pass in [true, false] {
            for (i, (id, name, input)) in calls.iter().enumerate() {
                if (name.as_str() == prompt::ASK_USER) != pass {
                    continue;
                }
                let started = Instant::now();
                let out = session.serve(id, name, input).await;
                let seconds = started.elapsed().as_secs_f64();
                observer.tool(out.report.tool, out.report.outcome, seconds);
                if out.report.tool == AiTool::Submit {
                    observer.submission(out.report.outcome == AiToolOutcome::Accepted);
                }
                tracing::info!(
                    event = "draft_ai.tool",
                    tool = out.report.tool.as_str(),
                    outcome = out.report.outcome.as_str(),
                    duration_ms = (seconds * 1000.0) as u64,
                    problems = out.report.problems,
                    problem_kinds = %out.report.problem_kinds.join(","),
                    previews = session.usage.previews,
                    notes = session.notes.len(),
                );
                served[i] = Some(if out.report.tool == AiTool::AskUser && !out.is_error {
                    Pending::Ask {
                        tool_use_id: (*id).clone(),
                    }
                } else {
                    Pending::Result { part: out.part(id) }
                });
            }
        }
        let results: Vec<Pending> = served.into_iter().flatten().collect();

        // The assistant turn goes back unchanged, thinking blocks included.
        messages.push(AnthropicMessage::with_parts(
            AnthropicRole::Assistant,
            content,
        ));
        if session.suspend {
            transcript.pending = results;
            break 'turns DraftStop::Suspended;
        }
        let parts: Vec<AnthropicContentPart> = results
            .into_iter()
            .filter_map(|p| match p {
                Pending::Result { part } => Some(part),
                Pending::Ask { .. } => None,
            })
            .collect();
        messages.push(AnthropicMessage::with_parts(AnthropicRole::User, parts));
    };

    log_finished(&session, stop_tag(&stop), started);
    // Everything after the opening message is the transcript.
    messages.remove(0);
    transcript.messages = messages;
    transcript.computed = session
        .computed
        .iter()
        .map(|(id, c)| (id.clone(), c.tool.clone()))
        .collect();
    transcript.refs = session.refs.clone();
    transcript.usage = session.usage.clone();
    Ok(DraftOutcome {
        stop,
        transcript: transcript.stored(),
        questions: session.questions,
        model,
        summary,
        segment_turns,
    })
}

fn system_blocks() -> Vec<AnthropicSystemTextBlock> {
    let mut last = AnthropicSystemTextBlock::text(prompt::extra_reference());
    // The static instructions stay cached whatever the draft holds.
    last.cache_control = Some(CacheControl::ephemeral());
    vec![
        AnthropicSystemTextBlock::text(prompt::SYSTEM_PROMPT),
        AnthropicSystemTextBlock::text(super::prompt::reference()),
        last,
    ]
}

fn log_finished(session: &Session<'_>, stop: &'static str, started: Instant) {
    let usage = &session.usage;
    tracing::info!(
        event = "draft_ai.finished",
        stop,
        notes = session.notes.len(),
        turns = usage.turns,
        previews = usage.previews,
        rejected = session.rejected,
        input_tokens = usage.input_tokens,
        output_tokens = usage.output_tokens,
        cache_read_input_tokens = usage.cache_read_input_tokens,
        cost_usd = usage.cost_usd,
        wall_ms = started.elapsed().as_millis() as u64,
    );
}

/// The user turn that carries a suspended conversation on: the results of the
/// last assistant message, `ask_user`'s being the answers, then the draft as it
/// stands.
fn resume_message(
    pending: &mut Vec<Pending>,
    questions: &[DraftQuestion],
    resume: &Resume,
) -> AnthropicMessage {
    let answers: Vec<Value> = questions
        .iter()
        .filter_map(|q| {
            q.answer
                .as_ref()
                .map(|a| json!({"key": q.key, "prompt": q.prompt, "answer": a}))
        })
        .collect();
    let reply = json!({
        "answers": answers,
        "unblocked_notes": resume.unblocked,
    })
    .to_string();
    let mut parts: Vec<AnthropicContentPart> = std::mem::take(pending)
        .into_iter()
        .map(|p| match p {
            Pending::Result { part } => part,
            Pending::Ask { tool_use_id } => {
                AnthropicContentPart::tool_result(tool_use_id, reply.clone())
            }
        })
        .collect();
    parts.push(AnthropicContentPart::text(format!(
        "The person answered. {}",
        resume.state
    )));
    AnthropicMessage::with_parts(AnthropicRole::User, parts)
}

// ── serving tools ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ReadInput {
    id: i64,
    #[serde(default)]
    pages: Option<Value>,
    #[serde(default)]
    extraction: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DocumentInput {
    document_id: i64,
    #[serde(default)]
    months: Option<u32>,
}

/// `"3"`, `"2-4"` or `3` as an inclusive page range.
fn parse_pages(value: &Value) -> Result<RangeInclusive<u32>, String> {
    let text = match value {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        _ => return Err("pages is like \"2\" or \"2-4\"".into()),
    };
    let bad = || "pages is like \"2\" or \"2-4\", counting from 1".to_owned();
    let (from, to) = match text.split_once('-') {
        Some((a, b)) => (a.trim().parse::<u32>(), b.trim().parse::<u32>()),
        None => {
            let page = text.parse::<u32>();
            (page.clone(), page)
        }
    };
    match (from, to) {
        (Ok(from), Ok(to)) if from >= 1 && to >= from => Ok(from..=to),
        _ => Err(bad()),
    }
}

fn render_pages(
    filename: &str,
    kind: &str,
    total: usize,
    pages: &[DocumentPage],
    asked_all: bool,
) -> String {
    let mut out = format!("Document {filename:?} ({kind}), {total} page(s).\n");
    let mut used = 0usize;
    let mut shown = 0usize;
    for page in pages {
        if asked_all && (shown >= DEFAULT_PAGES || used + page.text.len() > MAX_READ_CHARS) {
            break;
        }
        used += page.text.len();
        shown += 1;
        out.push_str(&format!(
            "\n--- page {} ---\n{}\n",
            page.page,
            page.text.trim()
        ));
    }
    let last_shown = pages
        .get(shown.saturating_sub(1))
        .map_or(0, |p| p.page as usize);
    if asked_all && last_shown < total {
        out.push_str(&format!(
            "\n[pages {}-{} not shown: call read_document again with pages]",
            last_shown + 1,
            total
        ));
    }
    if pages.is_empty() {
        out.push_str("\n[no such pages]");
    }
    out
}

/// The document-derived tools, shared by the loop and by
/// [`Transcript::restore`]. `replay` is a repeat of an earlier call: a held
/// image is not attached again.
async fn document_call(
    host: &dyn DraftHost,
    tool: &str,
    input: &Value,
    replay: bool,
) -> Result<(String, Vec<AnthropicContentPart>), String> {
    match tool {
        prompt::READ_DOCUMENT => {
            let input: ReadInput =
                serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))?;
            let range = input.pages.as_ref().map(parse_pages).transpose()?;
            match host.read_document(input.id, range.clone()).await? {
                DocumentRead::Text {
                    filename,
                    kind,
                    pages_total,
                    pages,
                } => Ok((
                    render_pages(&filename, &kind, pages_total, &pages, range.is_none()),
                    Vec::new(),
                )),
                DocumentRead::Held {
                    filename,
                    mime,
                    bytes,
                } => {
                    if replay {
                        return Ok((
                            format!(
                                "The image {filename:?} was shown here earlier and is not repeated."
                            ),
                            Vec::new(),
                        ));
                    }
                    let data = Base64::encode_string(&bytes);
                    let part = if mime == "application/pdf" {
                        AnthropicContentPart::Document {
                            source: json!({"type": "base64", "media_type": mime, "data": data}),
                            title: Some(filename.clone()),
                            context: None,
                            citations: None,
                            cache_control: None,
                        }
                    } else {
                        AnthropicContentPart::image_base64(mime, data)
                    };
                    Ok((
                        format!(
                            "Document {filename:?} has no text layer; the original is attached. Read it, then call read_document again with `extraction` set to a faithful transcription of every figure and label the plan needs; the server keeps that as the document's text and deletes the original."
                        ),
                        vec![part],
                    ))
                }
            }
        }
        prompt::SUMMARIZE_TRANSACTIONS | prompt::MATCH_ACCOUNT | prompt::RECONCILE => {
            let input: DocumentInput =
                serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))?;
            let which = match tool {
                prompt::SUMMARIZE_TRANSACTIONS => DocumentTool::SummarizeTransactions,
                prompt::MATCH_ACCOUNT => DocumentTool::MatchAccount,
                _ => DocumentTool::Reconcile,
            };
            let value = host
                .document_tool(which, input.document_id, input.months)
                .await?;
            Ok((value.to_string(), Vec::new()))
        }
        other => Err(format!("`{other}` is not a document tool")),
    }
}

/// A problem with a submission: a fixed kind for logs and the message the
/// model reads.
type Problem = (&'static str, Value);

/// The draft's own tools, the shared ones, and `submit_suggestion`.
impl Session<'_> {
    async fn serve(&mut self, call_id: &str, name: &str, input: &Value) -> Served {
        let served = match name {
            prompt::ASK_USER => self.ask_user(input).await,
            prompt::SUBMIT => self.submit(input).await,
            prompt::READ_DOCUMENT => self.read_document(call_id, input).await,
            prompt::SUMMARIZE_TRANSACTIONS | prompt::MATCH_ACCOUNT | prompt::RECONCILE => {
                self.document_tool(call_id, name, input).await
            }
            prompt::EXPAND_TEMPLATE => self.expand_template(input),
            prompt::FIND_RETURN_PROFILE => {
                match profiles::run(input, &self.host.return_profiles()) {
                    Ok(answer) => Served::ok(answer.to_string(), AiTool::FindReturnProfile),
                    Err(message) => {
                        Served::error(message, AiTool::FindReturnProfile, AiToolOutcome::Invalid)
                    }
                }
            }
            prompt::SIMULATE_DRAFT => self.simulate(input).await,
            _ => self.shared(name, input).await,
        };
        // Every successful call of a citable kind is kept for the session.
        if !served.is_error
            && !matches!(name, prompt::ASK_USER | prompt::SUBMIT)
            && served.report.tool != AiTool::Unknown
        {
            self.computed.insert(
                call_id.to_owned(),
                Computed {
                    tool: name.to_owned(),
                    output: served.text.clone(),
                },
            );
        }
        served
    }

    async fn shared(&mut self, name: &str, input: &Value) -> Served {
        let env = ToolEnv {
            host: self.host,
            previews_left: self
                .client
                .settings
                .max_previews
                .saturating_sub(self.usage.previews),
            goal_seeks_left: super::tools::goal_seek::MAX_GOAL_SEEKS
                .saturating_sub(self.usage.goal_seeks),
            failure_profile: None,
        };
        let Some((tool, out)) = self.client.registry.dispatch(name, input, &env).await else {
            return Served::error(
                format!("unknown tool `{name}`"),
                AiTool::Unknown,
                AiToolOutcome::Invalid,
            );
        };
        self.usage.previews += out.previews_spent;
        self.usage.goal_seeks += out.goal_seeks_spent;
        let mut served = if out.is_error {
            Served::error(self.client.scrub(&out.text), tool, out.outcome)
        } else {
            Served::ok(out.text, tool)
        };
        served.report.outcome = out.outcome;
        served.report.problems = out.problems;
        served
    }

    async fn read_document(&mut self, call_id: &str, input: &Value) -> Served {
        let tool = AiTool::ReadDocument;
        let parsed: ReadInput = match serde_json::from_value(input.clone()) {
            Ok(p) => p,
            Err(e) => {
                return Served::error(format!("bad input: {e}"), tool, AiToolOutcome::Invalid);
            }
        };
        if let Some(text) = parsed.extraction {
            self.host.progress("Reading a scan".into()).await;
            return match self.host.store_extraction(parsed.id, text).await {
                Ok(pages) => {
                    let characters: usize = pages.iter().map(String::len).sum();
                    self.context.documents.insert(parsed.id, pages);
                    Served::ok(
                        format!(
                            "Kept your reading as document #{}'s text ({characters} characters) and deleted the image. You can quote it as document evidence.",
                            parsed.id
                        ),
                        tool,
                    )
                }
                Err(message) => Served::error(message, tool, AiToolOutcome::Invalid),
            };
        }
        self.host
            .progress(format!("Reading document #{}", parsed.id))
            .await;
        match document_call(self.host, prompt::READ_DOCUMENT, input, false).await {
            Ok((text, attachments)) => {
                self.refs.insert(
                    call_id.to_owned(),
                    ToolRef {
                        tool: prompt::READ_DOCUMENT.into(),
                        input: input.clone(),
                    },
                );
                let mut served = Served::ok(text, tool);
                served.attachments = attachments;
                served
            }
            Err(message) => Served::error(message, tool, AiToolOutcome::Invalid),
        }
    }

    async fn document_tool(&mut self, call_id: &str, name: &str, input: &Value) -> Served {
        let tool = match name {
            prompt::SUMMARIZE_TRANSACTIONS => AiTool::SummarizeTransactions,
            prompt::MATCH_ACCOUNT => AiTool::MatchAccount,
            _ => AiTool::Reconcile,
        };
        match document_call(self.host, name, input, false).await {
            Ok((text, _)) => {
                self.refs.insert(
                    call_id.to_owned(),
                    ToolRef {
                        tool: name.into(),
                        input: input.clone(),
                    },
                );
                Served::ok(text, tool)
            }
            Err(message) => Served::error(message, tool, AiToolOutcome::Invalid),
        }
    }

    fn expand_template(&self, input: &Value) -> Served {
        let tool = AiTool::ExpandTemplate;
        let invalid = |message: String| Served::error(message, tool, AiToolOutcome::Invalid);
        let Some(kind) = input.get("kind").and_then(Value::as_str) else {
            return invalid("kind is required".into());
        };
        let mut request = match input.get("params") {
            Some(Value::Object(map)) => map.clone(),
            Some(Value::Null) | None => serde_json::Map::new(),
            Some(_) => return invalid("params is an object".into()),
        };
        request.insert("kind".into(), json!(kind));
        request.insert(
            "key_prefix".into(),
            input.get("key_prefix").cloned().unwrap_or(json!("")),
        );
        let request: TemplateRequest = match serde_json::from_value(Value::Object(request)) {
            Ok(r) => r,
            Err(e) => return invalid(format!("the {kind} parameters do not read: {e}")),
        };
        match request.expand() {
            Ok(expansion) => match serde_json::to_string(&expansion) {
                Ok(text) => Served::ok(text, tool),
                Err(e) => invalid(e.to_string()),
            },
            Err(e) => invalid(e.to_string()),
        }
    }

    async fn simulate(&mut self, input: &Value) -> Served {
        let tool = AiTool::SimulateDraft;
        if self.usage.previews >= self.client.settings.max_previews {
            return Served::error(
                "the simulation budget for this draft is used up",
                tool,
                AiToolOutcome::BudgetExhausted,
            );
        }
        let steps: Vec<Vec<Change>> = match input.get("steps") {
            None | Some(Value::Null) => Vec::new(),
            Some(value) => match serde_json::from_value(value.clone()) {
                Ok(steps) => steps,
                Err(e) => {
                    return Served::error(
                        format!("steps do not read: {e}"),
                        tool,
                        AiToolOutcome::Invalid,
                    );
                }
            },
        };
        if steps.len() > crate::api::suggestions::MAX_STEPS
            || steps.iter().any(|s| s.len() > MAX_CHANGES)
        {
            return Served::error(
                "too many steps or changes to simulate at once",
                tool,
                AiToolOutcome::Invalid,
            );
        }
        self.host
            .progress("Checking that the plan runs".into())
            .await;
        self.usage.previews += 1;
        match self.host.simulate(steps).await {
            Ok(result) => Served::ok(result.to_string(), tool),
            Err(message) => Served::error(self.client.scrub(&message), tool, AiToolOutcome::Error),
        }
    }

    // ── ask_user ────────────────────────────────────────────────────────────

    async fn ask_user(&mut self, input: &Value) -> Served {
        let tool = AiTool::AskUser;
        let invalid = |message: String| Served::error(message, tool, AiToolOutcome::Invalid);
        let Some(list) = input.get("questions").and_then(Value::as_array) else {
            return invalid("questions is a list of one to three questions".into());
        };
        let mut asked: Vec<DraftQuestion> = Vec::new();
        for raw in list {
            match serde_json::from_value::<DraftQuestion>(raw.clone()) {
                Ok(q) => asked.push(DraftQuestion { answer: None, ..q }),
                Err(e) => return invalid(format!("a question does not read: {e}")),
            }
        }
        if asked.is_empty() || asked.len() > 3 {
            return invalid("ask one to three questions".into());
        }
        if self.questions.len() + asked.len() > MAX_QUESTIONS {
            return invalid(format!(
                "a draft asks at most {MAX_QUESTIONS} questions in all, and {} have been asked. Decide on the most likely answer and put the rest in a check note",
                self.questions.len()
            ));
        }
        let mut keys: HashSet<&str> = self.questions.iter().map(|q| q.key.as_str()).collect();
        let mut problems = Vec::new();
        for q in &asked {
            if !valid_key(&q.key) {
                problems.push(format!(
                    "question key \"{}\" must be 1 to 32 of a-z, 0-9 and -",
                    q.key
                ));
            } else if !keys.insert(q.key.as_str()) {
                problems.push(format!("question key \"{}\" is used twice", q.key));
            }
            let prompt = q.prompt.trim();
            if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT || prompt.contains('\n') {
                problems.push(format!(
                    "`{}`: the prompt is one line of 1 to {MAX_PROMPT} characters",
                    q.key
                ));
            }
            match q.answer_type {
                AnswerType::Choice => {
                    if !(2..=6).contains(&q.options.len()) {
                        problems.push(format!("`{}`: a choice offers two to six options", q.key));
                    }
                    let mut values = HashSet::new();
                    for o in &q.options {
                        if o.value.trim().is_empty()
                            || o.value.chars().count() > MAX_OPTION_VALUE
                            || o.label.trim().is_empty()
                            || o.label.chars().count() > MAX_OPTION_LABEL
                            || !values.insert(o.value.as_str())
                        {
                            problems.push(format!(
                                "`{}`: options need a distinct value (at most {MAX_OPTION_VALUE} characters) and a label (at most {MAX_OPTION_LABEL})",
                                q.key
                            ));
                            break;
                        }
                    }
                }
                _ if !q.options.is_empty() => {
                    problems.push(format!("`{}`: only a choice has options", q.key));
                }
                _ => {}
            }
            for note in &q.blocks {
                if !self
                    .notes
                    .iter()
                    .any(|n| n.open && n.key.as_deref() == Some(note))
                {
                    problems.push(format!(
                        "`{}` blocks `{note}`, which is not an open note of yours; submit the note first (with that key)",
                        q.key
                    ));
                }
            }
        }
        if !problems.is_empty() {
            return invalid(problems.join("; "));
        }
        for q in &mut asked {
            q.prompt = q.prompt.trim().to_owned();
            q.blocks.sort();
            q.blocks.dedup();
        }
        for q in &asked {
            if !q.blocks.is_empty()
                && let Err(message) = self.host.block_notes(q.key.clone(), q.blocks.clone()).await
            {
                return Served::error(message, tool, AiToolOutcome::Error);
            }
        }
        self.questions.extend(asked);
        self.suspend = true;
        self.host.progress("Waiting for your answers".into()).await;
        Served::ok("Questions registered.", tool)
    }

    // ── submit_suggestion ───────────────────────────────────────────────────

    async fn submit(&mut self, input: &Value) -> Served {
        let tool = AiTool::Submit;
        if self.notes.len() >= MAX_DRAFT_NOTES {
            self.rejected += 1;
            return Served::error(
                format!(
                    "This draft already has {MAX_DRAFT_NOTES} notes, the most allowed. Stop here."
                ),
                tool,
                AiToolOutcome::BudgetExhausted,
            );
        }
        let submission: DraftSubmission = match serde_json::from_value(input.clone()) {
            Ok(s) => s,
            Err(e) => {
                self.rejected += 1;
                let mut served = Served::error(
                    format!("rejected: the note does not parse: {e}"),
                    tool,
                    AiToolOutcome::Invalid,
                );
                served.report.problems = 1;
                served.report.problem_kinds = vec!["parse"];
                return served;
            }
        };
        let note = match self.check(submission) {
            Ok(note) => note,
            Err(problems) => {
                self.rejected += 1;
                let mut served = Served::error("", tool, AiToolOutcome::Rejected);
                served.report.problems = problems.len();
                for (kind, _) in &problems {
                    if !served.report.problem_kinds.contains(kind) {
                        served.report.problem_kinds.push(kind);
                    }
                }
                let messages: Vec<Value> = problems.into_iter().map(|(_, m)| m).collect();
                served.text = json!({"accepted": false, "problems": messages}).to_string();
                return served;
            }
        };

        let diffs: Vec<Value> = note
            .draft
            .paths
            .iter()
            .map(|p| {
                json!({
                    "path": p.key,
                    "steps": p.steps.iter().map(|s| json!({"step": s.key, "diff": s.diff})).collect::<Vec<_>>(),
                })
            })
            .collect();
        let known = KnownNote {
            key: note.key.clone(),
            open: true,
            existing: existing(
                note.draft.kind,
                &note.draft.title,
                &note.draft.all_changes(),
            ),
        };
        let replaces = note.replaces.clone();
        let waiting = !note.blocked_by.is_empty();
        let column = note.column;
        match self.host.add_note(note).await {
            Ok(added) => {
                if let Some(key) = &replaces {
                    self.notes.retain(|n| n.key.as_deref() != Some(key));
                }
                self.notes.push(KnownNote {
                    open: !added.added,
                    ..known
                });
                let mut reply = json!({
                    "accepted": true,
                    "note_id": added.id,
                    "state": if added.added { "added" } else if waiting { "waiting_on_answer" } else { "open" },
                    "column": column,
                    "diffs": diffs,
                    "draft": added.counts,
                    "notes": self.notes.len(),
                });
                if !added.created.is_empty() {
                    reply["created"] = json!(added.created);
                }
                if let Some(why) = added.not_added {
                    reply["auto_add_failed"] = json!(why);
                }
                let mut served = Served::ok(reply.to_string(), tool);
                served.report.outcome = AiToolOutcome::Accepted;
                served
            }
            Err(message) => {
                // Not the model's to read: a key must never reach it.
                Served::error(self.client.scrub(&message), tool, AiToolOutcome::Error)
            }
        }
    }

    /// Every check a draft note must pass, collected rather than stopping at
    /// the first so the model can fix them in one go.
    fn check(&self, s: DraftSubmission) -> Result<NewNote, Vec<Problem>> {
        let mut messages: Vec<(&'static str, String)> = Vec::new();
        let mut problem = |kind: &'static str, text: String| messages.push((kind, text));

        if !matches!(s.kind, Kind::Add | Kind::Check) {
            problem("kind", "a draft note is an add or a check note".into());
        }
        if s.section == Section::Results {
            problem(
                "section",
                "a draft has no results section: use portfolio or plan".into(),
            );
        }
        let title = s.title.trim().to_owned();
        let reasoning = s.reasoning.trim().to_owned();
        if title.is_empty() || title.chars().count() > MAX_TITLE || title.contains('\n') {
            problem(
                "title",
                format!("title must be one line of 1 to {MAX_TITLE} characters"),
            );
        }
        if reasoning.is_empty() || reasoning.chars().count() > MAX_REASONING {
            problem(
                "reasoning",
                format!("reasoning must be 1 to {MAX_REASONING} characters"),
            );
        }
        let no_change_reason = s
            .no_change_reason
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_owned);
        if let Some(reason) = &no_change_reason
            && (reason.chars().count() > MAX_NO_CHANGE_REASON || reason.contains('\n'))
        {
            problem(
                "no_change_reason",
                format!(
                    "no_change_reason must be one line of at most {MAX_NO_CHANGE_REASON} characters"
                ),
            );
        }
        if s.kind == Kind::Check && s.paths.is_empty() && no_change_reason.is_none() {
            problem(
                "no_change",
                "a check note needs at least one path (an estimate the draft can run on); if it truly cannot be expressed as a change, say why in no_change_reason".into(),
            );
        }

        // Evidence: only what a draft has to cite.
        if s.evidence.len() > MAX_EVIDENCE {
            problem(
                "evidence_count",
                format!("at most {MAX_EVIDENCE} evidence entries"),
            );
        }
        if s.kind == Kind::Add && s.evidence.is_empty() {
            problem(
                "evidence",
                "an add note cites where its facts come from: a document, the description, an answer or a tool call".into(),
            );
        }
        for (i, e) in s.evidence.iter().enumerate() {
            let outcome = match e {
                Evidence::Document { .. }
                | Evidence::Answer { .. }
                | Evidence::Description { .. }
                | Evidence::Computed { .. } => {
                    check_source_evidence(&self.context, &self.computed, e)
                }
                _ => Err(
                    "a draft has no run: cite a document, the description, an answer or a tool call"
                        .into(),
                ),
            };
            if let Err(why) = outcome {
                problem("evidence", format!("evidence[{i}]: {why}"));
            }
        }

        // Keys and questions.
        let replaced = match s.replaces.as_deref() {
            None => None,
            Some(key) => match self.notes.iter().find(|n| n.key.as_deref() == Some(key)) {
                Some(n) if n.open => Some(key),
                Some(_) => {
                    problem(
                        "replaces",
                        format!(
                            "note `{key}` is already added to the draft and cannot be replaced"
                        ),
                    );
                    None
                }
                None => {
                    problem("replaces", format!("there is no note `{key}` of yours"));
                    None
                }
            },
        };
        if let Some(key) = s.key.as_deref() {
            if !valid_key(key) {
                problem("key", "key must be 1 to 32 of a-z, 0-9 and -".into());
            } else if self
                .notes
                .iter()
                .any(|n| n.key.as_deref() == Some(key) && Some(key) != replaced)
            {
                problem(
                    "key",
                    format!("key `{key}` is already used by another note"),
                );
            }
        }
        let mut blocked_by: Vec<String> = s.blocked_by.clone();
        blocked_by.sort();
        blocked_by.dedup();
        for key in &blocked_by {
            match self.questions.iter().find(|q| q.key == *key) {
                Some(q) if q.is_open() => {}
                Some(_) => problem(
                    "blocked_by",
                    format!("question `{key}` is already answered; use the answer instead"),
                ),
                None => problem(
                    "blocked_by",
                    format!(
                        "you have not asked a question `{key}`; call ask_user first (in the same message, before this note)"
                    ),
                ),
            }
        }
        if s.auto_add {
            if s.kind != Kind::Add {
                problem("auto_add", "only an add note can be auto_add".into());
            }
            if s.paths.len() != 1 {
                problem(
                    "auto_add",
                    "an auto_add note has exactly one path: a choice is left for the person".into(),
                );
            }
            if !blocked_by.is_empty() {
                problem(
                    "auto_add",
                    "a note waiting on a question cannot be auto_add".into(),
                );
            }
            if !s.evidence.iter().any(|e| {
                matches!(
                    e,
                    Evidence::Document { .. }
                        | Evidence::Answer { .. }
                        | Evidence::Description { .. }
                )
            }) {
                problem(
                    "auto_add",
                    "auto_add is for facts read from a document, the description or an answer: cite it".into(),
                );
            }
        }

        // Paths.
        let shapes: Vec<PathShape<'_>> = s
            .paths
            .iter()
            .map(|p| PathShape {
                key: &p.key,
                label: &p.label,
                reasoning: p.reasoning.as_deref(),
                recommended: p.recommended,
                steps: p
                    .steps
                    .iter()
                    .map(|st| StepShape {
                        key: &st.key,
                        title: &st.title,
                        reasoning: st.reasoning.as_deref(),
                        changes: &st.changes,
                    })
                    .collect(),
            })
            .collect();
        for shape in suggestion_paths::shape_problems(s.kind, &shapes, MAX_CHANGES) {
            problem("shape", shape);
        }
        for path in &s.paths {
            if let Some(estimate) = &path.estimate {
                for (name, value) in [
                    ("success_rate", estimate.success_rate),
                    ("funding_success_rate", estimate.funding_success_rate),
                ] {
                    if value.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
                        problem(
                            "estimate",
                            format!(
                                "path \"{}\": estimate.{name} must be a fraction between 0 and 1",
                                path.key
                            ),
                        );
                    }
                }
            }
        }

        // Duplicates: against the draft's own notes.
        let normalized = context::normalize(&title);
        let all: Vec<Change> = s.paths.iter().flat_map(|p| p.batches()).flatten().collect();
        let edits = draft_edits(&all);
        if let Some(twin) = self.notes.iter().find(|n| {
            !(replaced.is_some() && n.key.as_deref() == replaced)
                && (n.existing.title == normalized
                    || (!edits.is_empty()
                        && n.existing.edits == edits
                        && n.existing.kind == s.kind))
        }) {
            problem(
                "duplicate",
                format!(
                    "repeats a note already in the draft (\"{}\"); drop it, or replace that one with `replaces`",
                    twin.existing.title
                ),
            );
        }

        // Every path resolved against the draft as it stands, for its diffs.
        let mut change_problems: Vec<Problem> = Vec::new();
        let mut paths = Vec::with_capacity(s.paths.len());
        for path in s.paths {
            let batches = path.batches();
            let diffs = match self.host.resolve_steps(&batches) {
                Ok(diffs) => diffs,
                Err((step, found)) => {
                    let step_key = path.steps.get(step).map_or("", |s| s.key.as_str());
                    let kind = found.first().map_or("change", change_problem_kind);
                    change_problems.push((
                        kind,
                        json!({"path": path.key, "step": step_key, "problems": found}),
                    ));
                    continue;
                }
            };
            for (step, diff) in path.steps.iter().zip(&diffs) {
                if diff.is_empty() {
                    problem(
                        "no_effect",
                        format!(
                            "path \"{}\", step \"{}\": the changes leave the draft as it is",
                            path.key, step.key
                        ),
                    );
                }
            }
            paths.push(AiPath {
                key: path.key,
                label: path.label.trim().to_owned(),
                reasoning: path
                    .reasoning
                    .map(|r| r.trim().to_owned())
                    .filter(|r| !r.is_empty()),
                recommended: path.recommended,
                estimate: path.estimate,
                previewed: false,
                preview: None,
                steps: path
                    .steps
                    .into_iter()
                    .zip(diffs)
                    .map(|(step, diff)| AiStep {
                        key: step.key,
                        title: step.title.trim().to_owned(),
                        reasoning: step
                            .reasoning
                            .map(|r| r.trim().to_owned())
                            .filter(|r| !r.is_empty()),
                        changes: step.changes,
                        diff,
                    })
                    .collect(),
            });
        }

        if !messages.is_empty() || !change_problems.is_empty() {
            return Err(messages
                .into_iter()
                .map(|(kind, text)| (kind, Value::String(text)))
                .chain(change_problems)
                .collect());
        }
        let reasoning = match no_change_reason {
            Some(reason) if s.kind == Kind::Check && paths.is_empty() => {
                format!("{reasoning} {}.", reason.trim_end_matches('.'))
            }
            _ => reasoning,
        };
        let column = s.column.unwrap_or(match (s.kind, s.section) {
            (Kind::Check, _) => DraftColumn::ToConfirm,
            (_, Section::Portfolio) => DraftColumn::Portfolio,
            _ => DraftColumn::Plan,
        });
        Ok(NewNote {
            draft: AiDraft {
                kind: s.kind,
                section: s.section,
                title,
                reasoning,
                evidence: s.evidence,
                paths,
            },
            key: s.key,
            blocked_by,
            column,
            auto_add: s.auto_add,
            replaces: s.replaces,
        })
    }
}

/// Which fields a batch touches, as the duplicate check reads it. A review
/// names a created resource by its kind alone; a draft adds many resources of
/// one kind (three accounts, two parameters), so a created resource is named by
/// its kind and its name.
fn draft_edits(changes: &[Change]) -> std::collections::BTreeSet<String> {
    let mut edits = std::collections::BTreeSet::new();
    for change in changes {
        let created = matches!(
            change.target,
            ChangeTarget::NewEvent(_)
                | ChangeTarget::NewAsset(_)
                | ChangeTarget::NewAccount(_)
                | ChangeTarget::NewParameter(_)
                | ChangeTarget::NewReturnProfile(_)
                | ChangeTarget::NewTaxConfig(_)
        );
        let name = change
            .value
            .as_ref()
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str);
        match (created, change.path.as_str(), name) {
            (true, "", Some(name)) => {
                let kind = context::edits(std::slice::from_ref(change))
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                edits.insert(format!("{kind}:{}", context::normalize(name)));
            }
            _ => edits.extend(context::edits(std::slice::from_ref(change))),
        }
    }
    edits
}

/// A note as the duplicate check knows it.
fn existing(kind: Kind, title: &str, changes: &[Change]) -> context::Existing {
    context::Existing {
        kind,
        title: context::normalize(title),
        edits: draft_edits(changes),
    }
}

fn change_problem_kind(problem: &ChangeProblem) -> &'static str {
    // The review names the same kinds; a fixed set keeps logs bounded.
    match problem {
        ChangeProblem::Stale { .. } => "change_stale",
        ChangeProblem::BadPath { .. } => "change_bad_path",
        ChangeProblem::InvalidBody { .. } => "change_invalid_body",
        ChangeProblem::UnknownTarget { .. } => "change_unknown_target",
        ChangeProblem::UnsupportedOp { .. } => "change_unsupported_op",
        ChangeProblem::DuplicateKey { .. } => "change_duplicate_key",
        ChangeProblem::UnknownReference { .. } => "change_unknown_reference",
        ChangeProblem::WrongReferenceKind { .. } => "change_wrong_reference_kind",
        ChangeProblem::ReferenceCycle { .. } => "change_reference_cycle",
    }
}

/// What a draft submission looks like on the wire.
#[derive(Debug, Deserialize)]
struct DraftSubmission {
    kind: Kind,
    section: Section,
    #[serde(default)]
    column: Option<DraftColumn>,
    #[serde(default)]
    key: Option<String>,
    title: String,
    reasoning: String,
    #[serde(default)]
    no_change_reason: Option<String>,
    #[serde(default)]
    evidence: Vec<Evidence>,
    #[serde(default)]
    paths: Vec<SubmittedPath>,
    #[serde(default)]
    auto_add: bool,
    #[serde(default)]
    blocked_by: Vec<String>,
    #[serde(default)]
    replaces: Option<String>,
}
