-- The Results fan: P10–P90 and P25–P75 around the median, in place of P5–P95.
--
-- Nullable because runs stored before this migration never measured them; the
-- API passes the NULLs through and the chart falls back to the P5–P95 band
-- those runs do have. P5 and P95 stay, for the report and older clients.

ALTER TABLE run_real_quantiles ADD COLUMN p10 REAL;
ALTER TABLE run_real_quantiles ADD COLUMN p25 REAL;
ALTER TABLE run_real_quantiles ADD COLUMN p75 REAL;
ALTER TABLE run_real_quantiles ADD COLUMN p90 REAL;
