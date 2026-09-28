-- Draft scenarios (api::drafts): a scenario the AI-guided flow is still
-- writing. A draft is an ordinary scenario row, so the resolve, apply,
-- suggestion and chat plumbing works on it unchanged; `status` is what keeps it
-- out of scenario lists and plan-slot counts until Create & run makes it real.

ALTER TABLE scenarios ADD COLUMN status TEXT NOT NULL DEFAULT 'active'
    CHECK (status IN ('draft', 'active'));
CREATE INDEX idx_scenarios_status ON scenarios (status, updated_at);

-- Drafts written this calendar month (UTC), like monthly_goal_seeks. Spent when
-- a draft is started, after the plan slot is checked, and never refunded.
CREATE TABLE monthly_ai_drafts (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    month   TEXT NOT NULL,
    used    INTEGER NOT NULL,
    PRIMARY KEY(user_id, month)
);

-- A note on a draft has no run to be written against, so run_id is null there.
-- Everything else is unchanged.
CREATE TABLE suggestions_new (
    id             INTEGER PRIMARY KEY,
    scenario_id    INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    run_id         INTEGER,
    source         TEXT    NOT NULL CHECK (source IN ('rules', 'ai')),
    rule           TEXT,
    kind           TEXT    NOT NULL CHECK (kind IN ('fix', 'check', 'stress', 'read')),
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
    parent_id      INTEGER REFERENCES suggestions(id) ON DELETE SET NULL
);

-- Dropping `suggestions` deletes its rows first, and `suggestion_threads` (and
-- through it `suggestion_messages`) cascade from them. The migration runs in a
-- transaction, where foreign keys cannot be switched off, so those two are
-- copied aside and put back once the rebuilt table is in place. So is
-- `parent_id`, which the drop's own `SET NULL` clears in the copy.
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
