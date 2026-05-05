use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    Failed { exit_code: i32, stderr: String },
    TimedOut,
    Crash { signal: i32 },
    FlakyPass { retries: u32 },
    CompilationError(String),
    InternalError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestDef {
    pub name: String,
    pub source_file: String,
    pub ignored: bool,
    pub should_panic: bool,
    pub is_optimize_check: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    pub name: String,
    pub outcome: TestOutcome,
    pub duration: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_outcome_equality_depends_on_full_payload() {
        let a = TestOutcome::Failed { exit_code: 1, stderr: "a".into() };
        let b = TestOutcome::Failed { exit_code: 2, stderr: "a".into() };
        assert_ne!(a, b);
        let c = TestOutcome::Failed { exit_code: 1, stderr: "a".into() };
        assert_eq!(a, c);
    }

    #[test]
    fn internal_error_is_not_compilation_error() {
        let a = TestOutcome::InternalError("x".into());
        let b = TestOutcome::CompilationError("x".into());
        assert_ne!(a, b);
    }

    #[test]
    fn test_result_carries_name_and_duration() {
        let r = TestResult {
            name: "x".into(),
            outcome: TestOutcome::Passed,
            duration: std::time::Duration::from_millis(42),
        };
        assert_eq!(r.name, "x");
        assert_eq!(r.outcome, TestOutcome::Passed);
        assert_eq!(r.duration.as_millis(), 42);
    }
}
