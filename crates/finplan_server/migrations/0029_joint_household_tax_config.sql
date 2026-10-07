-- Registration now seeds two tax configs, "Single" and "Joint Household"
-- (finplan_plan::library::seed). Give every existing library the 2024
-- married-filing-jointly one; its single-filer config is left as it is.
--
-- A user without any tax config deleted the seeded one and is left alone, as
-- is one who already has a config by this name.
INSERT INTO tax_configs
    (user_id, name, description, state_rate, early_withdrawal_penalty_rate,
     standard_deduction, age_65_extra_deduction, niit_threshold)
SELECT u.id, 'Joint Household',
       '2024 married filing jointly: federal and capital gains brackets, standard deduction, NIIT, 5% state',
       0.05, 0.10, 29200.0, 3100.0, 250000.0
  FROM users u
 WHERE EXISTS (SELECT 1 FROM tax_configs t WHERE t.user_id = u.id)
   AND NOT EXISTS (SELECT 1 FROM tax_configs t
                    WHERE t.user_id = u.id AND t.name = 'Joint Household');

-- The rows just written are the only 'Joint Household' configs without brackets.
INSERT INTO tax_brackets (tax_config_id, threshold, rate)
SELECT t.id, s.threshold, s.rate
  FROM tax_configs t,
       (SELECT 0.0 AS threshold, 0.10 AS rate
        UNION ALL SELECT 23200.0, 0.12
        UNION ALL SELECT 94300.0, 0.22
        UNION ALL SELECT 201050.0, 0.24
        UNION ALL SELECT 383900.0, 0.32
        UNION ALL SELECT 487450.0, 0.35
        UNION ALL SELECT 731200.0, 0.37) s
 WHERE t.name = 'Joint Household'
   AND NOT EXISTS (SELECT 1 FROM tax_brackets b WHERE b.tax_config_id = t.id)
   AND NOT EXISTS (SELECT 1 FROM tax_gain_brackets g WHERE g.tax_config_id = t.id);

INSERT INTO tax_gain_brackets (tax_config_id, threshold, rate)
SELECT t.id, s.threshold, s.rate
  FROM tax_configs t,
       (SELECT 0.0 AS threshold, 0.0 AS rate
        UNION ALL SELECT 94050.0, 0.15
        UNION ALL SELECT 583750.0, 0.20) s
 WHERE t.name = 'Joint Household'
   AND NOT EXISTS (SELECT 1 FROM tax_gain_brackets g WHERE g.tax_config_id = t.id);
