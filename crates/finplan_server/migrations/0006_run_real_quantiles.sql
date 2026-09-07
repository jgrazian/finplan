-- Independent statistical distribution; deliberately has no percentile path key.
-- Historical runs have no rows: their real distribution cannot be reconstructed
-- from the few representative nominal-ranked paths.
CREATE TABLE run_real_stats (
    run_id INTEGER PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
    base_date TEXT NOT NULL,
    num_iterations INTEGER NOT NULL,
    mean REAL NOT NULL,
    std_dev REAL NOT NULL,
    min REAL NOT NULL,
    max REAL NOT NULL
);
CREATE TABLE run_real_quantiles (
    run_id INTEGER NOT NULL REFERENCES run_real_stats(run_id) ON DELETE CASCADE,
    as_of_date TEXT NOT NULL,
    p5 REAL NOT NULL,
    p50 REAL NOT NULL,
    p95 REAL NOT NULL,
    PRIMARY KEY (run_id, as_of_date),
    CHECK (p5 <= p50 AND p50 <= p95)
);
