-- Appearance is an account preference, not a scenario one: the plan is the
-- same plan whichever palette it is drawn in, so the choice follows the person
-- across every scenario they open rather than being stored with one of them.
--
-- 'system' is the default rather than 'light' because it is the only value
-- that is never wrong on a new account: the stylesheet answers
-- prefers-color-scheme itself, so the machine's existing answer is inherited
-- instead of overridden.
ALTER TABLE users ADD COLUMN theme_mode TEXT NOT NULL DEFAULT 'system'
    CHECK (theme_mode IN ('light', 'dark', 'system'));

-- Ground, ink and dividers are shared by every palette; only the accent ramp
-- turns, so one column names the whole set.
ALTER TABLE users ADD COLUMN accent TEXT NOT NULL DEFAULT 'blue'
    CHECK (accent IN ('blue', 'green', 'purple'));
