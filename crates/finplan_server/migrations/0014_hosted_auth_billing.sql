ALTER TABLE users ADD COLUMN email_verified_at TEXT;
CREATE TABLE auth_action_tokens (
 token_hash TEXT PRIMARY KEY,
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 purpose TEXT NOT NULL CHECK(purpose IN ('reset','verify')),
 email TEXT NOT NULL,
 expires_at INTEGER NOT NULL
);
CREATE INDEX auth_action_user ON auth_action_tokens(user_id, purpose);
CREATE TABLE subscriptions (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 provider TEXT NOT NULL,
 subscription_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('active','canceling','past_due','expired')),
 access_until INTEGER NOT NULL,
 revision INTEGER NOT NULL,
 UNIQUE(provider,subscription_id)
);
CREATE TABLE billing_receipts (
 provider TEXT NOT NULL,
 event_id TEXT NOT NULL,
 received_at TEXT NOT NULL DEFAULT (datetime('now')),
 PRIMARY KEY(provider,event_id)
);
CREATE TABLE monthly_goal_seeks (
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 month TEXT NOT NULL,
 used INTEGER NOT NULL,
 PRIMARY KEY(user_id,month)
);
CREATE TABLE editable_plans (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE
);
