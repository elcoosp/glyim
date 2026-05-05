use crate::flaky::FlakeTracker;
use crate::types::TestOutcome;

#[test]
fn new_tracker_score_zero() {
    let tracker = FlakeTracker::new();
    assert_eq!(tracker.score("any", &TestOutcome::Passed), 0.0);
}

#[test]
fn record_does_not_panic() {
    let tracker = FlakeTracker::new();
    tracker.record("test", &TestOutcome::Failed { exit_code: 1, stderr: "".into() });
    // no panic
}
