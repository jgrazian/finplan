-- Retried setup requests reuse their original transaction result.
CREATE TABLE setup_receipts (
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 request_id TEXT NOT NULL,
 request_json TEXT NOT NULL,
 scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
 PRIMARY KEY(user_id, request_id)
);
