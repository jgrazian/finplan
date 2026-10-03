-- Where a new plan is created by default (spec 19): on this device ("local") or
-- on FinPlan's server ("cloud"). Local for everyone at first; existing accounts
-- keep their cloud plans either way.
ALTER TABLE users ADD COLUMN default_plan_home TEXT NOT NULL DEFAULT 'local'
    CHECK (default_plan_home IN ('local', 'cloud'));
