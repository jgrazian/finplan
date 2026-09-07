-- Preserve the original terminal-net-worth metric. Historical runs cannot be
-- classified from a handful of percentile paths or year-end snapshots, so NULL
-- means "not measured", never a backfilled estimate of funding success.
ALTER TABLE run_stats ADD COLUMN funding_success_rate REAL
    CHECK (funding_success_rate IS NULL OR funding_success_rate BETWEEN 0.0 AND 1.0);
