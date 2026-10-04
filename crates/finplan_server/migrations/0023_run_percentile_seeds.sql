-- The seed that reproduces each stored percentile path (a u64, so decimal
-- text). NULL for runs saved before seeds were kept.
ALTER TABLE run_percentiles ADD COLUMN seed TEXT;
