-- Bracket filling: a withdrawal strategy that draws tax-deferred money only up
-- to the top of a chosen federal bracket, and the column holding that bracket.
--
-- SQLite cannot widen a CHECK in place, so the table is rebuilt. Nothing
-- references it, which keeps the rebuild to a copy and a rename.

PRAGMA defer_foreign_keys = ON;

CREATE TABLE effect_withdrawal_sources_new (
    effect_id  INTEGER PRIMARY KEY REFERENCES effects(id) ON DELETE CASCADE,
    mode       TEXT NOT NULL CHECK (mode IN ('SingleAsset','SingleAccount','Strategy','Custom')),
    account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    asset_id   INTEGER REFERENCES assets(id)   ON DELETE CASCADE,
    strategy   TEXT CHECK (strategy IS NULL OR strategy IN
                   ('TaxEfficientEarly','TaxDeferredFirst','TaxFreeFirst','ProRata','PenaltyAware',
                    'BracketFilling')),
    -- BracketFilling: the highest marginal rate to fill to. NULL is the default.
    bracket_ceiling REAL CHECK (bracket_ceiling IS NULL OR (bracket_ceiling >= 0 AND bracket_ceiling < 1)),

    CHECK (mode <> 'SingleAsset'   OR (account_id IS NOT NULL AND asset_id IS NOT NULL)),
    CHECK (mode <> 'SingleAccount' OR account_id IS NOT NULL),
    CHECK (mode <> 'Strategy'      OR strategy IS NOT NULL)
);

INSERT INTO effect_withdrawal_sources_new (effect_id, mode, account_id, asset_id, strategy)
    SELECT effect_id, mode, account_id, asset_id, strategy FROM effect_withdrawal_sources;

DROP TABLE effect_withdrawal_sources;
ALTER TABLE effect_withdrawal_sources_new RENAME TO effect_withdrawal_sources;
