-- Convergence-stopped runs.
--
-- A run either takes a fixed number of iterations or keeps sampling until its
-- metric settles. For the second kind `iterations` stops being the count and
-- becomes the minimum sample taken before the metric is first tested, and
-- `max_iterations` is the ceiling the run may not pass. Fixed runs leave both
-- columns alone: `converge` is 0 and `max_iterations` is NULL, so `iterations`
-- still reads as the count it always was.

ALTER TABLE runs ADD COLUMN converge INTEGER NOT NULL DEFAULT 0 CHECK (converge IN (0,1));
ALTER TABLE runs ADD COLUMN max_iterations INTEGER CHECK (max_iterations IS NULL OR max_iterations > 0);
