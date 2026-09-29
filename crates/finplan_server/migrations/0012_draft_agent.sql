-- The drafting agent (suggest::ai::draft, api::draft_agent).
--
-- Notes written for a draft get a fourth kind, `add` ("put this in the plan"),
-- and four columns of their own: the note's author-chosen `note_key` (what a
-- question's `blocks` names), the question keys a note is still `blocked_by`,
-- the Review column it groups under (`board_column`: portfolio, plan or
-- to_confirm) and whether the agent applied it itself (`auto_added`).
-- A CHECK cannot be altered in SQLite, so `suggestions` is rebuilt as
-- 0010 rebuilt it, every existing row carried over.
CREATE TABLE suggestions_new (
    id             INTEGER PRIMARY KEY,
    scenario_id    INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    run_id         INTEGER,
    source         TEXT    NOT NULL CHECK (source IN ('rules', 'ai')),
    rule           TEXT,
    kind           TEXT    NOT NULL CHECK (kind IN ('add', 'fix', 'check', 'stress', 'read')),
    section        TEXT    NOT NULL CHECK (section IN ('portfolio', 'plan', 'results')),
    title          TEXT    NOT NULL,
    reasoning      TEXT    NOT NULL,
    evidence_json  TEXT    NOT NULL,
    fingerprint    TEXT    NOT NULL,
    status         TEXT    NOT NULL DEFAULT 'open'
                           CHECK (status IN ('open', 'applied', 'dismissed', 'confirmed')),
    created_at     TEXT    NOT NULL DEFAULT (datetime('now')),
    resolved_at    TEXT,
    review_job     TEXT,
    paths_json     TEXT    NOT NULL DEFAULT '[]',
    applied_path   TEXT,
    created_json   TEXT    NOT NULL DEFAULT '{}',
    parent_id      INTEGER REFERENCES suggestions(id) ON DELETE SET NULL,
    note_key       TEXT,
    blocked_by_json TEXT   NOT NULL DEFAULT '[]',
    board_column   TEXT    CHECK (board_column IN ('portfolio', 'plan', 'to_confirm')),
    auto_added     INTEGER NOT NULL DEFAULT 0
);

-- Dropping `suggestions` deletes its rows first, and `suggestion_threads` (and
-- through it `suggestion_messages`) cascade from them, so they are copied
-- aside and put back, exactly as in 0010.
CREATE TABLE suggestion_parents_kept AS
    SELECT id, parent_id FROM suggestions WHERE parent_id IS NOT NULL;
CREATE TABLE suggestion_threads_kept AS SELECT * FROM suggestion_threads;
CREATE TABLE suggestion_messages_kept AS SELECT * FROM suggestion_messages;

INSERT INTO suggestions_new
    (id, scenario_id, run_id, source, rule, kind, section, title, reasoning,
     evidence_json, fingerprint, status, created_at, resolved_at, review_job,
     paths_json, applied_path, created_json, parent_id)
SELECT id, scenario_id, run_id, source, rule, kind, section, title, reasoning,
       evidence_json, fingerprint, status, created_at, resolved_at, review_job,
       paths_json, applied_path, created_json, parent_id
  FROM suggestions;

DROP TABLE suggestions;
ALTER TABLE suggestions_new RENAME TO suggestions;
CREATE INDEX suggestions_by_scenario ON suggestions (scenario_id, status);
CREATE INDEX suggestions_by_fingerprint ON suggestions (scenario_id, fingerprint);

UPDATE suggestions
   SET parent_id = (SELECT parent_id FROM suggestion_parents_kept k WHERE k.id = suggestions.id)
 WHERE id IN (SELECT id FROM suggestion_parents_kept);
INSERT INTO suggestion_threads SELECT * FROM suggestion_threads_kept;
INSERT INTO suggestion_messages SELECT * FROM suggestion_messages_kept;
DROP TABLE suggestion_threads_kept;
DROP TABLE suggestion_messages_kept;
DROP TABLE suggestion_parents_kept;

-- One drafting job per draft. `state` is what the web polls: `drafting` while
-- the model is working, `awaiting_answers` once it has asked its questions
-- (`questions_json`, answers filled in as they arrive), `ready` when it has
-- finished (or stopped at a limit; `stop` says which) and `failed` with a public
-- `error`. `transcript_json` is the conversation so far with documents kept by
-- reference, so it can resume after answers; it, like the documents it points
-- at, goes when the draft becomes a plan. The usage columns total every
-- segment of the job.
CREATE TABLE draft_jobs (
    scenario_id     INTEGER PRIMARY KEY REFERENCES scenarios(id) ON DELETE CASCADE,
    job             TEXT    NOT NULL,
    state           TEXT    NOT NULL
                            CHECK (state IN ('drafting', 'awaiting_answers', 'ready', 'failed')),
    description     TEXT    NOT NULL,
    progress        TEXT,
    error           TEXT,
    stop            TEXT,
    questions_json  TEXT    NOT NULL DEFAULT '[]',
    transcript_json TEXT,
    simulation_json TEXT,
    turns           INTEGER NOT NULL DEFAULT 0,
    previews        INTEGER NOT NULL DEFAULT 0,
    input_tokens    INTEGER NOT NULL DEFAULT 0,
    output_tokens   INTEGER NOT NULL DEFAULT 0,
    cost_usd        REAL    NOT NULL DEFAULT 0,
    request_id      TEXT,
    started_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    finished_at     TEXT,
    updated_at      TEXT    NOT NULL DEFAULT (datetime('now'))
);
