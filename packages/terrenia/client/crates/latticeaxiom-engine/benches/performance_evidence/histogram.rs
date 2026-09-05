//! Versioned nearest-rank histograms for ADR 0026 evidence.

/// Percentile and high-water summary for one integer series.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct HistogramV1 {
    /// Sample count retained after the warmup window.
    pub count: u64,
    /// Nearest-rank 50th percentile, when `count > 0`.
    pub p50: Option<u64>,
    /// Nearest-rank 95th percentile, when `count > 0`.
    pub p95: Option<u64>,
    /// Nearest-rank 99th percentile, when `count > 0`.
    pub p99: Option<u64>,
    /// Maximum sample.
    pub max: Option<u64>,
    /// Truncating arithmetic mean.
    pub mean: Option<u64>,
    /// Maximum sample; identical to `max` for a series of magnitudes.
    pub high_water: Option<u64>,
}

/// Accumulates unsorted integer samples and emits [`HistogramV1`].
#[derive(Clone, Debug, Default)]
pub struct SampleSeries {
    values: Vec<u64>,
}

impl SampleSeries {
    /// Appends one sample. Outliers are retained.
    pub fn push(&mut self, value: u64) {
        self.values.push(value);
    }

    /// Nearest-rank summary. Empty series yield `count = 0` and omitted percentiles.
    #[must_use]
    pub fn summarize(&self) -> HistogramV1 {
        summarize_samples(&self.values)
    }
}

/// Nearest-rank percentiles over an unsorted series.
///
/// Rank is `max(1, ceil(percentile / 100 * n))` on a non-decreasing copy.
/// Outliers are never dropped.
#[must_use]
pub fn summarize_samples(samples: &[u64]) -> HistogramV1 {
    if samples.is_empty() {
        return HistogramV1::default();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let count = u64::try_from(sorted.len()).unwrap_or(u64::MAX);
    let sum: u128 = sorted.iter().copied().map(u128::from).sum();
    let mean = u64::try_from(sum / u128::from(count)).unwrap_or(u64::MAX);
    let max = sorted.last().copied();
    HistogramV1 {
        count,
        p50: nearest_rank(&sorted, 50),
        p95: nearest_rank(&sorted, 95),
        p99: nearest_rank(&sorted, 99),
        max,
        mean: Some(mean),
        high_water: max,
    }
}

/// One-indexed nearest-rank percentile on a sorted slice.
#[must_use]
pub fn nearest_rank(sorted: &[u64], percentile: u32) -> Option<u64> {
    if sorted.is_empty() || percentile > 100 {
        return None;
    }
    let n = u128::try_from(sorted.len()).ok()?;
    let rank = (u128::from(percentile) * n).div_ceil(100);
    let rank = rank.max(1);
    let index = usize::try_from(rank.saturating_sub(1)).ok()?;
    sorted
        .get(index.min(sorted.len().saturating_sub(1)))
        .copied()
}
