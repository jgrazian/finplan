//! Chat turns: "Chat about this" on one review note, and plan chat on the
//! Review tab.
//!
//! The model reads what a review pass reads (the plan, the run and the notes
//! already on the board, as [`ReviewContext`]), then what the thread is about
//! ([`Subject`]: the note under discussion, or the plan as a whole) and the
//! thread so far, and answers the newest message. It has the review's two
//! tools: it may preview changes and submit new notes (at most
//! [`MAX_CHAT_SUGGESTIONS`] on a note, [`MAX_PLAN_CHAT_SUGGESTIONS`] on the
//! plan), checked exactly as a review's are. It never edits the plan: what it
//! proposes waits on the board for the user. Its closing text is the answer
//! the thread shows ([`answer_text`]).
//!
//! Pure over the model: no database. The caller renders the note, stores the
//! thread and the notes, and supplies the tools.

use std::time::Instant;

use openrouter_rs::api::messages::{AnthropicContentPart, AnthropicMessage, AnthropicRole};

use super::context::Existing;
use super::{
    AiClient, AiError, AiOutcome, Observer, ReviewContext, Settings, Stop, ToolHost, converse, text,
};
use crate::suggest::Change;
use crate::suggest::rules::Kind;

/// Most notes one chat turn on a note may add.
pub const MAX_CHAT_SUGGESTIONS: usize = 2;
/// Most notes one plan chat turn may add: a request like "retire at 60 and
/// sell the house" can take more than one.
pub const MAX_PLAN_CHAT_SUGGESTIONS: usize = 3;
/// Longest answer the thread keeps, in characters.
pub const MAX_ANSWER: usize = 1_500;

/// Who wrote a message in a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
}

/// What a thread is about, rendered for the model.
#[derive(Debug, Clone, Copy)]
pub enum Subject<'a> {
    /// The note under discussion, and the rest of the board.
    Note(&'a str),
    /// The plan as a whole: the notes on its board, and whether the plan
    /// changed since the run.
    Plan(&'a str),
}

impl Subject<'_> {
    fn tag(self) -> &'static str {
        match self {
            Subject::Note(_) => "note",
            Subject::Plan(_) => "plan",
        }
    }

    fn rendered(&self) -> &str {
        match self {
            Subject::Note(text) | Subject::Plan(text) => text,
        }
    }

    fn max_suggestions(self) -> usize {
        match self {
            Subject::Note(_) => MAX_CHAT_SUGGESTIONS,
            Subject::Plan(_) => MAX_PLAN_CHAT_SUGGESTIONS,
        }
    }
}

/// One chat turn's input.
pub struct ChatInput<'a> {
    /// The run the board reviews, as a review pass reads it.
    pub context: &'a ReviewContext,
    pub subject: Subject<'a>,
    /// The thread so far, oldest first, ending with the user's new message.
    /// A failed turn leaves two user messages in a row; they are read as one.
    pub history: &'a [(ChatRole, String)],
}

impl ReviewContext {
    /// Add notes already on the board to the ones the model must not repeat.
    pub fn with_notes<'n>(
        mut self,
        notes: impl IntoIterator<Item = (Kind, &'n str, Vec<Change>)>,
    ) -> Self {
        self.existing.extend(
            notes
                .into_iter()
                .map(|(kind, title, changes)| Existing::new(kind, title, &changes)),
        );
        self
    }
}

impl AiClient {
    /// This client with a chat turn's note cap. Shares the transport, and
    /// starts from the prices already looked up.
    fn for_chat(&self, max_suggestions: usize) -> AiClient {
        let settings = Settings {
            max_suggestions: self.settings.max_suggestions.min(max_suggestions),
            ..self.settings.clone()
        };
        let prices = self.prices.try_lock().ok().and_then(|cached| *cached);
        AiClient {
            settings,
            transport: self.transport.clone(),
            secret: self.secret.clone(),
            zdr: self.zdr,
            registry: self.registry.clone(),
            prices: tokio::sync::Mutex::new(prices),
        }
    }
}

/// Answer the thread's newest message. `Err` only when the first request
/// fails or cannot be read, as with a review.
pub async fn answer(
    client: &AiClient,
    input: &ChatInput<'_>,
    tools: &dyn ToolHost,
    observer: &dyn Observer,
) -> Result<AiOutcome, AiError> {
    let chat = client.for_chat(input.subject.max_suggestions());
    let settings = &chat.settings;
    let started = Instant::now();
    // Sizes and counts only: never the messages themselves.
    tracing::info!(
        event = "review_chat.model_started",
        run_id = input.context.run_id,
        model = %settings.model,
        thinking = settings.thinking,
        max_turns = settings.max_turns,
        max_suggestions = settings.max_suggestions,
        max_previews = settings.max_previews,
        subject = input.subject.tag(),
        messages = input.history.len(),
        context_bytes = input.context.text.len(),
        subject_bytes = input.subject.rendered().len(),
    );
    let opening = opening(input, settings.max_suggestions);
    converse(&chat, input.context, tools, observer, opening, started).await
}

/// What a chat turn asks, after the plan and the run, before the subject.
fn instructions(subject: Subject<'_>, max_suggestions: usize) -> String {
    match subject {
        Subject::Note(_) => note_instructions(max_suggestions),
        Subject::Plan(_) => plan_instructions(max_suggestions),
    }
}

fn plan_instructions(max_suggestions: usize) -> String {
    format!(
        "The user is talking with you about their plan as a whole, in a thread on its \
         Review tab; the notes already on the review are listed next. They may ask a \
         question about the plan or the run, or ask for a change (\"retire at 62\", \
         \"add a $40,000 car in 2029\", \"my 401(k) is now $310,000\"). Answer their newest \
         message directly and specifically, citing the plan's and the run's own figures. \
         Plain text only: no markdown headings, at most about 1,200 characters. You \
         cannot change the plan yourself. For a change the user asks for, or one that \
         would clearly answer their question, preview it with preview_changes, then \
         submit it with submit_suggestion as a note (at most {max_suggestions} this \
         turn); the user decides whether to apply it, adjust it or dismiss it. Write \
         exactly the change asked for; when the request is ambiguous, ask rather than \
         guess. Describe consequences, not what the user should do: offer options and \
         what each does to the plan, and leave the choice to them. Do not recommend \
         specific securities or funds. Do not resubmit a note already on the review. \
         Finish by saying whether you added a note and what it would change, or why not."
    )
}

fn note_instructions(max_suggestions: usize) -> String {
    format!(
        "The user is asking about one review note, shown next, in a short thread. \
         Answer their newest message directly and specifically, citing the plan's and \
         the run's own figures. Plain text only: no markdown headings, at most about \
         1,200 characters. If a concrete change to the plan would address it, preview \
         it with preview_changes, then submit it with submit_suggestion as a new note \
         (at most {max_suggestions} this turn); it is shown linked to this note. Do not \
         resubmit this note or any other already on the board. When the note flags a \
         problem without a change, prefer proposing one over only explaining it. Finish \
         your answer by saying whether you added a suggestion and what it does, or why not."
    )
}

/// The conversation as the model reads it: the plan, the run and the
/// subject in the first user turn, then the thread with roles alternating.
fn opening(input: &ChatInput<'_>, max_suggestions: usize) -> Vec<AnthropicMessage> {
    let mut messages = Vec::new();
    let mut user: Vec<AnthropicContentPart> = vec![
        text(input.context.text.as_str()),
        text(instructions(input.subject, max_suggestions)),
        text(input.subject.rendered()),
    ];
    for (role, message) in input.history {
        match role {
            ChatRole::User => user.push(text(format!("The user writes:\n{message}"))),
            ChatRole::Assistant => {
                if !user.is_empty() {
                    messages.push(AnthropicMessage::with_parts(
                        AnthropicRole::User,
                        std::mem::take(&mut user),
                    ));
                }
                messages.push(AnthropicMessage::with_parts(
                    AnthropicRole::Assistant,
                    vec![text(message.as_str())],
                ));
            }
        }
    }
    if !user.is_empty() {
        messages.push(AnthropicMessage::with_parts(AnthropicRole::User, user));
    }
    messages
}

/// The answer the thread keeps: the model's closing text, plain and capped;
/// a fixed line when it wrote none, by how the turn ended.
pub fn answer_text(outcome: &AiOutcome) -> String {
    let said = outcome
        .summary
        .as_deref()
        .map(plain)
        .filter(|s| !s.is_empty());
    let text = said.unwrap_or_else(|| {
        let added = outcome.drafts.len();
        let why = match outcome.stop {
            Stop::Finished => "I have nothing to add.",
            Stop::TurnLimit | Stop::SuggestionLimit => {
                "I ran out of steps before writing an answer. Ask again to continue."
            }
            Stop::MaxTokens => "My answer ran too long to finish. Try a narrower question.",
            Stop::Refused { .. } => "I can't help with that question.",
            Stop::Interrupted { .. } | Stop::Unexpected { .. } => {
                "I stopped before finishing an answer. Ask again to retry."
            }
        };
        match added {
            0 => why.to_owned(),
            1 => format!("{why} I added one suggestion."),
            n => format!("{why} I added {n} suggestions."),
        }
    });
    cap(&text, MAX_ANSWER)
}

/// Markdown headings read as plain lines; surrounding space trimmed.
fn plain(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                trimmed.trim_start_matches('#').trim_start()
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn cap(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max - 1).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::ai::Usage;

    fn outcome(summary: Option<&str>, stop: Stop) -> AiOutcome {
        AiOutcome {
            drafts: Vec::new(),
            stop,
            usage: Usage::default(),
            model: None,
            summary: summary.map(str::to_owned),
        }
    }

    #[test]
    fn the_answer_is_plain_and_capped() {
        let answer = answer_text(&outcome(
            Some("## Why\nSocial Security is missing.\n### Fix\nAdd it."),
            Stop::Finished,
        ));
        assert_eq!(answer, "Why\nSocial Security is missing.\nFix\nAdd it.");

        let long = "x".repeat(MAX_ANSWER + 50);
        let answer = answer_text(&outcome(Some(&long), Stop::Finished));
        assert_eq!(answer.chars().count(), MAX_ANSWER);
        assert!(answer.ends_with('…'));
    }

    #[test]
    fn a_turn_without_text_still_answers() {
        assert_eq!(
            answer_text(&outcome(None, Stop::TurnLimit)),
            "I ran out of steps before writing an answer. Ask again to continue."
        );
        assert_eq!(
            answer_text(&outcome(Some("  "), Stop::Finished)),
            "I have nothing to add."
        );
    }

    #[test]
    fn plan_chat_proposes_notes_and_leaves_the_choice_to_the_user() {
        let said = instructions(Subject::Plan("BOARD"), MAX_PLAN_CHAT_SUGGESTIONS);
        assert!(said.contains("cannot change the plan yourself"));
        assert!(said.contains("at most 3 this turn"));
        assert!(said.contains("leave the choice to them"));
        assert!(!instructions(Subject::Note("NOTE"), 2).contains("as a whole"));
        assert_eq!(
            Subject::Plan("").max_suggestions(),
            MAX_PLAN_CHAT_SUGGESTIONS
        );
    }

    #[test]
    fn the_thread_alternates_and_ends_on_the_user() {
        let context = ReviewContext {
            run_id: 1,
            text: "PLAN".into(),
            events: Default::default(),
            accounts: Default::default(),
            years: None,
            dates: Default::default(),
            existing: Vec::new(),
            failure_profile: None,
            documents: Default::default(),
            answers: Default::default(),
            description: None,
        };
        let history = vec![
            (ChatRole::User, "first".to_owned()),
            (ChatRole::Assistant, "answer".to_owned()),
            // A failed turn: two user messages in a row.
            (ChatRole::User, "second".to_owned()),
            (ChatRole::User, "again".to_owned()),
        ];
        let input = ChatInput {
            context: &context,
            subject: Subject::Note("NOTE"),
            history: &history,
        };
        let messages = opening(&input, 2);
        let roles: Vec<_> = messages.iter().map(|m| m.role.clone()).collect();
        assert_eq!(
            roles,
            vec![
                AnthropicRole::User,
                AnthropicRole::Assistant,
                AnthropicRole::User
            ]
        );
        let wire = serde_json::to_string(&messages).unwrap();
        assert!(wire.contains("PLAN") && wire.contains("NOTE"));
        assert!(wire.contains("The user writes:\\nsecond") && wire.contains("again"));
    }
}
