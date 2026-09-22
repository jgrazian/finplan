# Review #3: real quantile definition and storage decision

## Measurement contract

- Representative paths retain the legacy selection rule: sort **terminal nominal
  net worth**, then choose index `min(floor(N * p), N - 1)`. Deflating a selected
  path does not turn it into a pointwise percentile or a real-median path.
- The independent envelope uses **all** Monte Carlo iterations. Each observation
  is deflated with that iteration's own cumulative inflation **before** sorting.
  At each date P5/P50/P95 use type-7 linear interpolation: `h = (N - 1) * p`,
  interpolate between `floor(h)` and `ceil(h)`. A singleton has three equal quantiles.
- Common grid: plan start, December 31 checkpoints, terminal date. Repeated dates
  retain the last snapshot. We use existing checkpoint timing (including the
  engine's year-end snapshot convention); no interpolated wealth checkpoints are
  invented. A mismatched date grid fails the run rather than aligning by index.
- Dollar base date is the stored plan start. Deflators follow the engine's annual
  convention: `factor[observation.year - start.year]`; factor 0 is 1.0. There is
  **no within-year inflation interpolation**. Web labels name the base date and
  this convention; the annual chart keeps the last observation in each year.
- Funding shortfalls and nonfatal processing warnings do **not** remove paths from
  the distribution. Hard simulation errors, nonfinite wealth, nonpositive or
  nonfinite inflation factors, and nonfinite real aggregates fail the entire run.
  Hard errors are no longer silently discarded/retried in either MC pass.
- Real terminal mean/min/max/population standard deviation are computed from the
  same per-iteration real terminal observations. Real terminal quantiles are the
  last envelope point, not nominal aggregate values divided by one path's inflation.
- `run_real_stats` / `run_real_quantiles` are independent of representative path
  storage. `(run_id, path_id)` identifies a coherent path's account series,
  cash-flow table and ledger; an envelope deliberately has no such ID or ledger.
  The legacy `mean` series is synthetic and nominal, not a representative path.
- Historical runs have `real_net_worth: null`. The web does not reconstruct an
  envelope or approximate real terminal statistics from their stored paths.
  Historical path detail without recorded inflation is explicitly nominal.

## Reproducible microbenchmark

```bash
cargo bench -p finplan_core --bench results_quantiles -- \
  --warm-up-time 0.1 --measurement-time 0.2
```

Apple M5 Pro / arm64, optimized Cargo bench profile, Criterion 0.5, 10 samples.
These short local measurements compare quantile construction, **not whole
simulation runtime**, and are not a production capacity guarantee.

Both approaches ingest 41 date columns of unsorted, signed, skewed synthetic
wealth: `exp(U * 12) - 10_000`, fixed SmallRng seed 42. Exact clones/sorts each
column and reads three interpolated quantiles. The sketch uses `tdigest` 0.2.3,
100 centroids, streamed in chunks of 1,000, and reads the same three quantiles.
Input generation is excluded; copies, ingestion, sorting and queries are included.

| Paths | Exact vectors | t-digest | Exact scalar payload | Maximum sketch dollar error across 123 quantiles |
| ---: | ---: | ---: | ---: | ---: |
| 1,000 | 0.217 ms | 0.321 ms | 0.328 MB | 3,030.67 |
| 10,000 | 3.263 ms | 4.575 ms | 3.28 MB | 2,132.19 |
| 100,000 | 40.037 ms | 47.070 ms | 32.8 MB | 2,399.44 |

The sketch holds at most 4,100 centroids across these dates, plus an ingestion
chunk, rather than N × dates values. Exact payload is `8 * N * dates` bytes;
vector capacities, parallel merge overlap and ordinary simulation state add to
peak process memory. Only three quantiles/date and terminal stats are persisted,
not the observation vectors. Stats-only optimization/sweep calls do not allocate
these vectors because they do not return envelopes.

**Decision: exact annual vectors.** They are faster in these fixtures, preserve
reproducible interpolation without sketch error, and have a manageable scalar
payload at the tested sizes. Keep the sketch as a benchmark-only dependency.
Reconsider sketches if horizons/iteration limits/concurrency make vector memory
unacceptable; that requires an explicit accuracy budget and a new capacity test,
not silently changing quantile semantics.

## Regression coverage

- `crates/finplan_core/src/tests/results_quantiles.rs`: crossing paths, signed
  outcomes, nominal/real rank reversal, type-7 interpolation, singleton/duplicate
  dates, warnings included, invalid observations, parallel-accumulator merging,
  and seeded stochastic-inflation replay over every iteration.
- `crates/finplan_server/tests/api.rs`: exact deterministic persisted values,
  warning-path inclusion, ordered/stable envelopes across path selection,
  resolved IDs shared with ledger requests, and historical NULLs.
- `web/tests/results-mapping.test.mts`: independent real stats, crossing paths,
  exact response-owned path identity and simultaneous detail mapping, unchanged
  envelope on selection, historical data, nominal mean, and stored base dates.

Path switches retain the old **payload and its labels** with a loading message
until the new response arrives. Accounts, wealth, flows, warnings, inflation and
ledger identity then switch together. Run/scenario tags and effect cleanup reject
mismatched/late responses; ledger drawers are keyed by the loaded run/path pair.
Immutable full run inputs and scenario freshness remain separate Review #4 work.
