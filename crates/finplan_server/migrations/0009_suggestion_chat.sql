-- "Chat about this" (api::suggestion_chat): a thread of messages on one review
-- note, each user message answered by the review model in the background. A
-- suggestion the model adds from a thread points at the note it came from.
ALTER TABLE suggestions ADD COLUMN parent_id INTEGER
    REFERENCES suggestions(id) ON DELETE SET NULL;

-- One per note that has been chatted about. `status` is the current turn's:
-- `running` while the model answers (one turn at a time), `failed` when the
-- last turn could not finish (`error` is its public message), else `idle`.
-- `job` names the running turn; `request_id` is the POST that started it, so
-- its logs can be found. The usage columns describe the last finished turn.
CREATE TABLE suggestion_threads (
    suggestion_id INTEGER PRIMARY KEY REFERENCES suggestions(id) ON DELETE CASCADE,
    status        TEXT NOT NULL DEFAULT 'idle' CHECK (status IN ('idle', 'running', 'failed')),
    error         TEXT,
    job           TEXT,
    request_id    TEXT,
    started_at    TEXT,
    finished_at   TEXT,
    stop          TEXT,
    turns         INTEGER,
    input_tokens  INTEGER,
    output_tokens INTEGER,
    cost_usd      REAL,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- The thread, oldest first. `suggestion_ids` is a JSON array of the notes an
-- assistant message added.
CREATE TABLE suggestion_messages (
    id             INTEGER PRIMARY KEY,
    suggestion_id  INTEGER NOT NULL
                   REFERENCES suggestion_threads(suggestion_id) ON DELETE CASCADE,
    role           TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    text           TEXT NOT NULL,
    suggestion_ids TEXT NOT NULL DEFAULT '[]',
    created_at     TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX suggestion_messages_thread ON suggestion_messages (suggestion_id, id);
