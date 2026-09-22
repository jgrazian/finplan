-- Private messages submitted from the application footer. Delivery and reply
-- workflows are deliberately separate: this table is the durable inbox.
CREATE TABLE contact_messages (
    id         INTEGER PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    topic      TEXT NOT NULL CHECK (topic IN ('question', 'feedback', 'bug_report')),
    message    TEXT NOT NULL CHECK (length(message) BETWEEN 1 AND 2000),
    status     TEXT NOT NULL DEFAULT 'new'
                    CHECK (status IN ('new', 'reviewed', 'closed')),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Supports the per-user submission window without scanning the inbox.
CREATE INDEX idx_contact_messages_user_created
    ON contact_messages(user_id, created_at DESC);

-- Ready for a future administrator queue without adding a public read route.
CREATE INDEX idx_contact_messages_status_created
    ON contact_messages(status, created_at);
