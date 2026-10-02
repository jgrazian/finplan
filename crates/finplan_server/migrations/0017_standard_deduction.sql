-- The federal standard deduction, and the extra one from the tax year the
-- plan's person turns 65.
--
-- The engine folds the deduction into the brackets as a 0% band and indexes
-- both to inflation each year. Configs default to no deduction, which taxes
-- income exactly as before.
ALTER TABLE tax_configs ADD COLUMN standard_deduction REAL NOT NULL DEFAULT 0.0
    CHECK (standard_deduction >= 0);
ALTER TABLE tax_configs ADD COLUMN age_65_extra_deduction REAL NOT NULL DEFAULT 0.0
    CHECK (age_65_extra_deduction >= 0);

-- Registration seeds every user a 2024 single-filer config; give it the 2024
-- deduction that goes with those brackets. A config whose brackets were since
-- replaced (no longer the seeded 12% band at $11,600) is left as it is.
UPDATE tax_configs
   SET standard_deduction = 14600.0, age_65_extra_deduction = 1950.0
 WHERE name = 'US Federal 2024 (single)'
   AND EXISTS (SELECT 1 FROM tax_brackets b
                WHERE b.tax_config_id = tax_configs.id
                  AND b.threshold = 11600.0 AND b.rate = 0.12);
