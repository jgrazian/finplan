-- A note's lead sentence, written apart from its reasoning.
--
-- The Review tab shows one sentence under a note's title and keeps the
-- reasoning behind a disclosure. Authors (the rules, the review and drafting
-- models) now write that sentence themselves rather than the client cutting
-- the reasoning at its first full stop. Notes stored before this have none;
-- the client falls back to the old cut for them.
ALTER TABLE suggestions ADD COLUMN summary TEXT;
