-- Path series are now kept only for the newest successful run in each scenario
-- (db::CURRENT_RUN_ONLY_TABLES). Older runs keep run_stats,
-- run_percentile_values and run_real_stats. Cash flows and the ledger were
-- already pruned this way; this clears the backlog of everything else.
DELETE FROM run_net_worth_points WHERE run_id NOT IN (
    SELECT MAX(id) FROM runs WHERE status = 'succeeded' GROUP BY scenario_id);
DELETE FROM run_account_points WHERE run_id NOT IN (
    SELECT MAX(id) FROM runs WHERE status = 'succeeded' GROUP BY scenario_id);
DELETE FROM run_cash_flows WHERE run_id NOT IN (
    SELECT MAX(id) FROM runs WHERE status = 'succeeded' GROUP BY scenario_id);
DELETE FROM run_taxes WHERE run_id NOT IN (
    SELECT MAX(id) FROM runs WHERE status = 'succeeded' GROUP BY scenario_id);
DELETE FROM run_inflation WHERE run_id NOT IN (
    SELECT MAX(id) FROM runs WHERE status = 'succeeded' GROUP BY scenario_id);
DELETE FROM run_warnings WHERE run_id NOT IN (
    SELECT MAX(id) FROM runs WHERE status = 'succeeded' GROUP BY scenario_id);
DELETE FROM run_real_quantiles WHERE run_id NOT IN (
    SELECT MAX(id) FROM runs WHERE status = 'succeeded' GROUP BY scenario_id);
DELETE FROM run_ledger WHERE run_id NOT IN (
    SELECT MAX(id) FROM runs WHERE status = 'succeeded' GROUP BY scenario_id);
