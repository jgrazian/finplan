-- One stored result set per scenario.
--
-- The Results screen only ever reads a scenario's latest successful run, so
-- every earlier run's bands, ledger and per-account series sat in the database
-- unreachable from anywhere — the largest tables in the schema, kept for a
-- history nothing displayed. A run now replaces its predecessor: `POST /runs`
-- cancels whatever the scenario had and deletes it before inserting, and this
-- index is what holds that true.
--
-- Existing databases keep one run a scenario: the latest successful one where
-- there is one, and otherwise simply the latest, so a scenario whose only run
-- failed still has that failure to show.

DELETE FROM runs
 WHERE id NOT IN (
     SELECT id
       FROM (
           SELECT id,
                  ROW_NUMBER() OVER (
                      PARTITION BY scenario_id
                      ORDER BY (status = 'succeeded') DESC, created_at DESC, id DESC
                  ) AS rank
             FROM runs
       )
      WHERE rank = 1
 );

DROP INDEX idx_runs_scenario;
CREATE UNIQUE INDEX idx_runs_scenario ON runs(scenario_id);
