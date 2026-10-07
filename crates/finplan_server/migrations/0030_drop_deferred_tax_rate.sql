-- The rate a tax-deferred balance was valued at after tax (0024) is no longer
-- a plan setting: each simulated path now works out the income tax its
-- pre-tax balances still owe from the plan's own tax config and income
-- (`SimulationResult::deferred_exit_tax`).
ALTER TABLE scenarios DROP COLUMN deferred_tax_rate;
