-- What kind of holding a return profile describes, as opposed to what it is
-- called.
--
-- The web client fills an asset's profile in from its ticker: VTI is the total
-- US market, and the total US market belongs on whatever this library calls US
-- equities. It found that profile by matching patterns against the profile's
-- *name* — which is the user's to change, so renaming "US Total Market" to
-- "Equities (my assumptions)" silently stopped every equity ticker from
-- mapping, with no error and a plausible-looking flat-zero asset as the result.
-- A stored class survives a rename, a translation and a duplicate.
--
-- Null is the ordinary state, not a defect: a profile nobody has classified is
-- simply never chosen automatically, which is what a hand-made one should be.
ALTER TABLE return_profiles ADD COLUMN asset_class TEXT;

-- Backfill the starter library, which every existing user was seeded with under
-- exactly these names. This is the same name matching the client used to do on
-- every keystroke — done once, against the names the seeder itself wrote, where
-- being wrong is visible and fixable rather than silent and permanent. A
-- profile someone has already renamed is left unclassified for them to set.
UPDATE return_profiles SET asset_class = 'UsEquity'   WHERE name = 'US Total Market';
UPDATE return_profiles SET asset_class = 'UsSmallCap' WHERE name = 'US Small Cap';
UPDATE return_profiles SET asset_class = 'Bonds'      WHERE name = 'US Aggregate Bonds';
UPDATE return_profiles SET asset_class = 'IntlEquity' WHERE name = 'International Developed';
UPDATE return_profiles SET asset_class = 'Reit'       WHERE name = 'REITs';
UPDATE return_profiles SET asset_class = 'Cash'       WHERE name = 'Cash / T-Bills';
