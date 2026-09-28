# AI-guided scenario creation (design 2a) and AI tool additions

Status: plan, not started. Design source: claude.ai/design project "FinPlan Web",
`New Scenario.dc.html`, turn 2 (2a intake, 2b documents-first, 2c/2d reviewing
the draft, 2e refreshing a plan from new statements). This document covers 2a and
what it needs; 2b–2e reuse the same pieces and are noted where they differ.

## What 2a is

A third way into New Scenario, next to Guided and Blank: **Describe & upload**.

- The user writes a short description of themselves (age, income, spending,
  goals, big purchases) and attaches files in the same message: bank and
  brokerage statements, 401(k)/IRA statements, pay stubs, a 1040, CSV or OFX
  exports, screenshots. Up to 10 files per draft on Free.
- The model reads everything and asks **at most three questions** it cannot
  answer from the documents, each with segmented answers ("No bonus / Yes, add
  one", "Per fund / One 60/40 mix") or a value.
- While it works, a draft panel fills on the right: accounts with balances,
  events, and a live count ("Drafting… 3 accounts, 6 events, 7 parameters so
  far"). Items that depend on an unanswered question show "waiting on question 1".
- Anything else it is unsure of becomes a **note to confirm** (filing status, an
  unmatched screenshot, no Social Security modeled).
- A retention choice: delete the documents once the draft is made, or keep them
  with the plan to refresh it later (2e).
- A quota line: "Free · 1 of 2 AI drafts left this month".
- **Review draft** opens 2c/2d: every account, event and parameter is a
  Review-style note with paths, steps and a server-rendered diff, in Portfolio /
  Plan / To confirm columns. "Add to draft" applies a note's changes to the
  draft; nothing becomes a real plan until **Create & run**.

The existing Review AI pipeline (`suggest/ai`, paths and steps, `resolve_steps`,
`apply_steps_sql`, suggestion chat, `domain::clone_into_mapped`) is the right
engine for this. What it lacks is below.

## Gaps in the current AI tooling

| # | Gap | Where |
|---|-----|-------|
| G1 | **Everything hangs off a run.** `ReviewContext.run_id`, preview pairing against the base run's `snapshot_json`, evidence refs (`ledger`, `account_series`, `stat`, `diagnostic`), and `suggestions.run_id NOT NULL`. A draft has no run, and no scenario until one is created. | `suggest/ai/context.rs`, `api/preview.rs`, `migrations/0005_suggestions.sql` |
| G2 | **Changes cannot touch parameters.** `ChangeTarget` is event/asset/account only. 2c shows `Parameters › $cash_floor ADDED` and `retirement_age`; `TriggerSpec::AgeParameter { parameter_id }` and expression refs (`$name`) need a parameter the same path created. | `suggest/mod.rs` (`ChangeTarget`), `domain/edit.rs` |
| G3 | **Changes cannot touch scenario settings.** Birth date, start date, duration, inflation profile and tax config are all out of reach, and a draft must set every one. | `api/scenarios.rs` (`UpdateScenario`) |
| G4 | **Return profiles and tax configs are user-level, and the AI cannot create either.** "Per fund" VTI and BND needs a bootstrap-preset profile per fund; "single, Colorado" needs a tax config. | `api/profiles.rs`, `api/taxes.rs` |
| G5 | **No document intake.** No upload route, storage, retention or parsing; axum's multipart feature is off. The transport is not the blocker: `openrouter-rs` 0.16 has `AnthropicContentPart::Image` and `::Document`. | new |
| G6 | **No way to ask the user.** The loop's only tools are `preview_changes` and `submit_suggestion`, and a pass runs to completion. Nothing suspends for answers and resumes, and a note cannot be blocked on a question. | `suggest/ai/mod.rs` (`converse`) |
| G7 | **No document or answer evidence.** Draft notes cite "from 1 document and your answer"; `Evidence` has no `document { id, page, excerpt }`, `answer { question }` or `description` variant. | `suggest/rules/mod.rs` (`Evidence`) |
| G8 | **Review-shaped prompt and gates.** `SYSTEM_PROMPT` and `task()` are review-specific. The materiality floor (base vs edit) is meaningless when the base is an empty plan. The duplicate check is against rule notes. Kinds `fix/check/stress/read` have no "add" kind for 2c's Portfolio and Plan columns. | `suggest/ai/prompt.rs`, `suggest/ai/mod.rs` |
| G9 | **No draft-level simulation.** The "est. 81%" figures need a whole-plan preview of the draft with a path applied, with no base run to pair against. | `api/preview.rs` |
| G10 | **Guided setup is not reusable.** `onboarding.rs` lowers `SetupPlan` straight to SQL through `RowBatch`, so its salary, 401(k), spending and retirement logic cannot be expressed as `Change`s the model could reuse. | `api/onboarding.rs` (`create`) |
| G11 | **No quota or plan-slot handling for drafts.** No monthly AI-draft counter (only `monthly_goal_seeks`). Free allows one editable plan, so a Free user who already has a plan would use up a draft and then fail at Create. | `billing.rs` |
| G12 | **Privacy.** A 1040 carries an SSN; statements carry account numbers. Today only the plan is sent, under OpenRouter's no-data-collection routing. | `suggest/ai/transport.rs` |

## Tools for the drafting agent

1. **`ask_user(questions[≤3])`.** Each question: `key`, `prompt`, answer type
   (`choice` with options, `money`, `date`, `text`), and the note keys it
   blocks. Ends the model's turn; the job goes to `awaiting_answers`. Answers
   return as the next user turn. Closes G6.
2. **`read_document(id, pages?)`.** Lazy page loading. The context carries only
   a manifest (kind, filename, page count, first-page text), so an 11-page 1040
   is not resent every turn. Server-side extracted text also makes excerpts
   checkable (G7).
3. **`expand_template(kind, params)`.** Lowers a high-level fact into
   `Change`s with `$new` refs that the model places in a note or adjusts.
   Kinds: salary, employer 401(k) match, recurring expense, retirement marker +
   retirement spending, home purchase (property + mortgage), Social Security.
   The model stops hand-writing trigger/effect trees. Built from guided setup's
   own lowering (G10).
4. **`find_return_profile(ticker | asset_class)`.** Classifies a holding into
   a broad class (US broad-market equity, international equity, aggregate
   bond, cash, …) and returns the profile to reuse for the whole class: the
   user's existing profile for that class first, else a history preset (a
   broad-market index fund such as VTI → the S&P 500 profile). It does not
   create a profile per fund; a `new_return_profile` body is returned only when
   no existing profile or preset fits the class, and then one per class.
5. **`simulate_draft(steps?)`.** Unpaired whole-plan run of the draft, with a
   path applied if given: success and funding rates, or the compile error that
   keeps the draft from running. Drives "est. N%" and "N notes need an answer
   before this plan can run" (G9).
6. **`submit_suggestion`, extended.** New kind `add`; document/answer evidence;
   `blocked_by: [question_key]`; `auto_add: bool`. Plain facts read straight
   from a document (the USAA balance) are applied immediately as "Added";
   choices and uncertain items stay open. That gives 2c's "12 notes · 7 added ·
   3 to confirm".

Parameter and scenario edits (G2/G3) belong in the existing change model, not in
new tools, so preview, diff and apply keep working unchanged.

## Additional tools for Review and drafting

### Expose what already exists (cheap)

| Tool | For | Why |
|---|---|---|
| `validate_changes(steps)` | Both | Resolve-only dry run: `expect`, pointers, bodies. `ReviewTools::resolve_steps` already does it; today a bad pointer costs one of the 8 previews. |
| `preview_paths(paths[])` | Both | Preview all of a note's 2–4 paths in one call on the same draws. Fewer turns, fair comparison. |
| `preflight()` | Draft | `/scenarios/{id}/preflight` and `/compile` exist. The agent needs "can this draft run at all?", which is 2c's banner. |
| `goal_seek(parameter, metric, target)` | Both | The existing goal-seek feature (`monthly_goal_seeks`) turns "retire later" into "retire at 43 reaches 90%". Paths become findings. Expensive: tight cap. |
| `sweep(parameter, values[])` | Review | `analysis/jobs.rs` backs trade-off notes ("each year earlier costs ~4 points"). Lower priority than goal-seek. |

### Run inspection (Review)

The context shows only the median-ranked path; failures live on the bad paths
(the `series=worst` follow-up is still open).

- **`inspect_path(rank: worst | p10 | p25 | median, years?)`**: cash flows and
  balances on a failing path. The most useful grounding for risk notes.
- **`failure_profile()`**: across failing iterations, the distribution of
  first-shortfall year, which account ran dry, which event failed. Current
  funding diagnostics are aggregates only.
- **`ledger(year, account?, event?)`**: transaction-level entries for one year
  on demand. `Evidence::Ledger` can already cite a ledger year; check whether
  the context already carries those entries before building this, and load
  them only on request to keep the prompt small.

### Deterministic calculators (both)

- **`reference_facts(topic, year)`**: contribution limits (401(k), IRA, HSA),
  RMD age, standard deduction, Social Security bend points, state tax rates.
  Today only `EMPLOYEE_401K_DEFERRAL_LIMIT_2026` is hardcoded, in onboarding;
  otherwise the model supplies statutory figures from memory.
- **`estimate_social_security(earnings, birth_year, claim_age)`**: the review
  prompt asks the model to estimate Social Security from the plan's earnings by
  a method it states. A benefit (PIA) calculator gives the same answer every
  time, for review notes and the draft's "to confirm" item alike.
- **`finance_calc(op)`**: loan payment (PMT), future value, pay period ↔ annual,
  age ↔ date. Covers 2d's "20% down drops the payment $470 a month" and the pay
  stub's "$5,461.54 biweekly → $142k".
- **`estimate_taxes(income, filing_status, state)`**: runs
  `finplan_core/src/taxes.rs` over one year's income, to reconcile a pay stub's
  net pay with what the plan models.

### Document tools (draft, and 2e refresh)

- **`summarize_transactions(document_id, months?)`**: monthly spending by
  category from OFX/CSV, outliers flagged. 2e's "$4,620 a month; August
  includes a $1,380 flight" should come from server-side aggregation, not the
  model reading hundreds of transactions.
- **`match_account(document_id)`**: match a statement to an existing account by
  institution, last four digits and balance. Needed for 2e updates and 2b's
  "Which account?".
- **`reconcile(document_id)`**: structured diff of a document's figures against
  the plan's (balances, share counts, pay rate). 2e's "Fix · Balance" notes
  follow directly.

### Cross-cutting: computed evidence

The prompt requires every number to come from the plan, the run or a preview.
Add **`Evidence::Computed { tool, call_id }`**, and keep each tool call's output
for the session, so submission checks verify figures a tool produced instead of
rejecting them as unsourced.

### Cost

Tool definitions sit in the cached prefix, so their fixed cost is small, but
each call is a turn. Raise `max_turns` modestly and keep simulation-backed tools
(`preview_paths`, `simulate_draft`, `goal_seek`, `sweep`) under the existing
preview budget rather than granting each its own.

## Phased plan

### Phase 0 — Draft scenarios (G1, G11)

- `scenarios.status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('draft','active'))`.
  A draft is a real scenario row, so `ScenarioGraph::load`, resolve, apply,
  suggestions and chat all work unchanged.
- Filter drafts out of scenario lists, `check_plan_slot` counting and
  `editable_plans`.
- Drafts are **not resumable**. A user has at most one draft; starting a new
  one discards the old. Cancel or closing the dialog deletes the draft and its
  documents immediately; a sweeper deletes any draft older than a short TTL
  (config, default 24 h) to cover closed tabs and crashes.
- Create & run: `check_plan_slot`, set `status = 'active'`, queue a run, then a
  normal Review pass.
- `monthly_ai_drafts(user_id, month, used)` like `monthly_goal_seeks`. Check the
  plan slot *before* spending a draft.
- Expose AI availability and the draft limits to the web. Today the web only
  learns AI is off per review (`Review.ai: null`), so add them to
  `Entitlements`: `ai_drafts: { enabled, remaining, max_files, max_bytes,
  max_pages } | null`, null when the server has no review model.
- Rejected alternative: a separate draft graph blob, which would duplicate the
  resolve/apply plumbing.

### Phase 1 — Change-model coverage (G2–G4)

- `ChangeTarget::{Parameter(id), NewParameter(key), Scenario}` and user-level
  `NewReturnProfile(key)`, `NewTaxConfig(key)` (rows created at Create/apply).
- Matching `domain/edit.rs` operations, plus account and asset delete (already
  on the Review follow-up list).
- Accept `{"$new": key}` in `parameter_id` fields and resolve new parameter
  names in expression refs.
- Diff labels in `suggest/diff.rs`; regenerate bindings.

### Phase 2 — Templates (G10)

- Refactor `onboarding.rs` to `SetupPlan -> Vec<Change>` applied through
  `apply_steps_sql`. Guided setup keeps its behavior; the same builders back
  `expand_template`.
- New templates: employer match (an `Income` `TaxFree` event into the 401(k)
  with a `Scale` expression on salary), home purchase (property + mortgage),
  Social Security, standard stress events.

### Phase 3 — Documents (G5, G12)

- `documents(id, user_id, scenario_id, filename, mime, sha256, pages, text,
  kind, status, retain, created_at)`. **Original files are never persisted**:
  uploads are held only while the draft is being written (memory or a
  per-draft temp directory, removed with the draft), and only the redacted
  extracted text and the hash are stored. So production database backups never
  hold statements or tax returns.
- `POST /drafts/{id}/documents` (multipart; enable axum's feature). Limits from
  config (see Configuration).
- Parsing: CSV → text table; OFX → small deterministic parser (balances,
  positions, transactions); PDFs with a text layer → extracted text.
- **Redaction before anything reaches the model**: mask SSNs/ITINs/EINs, full
  account and routing numbers (keep the last four for `match_account`), dates
  of birth other than the birth date the plan needs, and street addresses.
  Redacted text is what is sent and what is kept.
- Scanned PDFs and screenshots have no text layer. Attempt OCR (to redact, then
  send text); if OCR is not available or fails, send the image unredacted and
  say so in the consent text. Never keep the image: the kept "text" for it is
  the model's extraction.
- Retention choice (2a): "Delete once the draft is made" deletes the document
  rows on Create; "Keep with this plan" keeps the redacted text and hash on the
  new scenario for 2e.
- Routing: require zero-data-retention providers for draft requests (and keep
  data-collection deny). Verify early which OpenRouter providers accept
  image/PDF parts on `/messages` under `require_parameters` and ZDR; if none do
  for images, OCR becomes mandatory and unreadable images are rejected rather
  than sent.
- `summarize_transactions`, `match_account`, `reconcile` land here.

### Phase 4 — Drafting agent

- `suggest/ai/draft.rs`: its own system prompt; reuses `SEMANTICS` and
  `reference()`. Context: description, document manifest, the user's profiles,
  history presets, tax configs and inflation profiles with ids, today's date,
  current contribution limits, and the draft graph so far.
- The draft's `start_date` is today's real date, set by the server, not the
  model. Statement balances are treated as current as of today; a statement
  noticeably older than today gets a to-confirm note, not a moved start date.
- Tools: the drafting set above plus `validate_changes`, `preflight`,
  `reference_facts`, `finance_calc`, `estimate_social_security`.
- Suspend on `ask_user`; resume on `POST /drafts/{id}/answers`. Store the
  transcript with documents by reference; cache breakpoint after the documents.
- Gates: no materiality floor; evidence checked against extracted document
  text where a text layer exists; `Evidence::Computed` for tool figures.
- `GET /drafts/{id}` status for polling: state, counts by resource, open
  questions, blocked notes.

### Phase 5 — Web (2a, then 2c/2d)

- "Describe & upload" tab in `web/components/scenario/NewScenarioDialog.tsx`:
  message box, attachments, questions (existing `.seg` classes in
  `web/app/design-system.css`), polling draft panel, retention radio, quota line.
- The tab is **absent** (not disabled) when `Entitlements.ai_drafts` is null,
  leaving Guided and Blank exactly as they are today.
- Consent text above the attach control: what is sent, to whom, that it is
  redacted first (and when an image cannot be), and that originals are not kept.
- Cancel and closing the dialog delete the draft (not resumable).
- "Review draft" opens the draft scenario on `ReviewScreen`, grouped Portfolio /
  Plan / To confirm. "Add to draft" / "Leave out" / "Chat about this" map to the
  existing apply, dismiss and chat routes.
- Later: 2d age-ruler timeline; 2e refresh (mostly Phase 3 plus a normal Review
  pass with documents attached).

### Phase 6 — Review tool additions

In this order, independent of the draft work where possible:

1. `validate_changes`, `preview_paths`, `preflight` — nearly free.
2. `reference_facts`, `finance_calc`, `estimate_social_security` — the biggest
   cut in invented figures, for both efforts.
3. `inspect_path`, `failure_profile` — review notes grounded in real failures.
4. `summarize_transactions`, `match_account` — with Phase 3.
5. `goal_seek`, with a per-review budget.

## Configuration

All limits are server config (clap args with `FINPLAN_DRAFT_*` env vars, next
to `FINPLAN_REVIEW_*` in `suggest/ai/config.rs`), validated at startup and
reported through `Entitlements`. Initial values:

| Setting | Free | Pro |
|---|---|---|
| AI drafts per month | 2 | 20 |
| Files per draft | 10 | 25 |
| Bytes per draft | 25 MB | 100 MB |
| Pages per draft (PDF pages + images) | 60 | 200 |
| Draft TTL (abandoned) | 24 h | 24 h |

Model turns, previews and output tokens per draft reuse the review settings'
shape, with their own `FINPLAN_DRAFT_MAX_*` values.

## Decisions

Made (2026-09-28):

- **AI off → no Describe & upload.** When the server has no review model the
  option is hidden entirely; Guided and Blank are unchanged. No deterministic
  no-AI fallback.
- **Drafts are not resumable.** One draft per user; cancel, close or a new
  draft discards it; a TTL sweeper handles the rest.
- **Limits are configurable**, starting from the table above.
- **Privacy: do all of it.** Zero-data-retention routing, consent text,
  redaction before sending, OCR-then-redact for images where possible, and
  never persist original files — only redacted text and a hash.
- **Start date is today's real date**, not the newest statement's date.
- **Return profiles are reused by class, not created per fund.** Broad-market
  index funds map to the S&P 500 profile, and so on; a new profile is created
  only when nothing fits a class, one per class.
- **No plan DSL for now.** Changes stay JSON-pointer `Change`s; Phases 1–2 as
  written.

Still open (recommendation in brackets):

1. Draft storage: hidden scenario row vs a separate draft store [row].
2. Auto-add plain facts (2c's "7 added") vs the user adding every note
   [auto-add].
3. First release scope [2a plus a basic 2c on `ReviewScreen`; 2b/2d/2e later;
   design turn 1's Guided rebuild separate]. Description-only drafts with no
   files [allowed].
4. Free user with no plan slot left [refuse before drafting, so no draft is
   spent on a plan that cannot be created].
5. Model and cost ceiling per draft [set a ceiling first; consider a cheaper
   model for reading documents and the review model for writing notes].
6. Create & run triggers which review [rule review only; AI review one click
   away, on its own quota].
7. Unknown cost basis [a to-confirm note with a suggested estimate, not a
   silent basis = value].
