-- The account a warning is about (the overdrawn account for a shortfall, or
-- the account a skipped effect could not find or use), as a row id. Null on
-- warnings stored before the engine recorded it, and for accounts created
-- mid-simulation that have no row.
ALTER TABLE run_warnings ADD COLUMN account_id INTEGER;
