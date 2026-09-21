-- Detailed cash flows and ledger entries are only retained for the newest
-- successful run in each scenario. Historical runs keep their immutable
-- inputs, summary statistics, chart series, taxes, warnings, and labels.
DELETE FROM run_cash_flows
WHERE run_id NOT IN (
    SELECT MAX(id)
    FROM runs
    WHERE status = 'succeeded'
    GROUP BY scenario_id
);

DELETE FROM run_ledger
WHERE run_id NOT IN (
    SELECT MAX(id)
    FROM runs
    WHERE status = 'succeeded'
    GROUP BY scenario_id
);
