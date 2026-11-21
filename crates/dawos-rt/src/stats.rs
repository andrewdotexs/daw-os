//! Statistics of a stream of durations, kept without allocating.
//!
//! A live thread adds one value per cycle; whoever prints the result does
//! so after the run, or from another thread on a copy.

/// Width of one histogram bucket, in nanoseconds.
const BUCKET_NS: u64 = 250;
/// Number of buckets: values of 100 us or more go into the overflow
/// count.
const BUCKETS: usize = 400;

/// Count, lowest, highest, sum and a histogram of durations in
/// nanoseconds. A fixed size, decided here: nothing grows.
#[derive(Clone)]
pub struct Durations {
    count: u64,
    sum_ns: u64,
    min_ns: u64,
    max_ns: u64,
    buckets: [u64; BUCKETS],
    overflow: u64,
}

impl Default for Durations {
    fn default() -> Self {
        Self::new()
    }
}

impl Durations {
    /// An empty collection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            count: 0,
            sum_ns: 0,
            min_ns: u64::MAX,
            max_ns: 0,
            buckets: [0; BUCKETS],
            overflow: 0,
        }
    }

    /// Add one duration. No allocation, no system call.
    #[inline]
    pub fn add(&mut self, ns: u64) {
        self.count += 1;
        self.sum_ns = self.sum_ns.saturating_add(ns);
        self.min_ns = self.min_ns.min(ns);
        self.max_ns = self.max_ns.max(ns);
        match self.buckets.get_mut((ns / BUCKET_NS) as usize) {
            Some(bucket) => *bucket += 1,
            None => self.overflow += 1,
        }
    }

    /// How many durations were added.
    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }

    /// The lowest, in nanoseconds; `None` when empty.
    #[must_use]
    pub const fn min_ns(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.min_ns)
        }
    }

    /// The highest, in nanoseconds; `None` when empty.
    #[must_use]
    pub const fn max_ns(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.max_ns)
        }
    }

    /// The mean, in nanoseconds; `None` when empty.
    #[must_use]
    pub const fn mean_ns(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum_ns / self.count)
        }
    }

    /// The value below which `per_100k` of every hundred thousand
    /// durations fall, to the width of a bucket; `None` when empty. When
    /// it lies beyond the histogram the highest value is returned.
    #[must_use]
    pub fn quantile_ns(&self, per_100k: u32) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        // The rank, rounded up: the smallest number of values that is at
        // least the share asked for.
        let rank = (u128::from(self.count) * u128::from(per_100k)).div_ceil(100_000);
        let rank = rank.max(1) as u64;
        let mut seen = 0;
        for (index, bucket) in self.buckets.iter().enumerate() {
            seen += bucket;
            if seen >= rank {
                // The upper edge of the bucket.
                return Some((index as u64 + 1) * BUCKET_NS);
            }
        }
        Some(self.max_ns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_collection_has_no_figures() {
        let durations = Durations::new();
        assert_eq!(durations.count(), 0);
        assert_eq!(durations.min_ns(), None);
        assert_eq!(durations.max_ns(), None);
        assert_eq!(durations.mean_ns(), None);
        assert_eq!(durations.quantile_ns(99_999), None);
    }

    #[test]
    fn figures_of_a_known_series() {
        let mut durations = Durations::new();
        for ns in [1_000, 2_000, 3_000, 6_000] {
            durations.add(ns);
        }
        assert_eq!(durations.count(), 4);
        assert_eq!(durations.min_ns(), Some(1_000));
        assert_eq!(durations.max_ns(), Some(6_000));
        assert_eq!(durations.mean_ns(), Some(3_000));
    }

    #[test]
    fn quantiles_follow_the_histogram() {
        let mut durations = Durations::new();
        // 99 999 values of 2 us and one of 20 us.
        for _ in 0..99_999 {
            durations.add(2_000);
        }
        durations.add(20_000);
        // 2 000 ns falls in the bucket from 2 000 to 2 250.
        assert_eq!(durations.quantile_ns(50_000), Some(2_250));
        assert_eq!(durations.quantile_ns(99_999), Some(2_250));
        assert_eq!(durations.quantile_ns(100_000), Some(20_250));
    }

    #[test]
    fn values_beyond_the_histogram_are_kept() {
        let mut durations = Durations::new();
        durations.add(5_000_000);
        assert_eq!(durations.max_ns(), Some(5_000_000));
        assert_eq!(durations.quantile_ns(100_000), Some(5_000_000));
    }
}
