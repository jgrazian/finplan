-- The most recent sweep of each scenario, so a reload does not lose it.
--
-- Analyses otherwise live in memory (see `analysis`): a sweep is a question
-- asked of a plan, cheap to ask again, and its answer is only interesting while
-- the question is on screen. A grid of a few hundred cells is expensive enough
-- that the screen it draws should survive a page reload, though, so the answer
-- — and only the newest one, per scenario — is kept here as the JSON the
-- results endpoint would have returned.
--
-- Stored as a document rather than as tables because nothing queries into it:
-- it is read back whole, by the one screen that drew it.

CREATE TABLE sweep_cache (
    scenario_id INTEGER PRIMARY KEY REFERENCES scenarios(id) ON DELETE CASCADE,
    user_id     TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- A serialized `SweepResults`.
    results     TEXT    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
