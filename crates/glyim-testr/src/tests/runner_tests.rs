use crate::config::TestConfig;
use crate::display::NoopReporter;
use crate::runner::TestRunner;

#[tokio::test]
async fn runner_runs_two_tests_and_both_pass() {
    let source = "#[test]\nfn a() -> i64 { 0 }\n#[test]\nfn b() -> i64 { 0 }";
    let config = TestConfig::default();
    let runner = TestRunner::new(config);
    let results = runner.run_all(source, &NoopReporter).await;
    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(matches!(r.outcome, crate::types::TestOutcome::Passed));
    }
}

#[tokio::test]
async fn runner_reports_failure_for_failing_test() {
    let source = "#[test]\nfn a() -> i64 { 0 }\n#[test]\nfn b() -> i64 { 1 }";
    let config = TestConfig::default();
    let runner = TestRunner::new(config);
    let results = runner.run_all(source, &NoopReporter).await;
    assert_eq!(results.len(), 2);
    let a = results.iter().find(|r| r.name == "a").unwrap();
    let b = results.iter().find(|r| r.name == "b").unwrap();
    assert_eq!(a.outcome, crate::types::TestOutcome::Passed);
    assert!(matches!(b.outcome, crate::types::TestOutcome::Failed { .. }));
}
