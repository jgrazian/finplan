-- The rate a tax-deferred balance is valued at when the plan is valued after
-- tax (spec 21): after-tax ending balance counts a 401(k) at (1 - rate).
ALTER TABLE scenarios ADD COLUMN deferred_tax_rate REAL NOT NULL DEFAULT 0.24
    CHECK (deferred_tax_rate >= 0 AND deferred_tax_rate < 1);
