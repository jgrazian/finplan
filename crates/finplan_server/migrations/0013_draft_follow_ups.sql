-- Follow-up messages to a finished draft (spec 16, design 2a's composer): each
-- one runs the drafting agent again on the draft as it stands, so a draft
-- takes only a few before its month's allowance would be meaningless.
ALTER TABLE draft_jobs ADD COLUMN follow_ups INTEGER NOT NULL DEFAULT 0;
