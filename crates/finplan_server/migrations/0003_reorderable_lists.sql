-- Every list the UI shows is draggable, so every list needs somewhere to keep
-- the order it was dragged into. Accounts, assets and events already had a
-- `sort_order`; positions and the two profile libraries did not.

ALTER TABLE positions          ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
ALTER TABLE return_profiles    ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
ALTER TABLE inflation_profiles ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

-- Every list is then ranked into the order it already had, rather than left as
-- a column of zeroes. Two reasons: a plan opened after this migration has to
-- look exactly as it did before anyone touches a grip, and `MAX + 1` on create
-- only appends to the end if the rows below it are already ranked apart.
--
-- The rank is computed into a temp table first and applied from there. Ranking
-- in place would have each UPDATE read a `sort_order` that earlier rows of the
-- same statement had already rewritten, which is how a renumbering quietly
-- shuffles the thing it was meant to preserve.

CREATE TEMP TABLE ranks (id INTEGER PRIMARY KEY, rank INTEGER NOT NULL);

-- Lots read oldest purchase first, within their account.
INSERT INTO ranks
SELECT id, ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY purchase_date, id) - 1
  FROM positions;
UPDATE positions SET sort_order = (SELECT rank FROM ranks WHERE ranks.id = positions.id);
DELETE FROM ranks;

-- Both profile libraries read alphabetically, within their owner.
INSERT INTO ranks
SELECT id, ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY name) - 1 FROM return_profiles;
UPDATE return_profiles SET sort_order = (SELECT rank FROM ranks WHERE ranks.id = return_profiles.id);
DELETE FROM ranks;

INSERT INTO ranks
SELECT id, ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY name) - 1 FROM inflation_profiles;
UPDATE inflation_profiles SET sort_order = (SELECT rank FROM ranks WHERE ranks.id = inflation_profiles.id);
DELETE FROM ranks;

-- The three that had the column are ranked from whatever it already held, so
-- their lists come out reading exactly as they did — densely numbered now, and
-- with ties (rows still on the default 0) broken by id, which is the order the
-- lists showed them in anyway.
INSERT INTO ranks
SELECT id, ROW_NUMBER() OVER (PARTITION BY scenario_id ORDER BY sort_order, id) - 1 FROM accounts;
UPDATE accounts SET sort_order = (SELECT rank FROM ranks WHERE ranks.id = accounts.id);
DELETE FROM ranks;

INSERT INTO ranks
SELECT id, ROW_NUMBER() OVER (PARTITION BY scenario_id ORDER BY sort_order, id) - 1 FROM assets;
UPDATE assets SET sort_order = (SELECT rank FROM ranks WHERE ranks.id = assets.id);
DELETE FROM ranks;

INSERT INTO ranks
SELECT id, ROW_NUMBER() OVER (PARTITION BY scenario_id ORDER BY sort_order, id) - 1 FROM events;
UPDATE events SET sort_order = (SELECT rank FROM ranks WHERE ranks.id = events.id);

DROP TABLE ranks;
