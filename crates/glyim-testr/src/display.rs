pub trait DisplayBackend: Send + Sync {
    fn suite_started(&self, test_count: usize);
    fn test_started(&self, name: &str);
    fn test_finished(&self, result: &crate::types::TestResult);
    fn suite_finished(&self, passed: usize, failed: usize, _total: usize);
}

pub struct HumanReporter;

impl DisplayBackend for HumanReporter {
    fn suite_started(&self, count: usize) {
        eprintln!("running {} tests", count);
    }
    fn test_started(&self, name: &str) {
        eprint!("test {} ... ", name);
    }
    fn test_finished(&self, result: &crate::types::TestResult) {
        match &result.outcome {
            crate::types::TestOutcome::Passed => eprintln!("ok"),
            crate::types::TestOutcome::Failed { exit_code, stderr } => {
                eprintln!("FAILED (exit code: {})", exit_code);
                if !stderr.is_empty() {
                    eprintln!("{}", stderr);
                }
            }
            crate::types::TestOutcome::TimedOut => eprintln!("TIMEOUT"),
            other => eprintln!("{:?}", other),
        }
    }
    fn suite_finished(&self, passed: usize, failed: usize, _total: usize) {
        let status = if failed == 0 { "ok" } else { "FAILED" };
        eprintln!(
            "\ntest result: {}. {} passed; {} failed; 0 ignored",
            status, passed, failed
        );
    }
}

pub struct NoopReporter;

impl DisplayBackend for NoopReporter {
    fn suite_started(&self, _: usize) {}
    fn test_started(&self, _: &str) {}
    fn test_finished(&self, _: &crate::types::TestResult) {}
    fn suite_finished(&self, _: usize, _: usize, _: usize) {}
}
