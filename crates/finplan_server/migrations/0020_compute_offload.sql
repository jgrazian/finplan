-- Server offload (spec 19): a local plan's run, executed on the server's runner
-- workers without ever becoming a scenario or a `run_*` row.
--
-- The plan itself is never stored: the worker is handed the compiled snapshot in
-- memory, so all that lands here is what the poller needs and what the budget
-- was charged. Results are the serialized `RunResults`, kept for an hour after
-- the job ends (`expires_at`) and then purged by the maintenance loop.
CREATE TABLE compute_jobs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status      TEXT    NOT NULL DEFAULT 'queued'
                CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')),
    -- The sample the run was asked for; progress counts toward it.
    iterations  INTEGER NOT NULL,
    seed        INTEGER NOT NULL,
    -- Iterations finished so far.
    progress    INTEGER NOT NULL DEFAULT 0,
    -- Cost units charged to the budget at admission, and the month charged.
    cost        INTEGER NOT NULL,
    month       TEXT    NOT NULL,
    result_json TEXT,
    error       TEXT,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT,
    -- Set when the job ends; the row is purged once it passes.
    expires_at  TEXT
);
CREATE INDEX compute_jobs_user ON compute_jobs(user_id);
CREATE INDEX compute_jobs_expiry ON compute_jobs(expires_at) WHERE expires_at IS NOT NULL;

-- Offload cost units spent this calendar month (UTC), like monthly_goal_seeks.
-- Kept apart from the jobs so deleting or purging a job does not give the
-- budget back.
CREATE TABLE monthly_offload_spend (
    user_id TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    month   TEXT    NOT NULL,
    used    INTEGER NOT NULL DEFAULT 0 CHECK (used >= 0),
    PRIMARY KEY (user_id, month)
);
