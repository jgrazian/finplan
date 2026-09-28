-- The model-written half of a review (api::review_ai), which runs in the
-- background after the rule notes are stored. `ai_job` names the pass that
-- may still write its notes: a newer review replaces it, and a pass whose job
-- is no longer the review's current one writes nothing.
ALTER TABLE suggestion_reviews ADD COLUMN ai_status TEXT NOT NULL DEFAULT 'off'
    CHECK (ai_status IN ('off', 'running', 'done', 'failed'));
ALTER TABLE suggestion_reviews ADD COLUMN ai_job TEXT;
ALTER TABLE suggestion_reviews ADD COLUMN ai_started_at TEXT;
ALTER TABLE suggestion_reviews ADD COLUMN ai_finished_at TEXT;
-- A short public message; never the API's raw error text.
ALTER TABLE suggestion_reviews ADD COLUMN ai_error TEXT;
-- Why the model stopped: finished, turn_limit, suggestion_limit, ...
ALTER TABLE suggestion_reviews ADD COLUMN ai_stop TEXT;

-- The review pass that wrote a suggestion; null for rule notes and for notes
-- a client posted itself. A new pass replaces only its predecessors' open
-- notes, never a client's.
ALTER TABLE suggestions ADD COLUMN review_job TEXT;
