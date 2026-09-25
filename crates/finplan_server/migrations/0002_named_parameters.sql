CREATE TABLE named_parameters (
    id INTEGER PRIMARY KEY,
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('Money','Rate','Date','Age')),
    number_value REAL,
    date_value TEXT,
    age_years INTEGER CHECK (age_years IS NULL OR age_years BETWEEN 0 AND 255),
    age_months INTEGER CHECK (age_months IS NULL OR age_months BETWEEN 0 AND 11),
    UNIQUE (scenario_id, name)
);
CREATE INDEX idx_named_parameters_scenario ON named_parameters(scenario_id);

-- Existing amount/trigger check constraints continue to validate legacy rows.
-- An expression occupies a Fixed row with value 0; the source is authoritative.
ALTER TABLE transfer_amounts ADD COLUMN expression_source TEXT;
ALTER TABLE triggers ADD COLUMN parameter_id INTEGER REFERENCES named_parameters(id) ON DELETE NO ACTION;
