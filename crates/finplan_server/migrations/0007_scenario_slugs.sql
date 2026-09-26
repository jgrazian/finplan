-- Public browser references remain stable when a scenario is renamed.
-- The letter prefix makes slugs unambiguous with legacy numeric bookmarks.
ALTER TABLE scenarios ADD COLUMN slug TEXT NOT NULL DEFAULT '';
UPDATE scenarios SET slug = 's' || lower(hex(randomblob(6)));
CREATE UNIQUE INDEX scenarios_slug ON scenarios(slug);

-- Cover every creation path, including onboarding, cloning and archive imports.
CREATE TRIGGER scenarios_assign_slug
AFTER INSERT ON scenarios
WHEN NEW.slug = ''
BEGIN
    UPDATE scenarios SET slug = 's' || lower(hex(randomblob(6))) WHERE id = NEW.id;
END;
