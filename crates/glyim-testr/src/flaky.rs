use crate::types::TestOutcome;

pub struct FlakeTracker;

impl Default for FlakeTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl FlakeTracker {
    pub fn new() -> Self {
        Self
    }
    pub fn score(&self, _test_name: &str, _outcome: &TestOutcome) -> f64 {
        // Placeholder: always returns 0.0 (not flaky)
        0.0
    }
    pub fn record(&self, _test_name: &str, _outcome: &TestOutcome) {
        // Placeholder
    }
}
