//! Rank metrics over one query, and bootstrap intervals over a set of them.

/// Graded relevance as judged: 0 not relevant, 1 partly, 2 relevant.
pub type Grade = u8;

pub const DEPTH: usize = 10;

/// Exponential gain, `2^grade - 1`: a relevant result is worth three partly
/// relevant ones.
fn gain(grade: Grade) -> f64 {
    match grade {
        0 => 0.0,
        1 => 1.0,
        _ => 3.0,
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "positions and counts are tiny; f64 holds them exactly"
)]
pub fn real(n: usize) -> f64 {
    n as f64
}

fn dcg(grades: impl Iterator<Item = Grade>) -> f64 {
    grades
        .take(DEPTH)
        .enumerate()
        .map(|(i, g)| gain(g) / (real(i) + 2.0).log2())
        .sum()
}

/// nDCG@10 of `ranked` (the grades of the ranking's results, in order),
/// against the ideal ordering of every grade judged for the query. A query
/// with nothing relevant scores 0 for every ranking.
pub fn ndcg(ranked: &[Grade], judged: &[Grade]) -> f64 {
    let mut ideal = judged.to_vec();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    let best = dcg(ideal.into_iter());
    if best == 0.0 { 0.0 } else { dcg(ranked.iter().copied()) / best }
}

/// Share of the first ten slots holding a partly or fully relevant result;
/// an empty slot counts as a miss.
pub fn precision(ranked: &[Grade]) -> f64 {
    real(ranked.iter().take(DEPTH).filter(|g| **g > 0).count()) / real(DEPTH)
}

/// 1 / position of the first hit that is a target, 0 when none is.
pub fn reciprocal_rank(is_target: &[bool]) -> f64 {
    is_target.iter().position(|t| *t).map_or(0.0, |i| 1.0 / (real(i) + 1.0))
}

pub fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / real(values.len())
    }
}

/// splitmix64 (Steele, Lea and Flood, OOPSLA 2014): a fixed seed gives
/// every run the same resamples, so an interval never moves between runs.
pub struct Rng(u64);

impl Rng {
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n`, by rejection so no index is favored.
    fn below(&mut self, n: usize) -> usize {
        let n = u64::try_from(n).unwrap_or(u64::MAX);
        let Some(waste) = u64::MAX.checked_rem(n) else { return 0 };
        let zone = u64::MAX.saturating_sub(waste);
        loop {
            let x = self.next();
            if x < zone {
                return x
                    .checked_rem(n)
                    .and_then(|i| usize::try_from(i).ok())
                    .unwrap_or(0);
            }
        }
    }
}

/// The seed every interval uses. Any fixed value would do; this one spells the
/// date the benchmark was first recorded, and `just relevance` reports how
/// far the intervals move under two other seeds.
pub const SEED: u64 = 0x2026_1007;

pub const RESAMPLES: usize = 10_000;

#[derive(Clone, Copy, Debug)]
pub struct Interval {
    pub mean: f64,
    pub low: f64,
    pub high: f64,
    /// Standard deviation of the resampled means: the standard error.
    pub se: f64,
}

/// Percentile bootstrap of the mean over queries (Efron and Tibshirani,
/// 1993): resample the per-query values with replacement, take the 2.5th
/// and 97.5th percentiles of the resampled means.
pub fn bootstrap(values: &[f64], seed: u64, resamples: usize) -> Interval {
    let center = mean(values);
    if values.is_empty() || resamples == 0 {
        return Interval { mean: center, low: center, high: center, se: 0.0 };
    }
    let mut rng = Rng::new(seed);
    let mut means: Vec<f64> = (0..resamples)
        .map(|_| {
            let total: f64 = (0..values.len())
                .filter_map(|_| values.get(rng.below(values.len())))
                .sum();
            total / real(values.len())
        })
        .collect();
    means.sort_unstable_by(f64::total_cmp);
    let at = |q: f64| {
        let i = (q * real(resamples.saturating_sub(1))).round();
        means.get(index(i)).copied().unwrap_or(center)
    };
    let average = mean(&means);
    let variance = means.iter().map(|m| (m - average).powi(2)).sum::<f64>()
        / real(resamples);
    Interval {
        mean: center,
        low: at(0.025),
        high: at(0.975),
        se: variance.sqrt(),
    }
}

/// The paired bootstrap: the interval of the mean per-query difference
/// `after - before`, which is far tighter than comparing two intervals
/// because most queries move together.
pub fn paired(before: &[f64], after: &[f64], seed: u64) -> Interval {
    let differences: Vec<f64> =
        after.iter().zip(before).map(|(a, b)| a - b).collect();
    bootstrap(&differences, seed, RESAMPLES)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a rounded, non-negative percentile position below the count"
)]
fn index(position: f64) -> usize {
    position.max(0.0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndcg_is_one_for_the_ideal_order_and_falls_when_it_is_reversed() {
        let judged = [2, 1, 0];
        assert!((ndcg(&[2, 1, 0], &judged) - 1.0).abs() < 1e-12);
        // DCG of [0, 1, 2] = 0 + 1/log2(3) + 3/log2(4); ideal = 3 + 1/log2(3).
        let worse = (1.0 / 3f64.log2() + 1.5) / (3.0 + 1.0 / 3f64.log2());
        assert!((ndcg(&[0, 1, 2], &judged) - worse).abs() < 1e-12);
    }

    #[test]
    fn ndcg_counts_only_the_first_ten_and_nothing_relevant_scores_zero() {
        let mut late = vec![0; 10];
        late.push(2);
        assert!(ndcg(&late, &[2]).abs() < 1e-12);
        assert!(ndcg(&[0, 0], &[0, 0]).abs() < 1e-12);
    }

    #[test]
    fn the_ideal_comes_from_every_judged_grade_not_from_the_ranking() {
        // The ranking shows only the grade-1 result; the ideal puts the
        // judged grade-2 one first: 1 / (3 + 1 / log2(3)).
        let expected = 1.0 / (3.0 + 1.0 / 3f64.log2());
        assert!((ndcg(&[1], &[2, 1]) - expected).abs() < 1e-12);
    }

    #[test]
    fn the_ideal_is_cut_at_ten_like_the_ranking() {
        assert!((ndcg(&[2; 10], &[2; 12]) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn precision_counts_partly_relevant_and_empty_slots_as_misses() {
        assert!((precision(&[2, 1, 0]) - 0.2).abs() < 1e-12);
        assert!((precision(&[1; 12]) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn reciprocal_rank_is_one_over_the_first_target_position() {
        assert!(
            (reciprocal_rank(&[false, false, true, true]) - 1.0 / 3.0).abs()
                < 1e-12
        );
        assert!(reciprocal_rank(&[false]).abs() < 1e-12);
    }

    #[test]
    fn bootstrap_of_a_constant_has_no_width_and_brackets_the_mean() {
        let flat = bootstrap(&[0.5; 20], SEED, 500);
        assert!(
            (flat.low - 0.5).abs() < 1e-12 && (flat.high - 0.5).abs() < 1e-12
        );
        let values: Vec<f64> = (0..40).map(|i| real(i % 5) / 4.0).collect();
        let spread = bootstrap(&values, SEED, 2_000);
        assert!(spread.low < spread.mean && spread.mean < spread.high);
        // The textbook standard error of this mean is sd / sqrt(n), about
        // 0.0559 here; the bootstrap should land near it.
        assert!((spread.se - 0.0559).abs() < 0.008, "{}", spread.se);
    }

    #[test]
    fn the_paired_interval_sees_a_shift_two_separate_intervals_would_hide() {
        let before: Vec<f64> = (0..30).map(|i| real(i) / 30.0).collect();
        let after: Vec<f64> = before.iter().map(|b| b + 0.02).collect();
        let shift = paired(&before, &after, SEED);
        assert!(shift.low > 0.0, "{shift:?}");
        let (b, a) =
            (bootstrap(&before, SEED, 2_000), bootstrap(&after, SEED, 2_000));
        assert!(a.low < b.high, "the separate intervals overlap");
    }

    #[test]
    fn rng_indices_stay_in_range() {
        let mut rng = Rng::new(SEED);
        assert!((0..1_000).all(|_| rng.below(7) < 7));
    }
}
