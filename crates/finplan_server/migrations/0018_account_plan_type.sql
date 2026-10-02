-- Which retirement plan an investment account is, and the extra contribution
-- room it allows by age (the 401(k) catch-up at 50 and at 60-63, the IRA one
-- at 50, the HSA one at 55).
--
-- `catch_up` is a JSON array of {from_age, through_age, amount} tiers on top of
-- `contribution_limit`; the engine applies the largest tier the person's age at
-- year end falls in. Existing accounts keep no plan type and no catch-up, so
-- they simulate exactly as before.
ALTER TABLE account_investment ADD COLUMN plan_type TEXT
    CHECK (plan_type IS NULL OR plan_type IN
        ('Traditional401k','Roth401k','TraditionalIra','RothIra','Hsa'));
ALTER TABLE account_investment ADD COLUMN catch_up TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(catch_up) AND json_type(catch_up) = 'array');
