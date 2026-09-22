//! Five-minute extrema at one-second resolution, using monotonic elapsed time.

const WINDOW: usize = 300;

#[derive(Clone, Copy)]
struct Bucket {
    second: u64,
    count: u64,
    min: f64,
    max: f64,
}

impl Default for Bucket {
    fn default() -> Self {
        Self {
            second: 0,
            count: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        }
    }
}

pub(super) struct RecentExtrema {
    buckets: Box<[Bucket; WINDOW]>,
}

impl Default for RecentExtrema {
    fn default() -> Self {
        Self {
            buckets: Box::new([Bucket::default(); WINDOW]),
        }
    }
}

impl RecentExtrema {
    pub fn observe(&mut self, second: u64, value: f64) {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        let bucket = &mut self.buckets[second as usize % WINDOW];
        if bucket.second != second {
            *bucket = Bucket {
                second,
                ..Bucket::default()
            };
        }
        bucket.count += 1;
        bucket.min = bucket.min.min(value);
        bucket.max = bucket.max.max(value);
    }

    pub fn snapshot(&self, now: u64) -> (f64, f64, u64) {
        let mut count = 0;
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for b in self.buckets.iter() {
            if b.count > 0 && b.second <= now && now - b.second < WINDOW as u64 {
                count += b.count;
                min = min.min(b.min);
                max = max.max(b.max);
            }
        }
        if count == 0 {
            (f64::NAN, f64::NAN, 0)
        } else {
            (min, max, count)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_window_expiry_rollover_and_empty() {
        let mut recent = RecentExtrema::default();
        assert!(recent.snapshot(0).0.is_nan());
        recent.observe(0, 4.0);
        recent.observe(0, 2.0);
        recent.observe(299, 8.0);
        assert_eq!(recent.snapshot(299), (2.0, 8.0, 3));
        assert_eq!(recent.snapshot(300), (8.0, 8.0, 1));
        recent.observe(300, 1.0);
        assert_eq!(recent.snapshot(300), (1.0, 8.0, 2));
        assert!(recent.snapshot(600).0.is_nan());
        assert_eq!(recent.snapshot(600).2, 0);
        recent.observe(601, f64::NAN);
        recent.observe(601, -2.0);
        assert_eq!(recent.snapshot(601).2, 0);
    }
}
