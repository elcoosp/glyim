use crate::types::TestOutcome;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlakeTracker {
    /// Minimum number of runs before we can judge flakiness
    min_runs: usize,
    /// A test is considered flaky if its pass rate is between these thresholds (exclusive of 1.0)
    flaky_low: f64,
    flaky_high: f64,
    /// Per-test history: recent outcomes (stores last N outcomes)
    history: HashMap<String, Vec<bool>>, // true = pass, false = fail
    max_history: usize,
}

impl FlakeTracker {
    pub fn new() -> Self {
        Self {
            min_runs: 5,
            flaky_low: 0.8,
            flaky_high: 1.0, // flaky if pass rate < 1.0 but >= 0.8
            history: HashMap::new(),
            max_history: 20,
        }
    }

    /// Record a test outcome.
    pub fn record(&mut self, test_name: &str, outcome: &TestOutcome) {
        let passed = match outcome {
            TestOutcome::Passed => true,
            _ => false,
        };
        let entry = self.history.entry(test_name.to_string()).or_default();
        entry.push(passed);
        if entry.len() > self.max_history {
            entry.remove(0);
        }
    }

    /// Compute the flake score (0.0 = not flaky, >0.0 = flaky likelihood).
    /// Based on the recent pass rate.
    pub fn score(&self, test_name: &str) -> f64 {
        if let Some(history) = self.history.get(test_name) {
            if history.len() < self.min_runs {
                return 0.0;
            }
            let passes = history.iter().filter(|&&p| p).count();
            let pass_rate = passes as f64 / history.len() as f64;
            if pass_rate >= self.flaky_low && pass_rate < self.flaky_high {
                // Flaky: high pass rate but not perfect. Score is how far from perfect.
                return 1.0 - pass_rate;
            }
        }
        0.0
    }

    /// List flaky tests.
    pub fn flaky_tests(&self) -> Vec<String> {
        self.history.keys()
            .filter(|name| self.score(name) > 0.0)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TestOutcome;

    #[test]
    fn new_tracker_no_flakes() {
        let tracker = FlakeTracker::new();
        assert_eq!(tracker.score("any"), 0.0);
    }

    #[test]
    fn flaky_after_intermittent_fails() {
        let mut tracker = FlakeTracker::new();
        let name = "unstable";
        for i in 0..8 {
            if i % 3 == 0 {
                tracker.record(name, &TestOutcome::Failed { exit_code: 1, stderr: String::new() });
            } else {
                tracker.record(name, &TestOutcome::Passed);
            }
        }
        // Should be >0.0 because pass rate is not 1.0 and >= min_runs
        assert!(tracker.score(name) > 0.0);
    }
}
