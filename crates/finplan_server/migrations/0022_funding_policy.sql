-- Plan-level funding policy: when cash runs short, sell investments in this
-- order. A NULL strategy means the policy is off.
ALTER TABLE scenarios ADD COLUMN funding_strategy TEXT CHECK (funding_strategy IS NULL OR
    funding_strategy IN ('TaxEfficientEarly','TaxDeferredFirst','TaxFreeFirst','ProRata',
                         'PenaltyAware','BracketFilling'));
ALTER TABLE scenarios ADD COLUMN funding_bracket_ceiling REAL CHECK (funding_bracket_ceiling IS NULL
    OR (funding_bracket_ceiling >= 0 AND funding_bracket_ceiling < 1));
CREATE TABLE scenario_funding_excludes (
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    account_id  INTEGER NOT NULL REFERENCES accounts(id)  ON DELETE CASCADE,
    PRIMARY KEY (scenario_id, account_id)
);
