//! Sample statistics and the M2.2 stability gate.
//!
//! The plan's M2.2 definition of done is a *variance* budget, not a speed
//! number: "CoV of instruction counts < 0.1 % over 10 runs". Instruction
//! counts are deterministic for a deterministic program, so a tight gate is
//! the cheapest possible flake detector: any noise (ASLR layout shifts,
//! transparent hugepages, background load) shows up as CoV far above the
//! budget and the run is rejected before its numbers can be compared.

/// Maximum accepted coefficient of variation, in percent, that the M2.2
/// gate applies to the primary counter by default.
pub const DEFAULT_MAX_COV_PCT: f64 = 0.1;

/// Summary statistics over one metric's samples.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleStats {
    /// Smallest observed sample.
    pub min: u64,
    /// Largest observed sample.
    pub max: u64,
    /// Arithmetic mean.
    pub mean: f64,
    /// Median (mean of the two middle samples for even counts).
    pub median: f64,
    /// Coefficient of variation (population), in percent.
    pub cov_pct: f64,
}

/// Coefficient of variation (population standard deviation / mean), in
/// percent. `None` for an empty sample set; `0.0` for a single sample.
#[must_use]
pub fn coefficient_of_variation(samples: &[u64]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let n = samples.len() as f64;
    let mean = samples.iter().map(|s| *s as f64).sum::<f64>() / n;
    let variance = samples
        .iter()
        .map(|s| {
            let d = *s as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / n;
    if mean == 0.0 {
        // All-zero samples: perfectly stable by definition.
        return Some(0.0);
    }
    Some((variance.sqrt() / mean) * 100.0)
}

/// Summarise a sample set; `None` when empty.
#[must_use]
pub fn summarize(samples: &[u64]) -> Option<SampleStats> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted: Vec<u64> = samples.to_vec();
    sorted.sort_unstable();
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];
    let n = sorted.len() as f64;
    let mean = sorted.iter().map(|s| *s as f64).sum::<f64>() / n;
    let median = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2] as f64
    } else {
        let (a, b) = (sorted[sorted.len() / 2 - 1], sorted[sorted.len() / 2]);
        (a as f64 + b as f64) / 2.0
    };
    let cov_pct = coefficient_of_variation(samples).unwrap_or(f64::NAN);
    Some(SampleStats {
        min,
        max,
        mean,
        median,
        cov_pct,
    })
}

/// The stability gate: `cov_pct <= max_cov_pct`.
#[must_use]
pub fn gate_passes(cov_pct: f64, max_cov_pct: f64) -> bool {
    cov_pct <= max_cov_pct
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MAX_COV_PCT, coefficient_of_variation, gate_passes, summarize};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn identical_samples_have_zero_cov() {
        let cov = coefficient_of_variation(&[1_000; 10]).expect("non-empty");
        assert!(close(cov, 0.0), "identical samples: cov {cov}");
    }

    #[test]
    fn cov_matches_hand_computed_value() {
        // mean = 101, population variance = 1, sd = 1.
        let cov = coefficient_of_variation(&[100, 102]).expect("non-empty");
        let expected = (1.0_f64 / 101.0) * 100.0;
        assert!(close(cov, expected), "cov {cov} vs {expected}");
    }

    #[test]
    fn empty_samples_have_no_cov() {
        assert!(coefficient_of_variation(&[]).is_none());
        assert!(summarize(&[]).is_none());
    }

    #[test]
    fn single_sample_is_perfectly_stable() {
        let cov = coefficient_of_variation(&[42]).expect("non-empty");
        assert!(close(cov, 0.0));
        let stats = summarize(&[42]).expect("non-empty");
        assert_eq!(stats.min, 42);
        assert_eq!(stats.max, 42);
        assert!(close(stats.median, 42.0));
    }

    #[test]
    fn all_zero_samples_are_stable_not_nan() {
        let cov = coefficient_of_variation(&[0, 0, 0]).expect("non-empty");
        assert!(close(cov, 0.0), "zero samples: cov {cov}");
    }

    #[test]
    fn summarize_median_even_and_odd() {
        let odd = summarize(&[5, 1, 3]).expect("non-empty");
        assert!(close(odd.median, 3.0));
        assert_eq!(odd.min, 1);
        assert_eq!(odd.max, 5);
        let even = summarize(&[4, 1, 2, 3]).expect("non-empty");
        assert!(close(even.median, 2.5));
    }

    #[test]
    fn gate_boundaries_match_the_m2_2_budget() {
        assert!(gate_passes(0.0, DEFAULT_MAX_COV_PCT));
        assert!(
            gate_passes(0.1, DEFAULT_MAX_COV_PCT),
            "boundary is <=, not <"
        );
        assert!(!gate_passes(0.11, DEFAULT_MAX_COV_PCT));
    }

    #[test]
    fn cov_is_scale_invariant() {
        let a = coefficient_of_variation(&[1_000, 1_002]).expect("non-empty");
        let b = coefficient_of_variation(&[1_000_000, 1_002_000]).expect("non-empty");
        assert!(close(a, b), "cov must not depend on absolute scale");
    }
}
