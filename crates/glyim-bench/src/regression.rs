use crate::profile::CompilationProfile;
use std::time::Duration;

/// Detects performance regressions by comparing a current profile against a baseline.
pub struct RegressionDetector {
    pub baseline: CompilationProfile,
    pub threshold: f64,
}

#[derive(Debug, Clone)]
pub struct Regression {
    pub stage: String,
    pub baseline_duration: Duration,
    pub current_duration: Duration,
    pub regression_ratio: f64,
    pub severity: RegressionSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegressionSeverity {
    /// Within acceptable threshold
    Acceptable,
    /// Exceeds threshold by up to 50%
    Warning,
    /// Exceeds threshold by more than 50%
    Critical,
}

impl RegressionDetector {
    pub fn new(baseline: CompilationProfile, threshold: f64) -> Self {
        Self {
            baseline,
            threshold,
        }
    }

    pub fn compare(&self, current: &CompilationProfile) -> Vec<Regression> {
        let mut regressions = Vec::new();
        for (stage_name, current_stage) in &current.stages {
            if let Some(baseline_stage) = self.baseline.stages.get(stage_name) {
                let current_ms = current_stage.duration.as_secs_f64() * 1000.0;
                let baseline_ms = baseline_stage.duration.as_secs_f64() * 1000.0;
                if baseline_ms > 0.0 {
                    let ratio = current_ms / baseline_ms;
                    if ratio > self.threshold {
                        let severity = if ratio > self.threshold * 1.5 {
                            RegressionSeverity::Critical
                        } else {
                            RegressionSeverity::Warning
                        };
                        regressions.push(Regression {
                            stage: format!("{:?}", stage_name),
                            baseline_duration: baseline_stage.duration,
                            current_duration: current_stage.duration,
                            regression_ratio: ratio,
                            severity,
                        });
                    }
                }
            }
        }
        regressions
    }
}
