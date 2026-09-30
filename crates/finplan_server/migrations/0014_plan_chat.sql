-- Plan chat (api::plan_chat): one thread per plan on its Review tab, each user
-- message answered by the review model in the background. The model changes
-- nothing itself: a change it proposes is stored as an ordinary note on the
-- plan's review, for the user to apply, adjust or dismiss.
--
-- Columns as suggestion_threads: `status` is the current turn's, `job` names
-- the running turn, `request_id` the POST that started it, and the usage
-- columns describe the last finished turn.
CREATE TABLE plan_chat_threads (
    scenario_id   INTEGER PRIMARY KEY REFERENCES scenarios(id) ON DELETE CASCADE,
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
CREATE TABLE plan_chat_messages (
    id             INTEGER PRIMARY KEY,
    scenario_id    INTEGER NOT NULL
                   REFERENCES plan_chat_threads(scenario_id) ON DELETE CASCADE,
    role           TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    text           TEXT NOT NULL,
    suggestion_ids TEXT NOT NULL DEFAULT '[]',
    created_at     TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX plan_chat_messages_thread ON plan_chat_messages (scenario_id, id);

-- Plan chat messages sent this calendar month (UTC), like monthly_ai_drafts.
-- Spent when a message is accepted, and never refunded.
CREATE TABLE monthly_ai_plan_chats (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    month   TEXT NOT NULL,
    used    INTEGER NOT NULL,
    PRIMARY KEY(user_id, month)
);
