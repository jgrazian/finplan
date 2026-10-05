-- The run's median after-tax ending balance (spec 21), nominal: what a
-- preview compares a Roth conversion against without simulating the base
-- again. NULL for runs stored before it was kept, which a preview re-runs.
ALTER TABLE run_stats ADD COLUMN after_tax_final REAL;
