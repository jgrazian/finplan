//! Compare exact annual vectors with bounded t-digests, including ingestion.
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rand::{Rng, SeedableRng};
use tdigest::TDigest;

fn benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_quantiles_41_dates");
    group.sample_size(10);
    for n in [1_000, 10_000, 100_000] {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
        // Signed, skewed wealth with bankrupt paths; no convenient sorted input.
        let columns: Vec<Vec<f64>> = (0..41)
            .map(|_| {
                (0..n)
                    .map(|_| (rng.random::<f64>() * 12.0).exp() - 10_000.0)
                    .collect()
            })
            .collect();
        group.bench_function(format!("exact/{n}"), |b| {
            b.iter(|| {
                for column in &columns {
                    let mut values = column.clone();
                    values.sort_unstable_by(f64::total_cmp);
                    for p in [0.05, 0.5, 0.95] {
                        let h = (n - 1) as f64 * p;
                        let lo = h.floor() as usize;
                        black_box(
                            values[lo] * (1.0 - h.fract()) + values[h.ceil() as usize] * h.fract(),
                        );
                    }
                }
            })
        });
        group.bench_function(format!("tdigest_100/{n}"), |b| {
            b.iter(|| {
                for column in &columns {
                    let mut digest = TDigest::new_with_size(100);
                    for chunk in column.chunks(1_000) {
                        digest = digest.merge_unsorted(chunk.to_vec());
                    }
                    for p in [0.05, 0.5, 0.95] {
                        black_box(digest.estimate_quantile(p));
                    }
                }
            })
        });
        let mut max_error = 0.0_f64;
        for column in &columns {
            let mut digest = TDigest::new_with_size(100);
            for chunk in column.chunks(1_000) {
                digest = digest.merge_unsorted(chunk.to_vec());
            }
            let mut sorted = column.clone();
            sorted.sort_unstable_by(f64::total_cmp);
            for p in [0.05, 0.5, 0.95] {
                let h = (n - 1) as f64 * p;
                let exact = sorted[h.floor() as usize] * (1.0 - h.fract())
                    + sorted[h.ceil() as usize] * h.fract();
                max_error = max_error.max((digest.estimate_quantile(p) - exact).abs());
            }
        }
        eprintln!(
            "n={n}: exact payload={} bytes; digest <=4100 centroids; max absolute quantile error={max_error:.2}",
            n * 41 * 8
        );
    }
    group.finish();
}
criterion_group!(benches, benchmark);
criterion_main!(benches);
