-- Why iterations failed the funding check (api::funding::FundingDiagnostics),
-- as JSON keyed by row ids. Summary-level, so it lives on run_stats and
-- survives the pruning of superseded runs' path details. Null on runs stored
-- before the engine measured it.
ALTER TABLE run_stats ADD COLUMN funding_diagnostics TEXT;
