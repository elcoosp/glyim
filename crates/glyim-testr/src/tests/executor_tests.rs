use glyim_testr::executor::Executor;
use glyim_testr::types::TestOutcome;
use std::time::Duration;

#[tokio::test]
async fn executor_runs_test_and_reports_pass() {
    let dir = tempfile::tempdir().unwrap();
    let mock_path = dir.path().join("mock_test");
    std::fs::write(&mock_path, "#!/bin/sh\necho 'PASS my_test'\nexit 0\n").unwrap();
    std::fs::set_permissions(&mock_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let executor = Executor::new(mock_path, Duration::from_secs(5));
    let result = executor.run_test("my_test").await.expect("run test");
    assert_eq!(result.name, "my_test");
    assert_eq!(result.outcome, TestOutcome::Passed);
}

#[tokio::test]
async fn executor_reports_timeout_when_process_hangs() {
    let dir = tempfile::tempdir().unwrap();
    let mock = dir.path().join("mock");
    std::fs::write(&mock, "#!/bin/sh\nsleep 10\nexit 0\n").unwrap();
    std::fs::set_permissions(&mock, std::fs::Permissions::from_mode(0o755)).unwrap();

    let executor = Executor::new(mock, Duration::from_millis(200));
    let result = executor.run_test("hangs").await.expect("run test");
    assert_eq!(result.outcome, TestOutcome::TimedOut);
}

#[tokio::test]
async fn executor_reports_failed_on_nonzero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let mock = dir.path().join("mock");
    std::fs::write(&mock, "#!/bin/sh\necho 'FAIL my_test'\nexit 3\n").unwrap();
    std::fs::set_permissions(&mock, std::fs::Permissions::from_mode(0o755)).unwrap();

    let executor = Executor::new(mock, Duration::from_secs(2));
    let result = executor.run_test("my_test").await.expect("run test");
    match result.outcome {
        TestOutcome::Failed { exit_code, .. } => assert_eq!(exit_code, 3),
        other => panic!("expected Failed, got {:?}", other),
    }
}
