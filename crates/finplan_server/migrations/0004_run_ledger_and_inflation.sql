-- Two things the Results screen needs that a run never wrote down.
--
-- The first is inflation. Every figure the engine produces is nominal — a
-- dollar in 2061 sitting next to a dollar in 2026 — and the only thing that
-- makes the two comparable is the path's own realised inflation. The engine
-- computes it (`SimulationResult::cumulative_inflation`) and then threw it
-- away at the persist boundary, so the web had no way to deflate anything.
--
-- The second is the ledger: the itemised effects behind each year's cash-flow
-- totals. `run_cash_flows` stores the sums; this stores what they are sums of.

-- Cumulative inflation factor per plan year, per stored path. Factor 1.0 is
-- the plan's first year, so `real = nominal / factor`.
CREATE TABLE run_inflation (
    id         INTEGER PRIMARY KEY,
    run_id     INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile REAL,
    year       INTEGER NOT NULL,
    factor     REAL    NOT NULL DEFAULT 1.0
);
CREATE INDEX idx_run_inflation ON run_inflation(run_id, percentile, year);

-- One row per state change worth reading back. Time advances and year
-- rollovers are dropped: they carry no figure and would bury the rest.
--
-- `detail` is deliberately free of dollar figures. Everything monetary goes in
-- `amount` and `basis` so the client can restate the row in real dollars
-- without having to rewrite prose.
CREATE TABLE run_ledger (
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
    account_id  INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    event_id    INTEGER REFERENCES events(id) ON DELETE SET NULL
);
CREATE INDEX idx_run_ledger ON run_ledger(run_id, percentile, year, position);
