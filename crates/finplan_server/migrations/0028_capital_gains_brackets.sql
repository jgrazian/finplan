-- Long-term capital gains are taxed by a bracket schedule stacked on ordinary
-- income (0% / 15% / 20% in law) in place of one flat rate, and a tax config
-- may name where the 3.8% net investment income tax starts.
--
-- The schedule is over taxable income, like `tax_brackets`; the engine moves
-- it by the standard deduction and indexes it to inflation with them.
CREATE TABLE tax_gain_brackets (
    id            INTEGER PRIMARY KEY,
    tax_config_id INTEGER NOT NULL REFERENCES tax_configs(id) ON DELETE CASCADE,
    threshold     REAL    NOT NULL CHECK (threshold >= 0),
    rate          REAL    NOT NULL CHECK (rate BETWEEN 0 AND 1),
    UNIQUE (tax_config_id, threshold)
);
CREATE INDEX idx_tax_gain_brackets_config ON tax_gain_brackets(tax_config_id);

-- Registration seeds every user a 2024 single-filer config; give it the 2024
-- single gains schedule. One whose brackets or rate were since changed (no
-- longer the seeded 12% band at $11,600 and 15% gains) is left as it was.
INSERT INTO tax_gain_brackets (tax_config_id, threshold, rate)
SELECT t.id, s.threshold, s.rate
  FROM tax_configs t,
       (SELECT 0.0 AS threshold, 0.0 AS rate
        UNION ALL SELECT 47025.0, 0.15
        UNION ALL SELECT 518900.0, 0.20) s
 WHERE t.name = 'US Federal 2024 (single)'
   AND t.capital_gains_rate = 0.15
   AND EXISTS (SELECT 1 FROM tax_brackets b
                WHERE b.tax_config_id = t.id
                  AND b.threshold = 11600.0 AND b.rate = 0.12);

-- Every other config keeps taxing gains as it did: its flat rate from 0.
INSERT INTO tax_gain_brackets (tax_config_id, threshold, rate)
SELECT id, 0.0, capital_gains_rate
  FROM tax_configs
 WHERE id NOT IN (SELECT tax_config_id FROM tax_gain_brackets);

ALTER TABLE tax_configs DROP COLUMN capital_gains_rate;

-- No threshold, no net investment income tax.
ALTER TABLE tax_configs ADD COLUMN niit_threshold REAL CHECK (niit_threshold >= 0);
