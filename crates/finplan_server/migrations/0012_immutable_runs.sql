-- Preserve all future runs. Existing inputs cannot be reconstructed honestly.
DROP INDEX idx_runs_scenario;
CREATE INDEX idx_runs_scenario ON runs(scenario_id, id DESC);
ALTER TABLE runs ADD COLUMN input_hash TEXT;
ALTER TABLE runs ADD COLUMN model_version TEXT;
ALTER TABLE runs ADD COLUMN snapshot_json TEXT;
CREATE TABLE run_account_labels (
 run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
 account_id INTEGER NOT NULL,
 name TEXT NOT NULL,
 sort_order INTEGER NOT NULL,
 PRIMARY KEY(run_id, account_id)
);
-- Legacy names are explicitly unidentified, rather than attributed to live inputs.
INSERT INTO run_account_labels SELECT DISTINCT run_id, account_id, 'Historical account ' || account_id, account_id FROM run_account_points;
CREATE TABLE run_account_points_retained (
    id         INTEGER PRIMARY KEY,
    run_id     INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile REAL,
    account_id INTEGER NOT NULL,
    step       INTEGER NOT NULL,
    value      REAL    NOT NULL
);
INSERT INTO run_account_points_retained SELECT * FROM run_account_points;
DROP TABLE run_account_points;
ALTER TABLE run_account_points_retained RENAME TO run_account_points;
CREATE TABLE run_warnings_retained (
    id         INTEGER PRIMARY KEY,
    run_id     INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile REAL,
    position   INTEGER NOT NULL DEFAULT 0,
    kind       TEXT    NOT NULL,
    as_of_date TEXT,
    event_id   INTEGER,
    message    TEXT    NOT NULL
);
INSERT INTO run_warnings_retained SELECT * FROM run_warnings;
DROP TABLE run_warnings;
ALTER TABLE run_warnings_retained RENAME TO run_warnings;
CREATE TABLE run_ledger_retained (
    id          INTEGER PRIMARY KEY,
    run_id      INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile  REAL,
    position    INTEGER NOT NULL,
    as_of_date  TEXT    NOT NULL,
    year        INTEGER NOT NULL,
    -- cash | asset | tax | event — the four filters the ledger offers.
    category    TEXT    NOT NULL,
    -- Short label for the row: Income, Contribution, RMD, Sell, Penalty…
    kind        TEXT    NOT NULL,
    detail      TEXT    NOT NULL,
    -- Signed against the plan: money in is positive, money out negative.
    amount      REAL,
    -- The secondary figure some entries carry: the gross a tax was charged on,
    -- the gain inside a sale, the amount an RMD required.
    basis       REAL,
    basis_label TEXT,
    account_id  INTEGER,
    event_id    INTEGER
);
INSERT INTO run_ledger_retained SELECT * FROM run_ledger;
DROP TABLE run_ledger;
ALTER TABLE run_ledger_retained RENAME TO run_ledger;
CREATE INDEX idx_account_points ON run_account_points(run_id, percentile, step);
CREATE INDEX idx_run_warnings ON run_warnings(run_id);
CREATE INDEX idx_run_ledger ON run_ledger(run_id, percentile, year, position);
