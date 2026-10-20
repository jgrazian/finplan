-- What every model pass spent, one row per pass (`usage::Ledger`): a review
-- pass, a chat turn on a note or a plan, or a draft segment. Written when the
-- pass ends however it ends — finished, failed, or superseded by a newer one —
-- so totals here are what the provider billed, unlike the per-feature usage
-- columns, which describe only the latest pass and are overwritten.
--
-- `user_id` is cleared rather than the row deleted when its user goes, so the
-- month's spend still adds up after an account is closed. `scenario_id` has
-- no foreign key for the same reason.
CREATE TABLE ai_usage (
    id                  INTEGER PRIMARY KEY,
    user_id             TEXT REFERENCES users(id) ON DELETE SET NULL,
    feature             TEXT    NOT NULL
                        CHECK (feature IN ('review', 'note_chat', 'plan_chat', 'draft')),
    scenario_id         INTEGER,
    model               TEXT    NOT NULL,
    -- The request that started the pass, as its log lines carry it; null for
    -- passes a background resume started.
    request_id          TEXT,
    turns               INTEGER NOT NULL,
    input_tokens        INTEGER NOT NULL,
    output_tokens       INTEGER NOT NULL,
    cache_read_tokens   INTEGER NOT NULL,
    cache_write_tokens  INTEGER NOT NULL,
    -- US dollars: OpenRouter's reported cost plus estimates from listed prices.
    cost_usd            REAL    NOT NULL,
    -- Answered requests with neither a reported cost nor a price to estimate
    -- from: when non-zero, `cost_usd` is an undercount.
    unpriced_turns      INTEGER NOT NULL,
    created_at          TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX ai_usage_created ON ai_usage (created_at);
CREATE INDEX ai_usage_user_created ON ai_usage (user_id, created_at);
