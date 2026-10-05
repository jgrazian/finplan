-- An investment account's uninvested cash at each stored point, part of
-- `value` (spec 21: the cash_accumulates check reads it). NULL for other
-- accounts, and for runs saved before it was kept.
ALTER TABLE run_account_points ADD COLUMN cash REAL;
