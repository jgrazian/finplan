-- Tracing a model-written review pass (api::review_ai) from the database:
-- the request_id of the POST /review that started it (the same id its log
-- lines and the response's x-request-id header carry), and what the pass
-- spent once it finished. Null on reviews without a pass and on passes from
-- before this migration.
ALTER TABLE suggestion_reviews ADD COLUMN ai_request_id TEXT;
ALTER TABLE suggestion_reviews ADD COLUMN ai_turns INTEGER;
ALTER TABLE suggestion_reviews ADD COLUMN ai_input_tokens INTEGER;
ALTER TABLE suggestion_reviews ADD COLUMN ai_output_tokens INTEGER;
-- US dollars: OpenRouter's reported cost, or an estimate from the model's
-- listed prices where a reply reported none.
ALTER TABLE suggestion_reviews ADD COLUMN ai_cost_usd REAL;
