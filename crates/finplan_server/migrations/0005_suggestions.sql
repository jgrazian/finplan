-- Review notes: rule- or model-written suggestions against one run of a
-- scenario, each a batch of plan changes (api::suggestions).
--
-- run_id is provenance, not a foreign key: runs are deleted and superseded
-- independently, and a suggestion (above all an applied or dismissed one,
-- whose fingerprint silences later reviews) must outlive the run it was
-- written against. The scenario owns it and takes it with it.
CREATE TABLE suggestions (
    id             INTEGER PRIMARY KEY,
    scenario_id    INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    run_id         INTEGER NOT NULL,
    source         TEXT    NOT NULL CHECK (source IN ('rules', 'ai')),
    rule           TEXT,
    kind           TEXT    NOT NULL CHECK (kind IN ('fix', 'check', 'stress', 'read')),
    section        TEXT    NOT NULL CHECK (section IN ('portfolio', 'plan', 'results')),
    title          TEXT    NOT NULL,
    reasoning      TEXT    NOT NULL,
    evidence_json  TEXT    NOT NULL,
    changes_json   TEXT    NOT NULL,
    diff_json      TEXT    NOT NULL,
    estimate_json  TEXT,
    check_json     TEXT,
    fingerprint    TEXT    NOT NULL,
    status         TEXT    NOT NULL DEFAULT 'open'
                           CHECK (status IN ('open', 'applied', 'dismissed', 'confirmed')),
    created_at     TEXT    NOT NULL DEFAULT (datetime('now')),
    resolved_at    TEXT
);

CREATE INDEX suggestions_by_scenario ON suggestions (scenario_id, status);
CREATE INDEX suggestions_by_fingerprint ON suggestions (scenario_id, fingerprint);

-- The latest review of each scenario: which run it read, and when.
CREATE TABLE suggestion_reviews (
    scenario_id  INTEGER PRIMARY KEY REFERENCES scenarios(id) ON DELETE CASCADE,
    run_id       INTEGER NOT NULL,
    reviewed_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
