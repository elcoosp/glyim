#[allow(unused_imports, dead_code)]
use crate::common::*;

#[tokio::test]
async fn e2e_test_should_panic_passes() {
    let input = temp_g("#[test(should_panic)]\nfn panics() { 1 }");
    let source = std::fs::read_to_string(&input).unwrap();
    let results =
        glyim_testr::run_tests(&source, &glyim_testr::config::TestConfig::default()).await;
    let passed = results
        .iter()
        .filter(|r| matches!(r.outcome, glyim_testr::types::TestOutcome::Passed))
        .count();
    assert_eq!(passed, 1, "should_panic test should pass");
}

#[tokio::test]
async fn e2e_test_should_panic_fails_on_zero() {
    let input = temp_g("#[test(should_panic)]\nfn no_panic() { 0 }");
    let source = std::fs::read_to_string(&input).unwrap();
    let results =
        glyim_testr::run_tests(&source, &glyim_testr::config::TestConfig::default()).await;
    let failed = results
        .iter()
        .filter(|r| matches!(r.outcome, glyim_testr::types::TestOutcome::Failed { .. }))
        .count();
    assert_eq!(failed, 1, "should_panic test that returns 0 should fail");
}

#[tokio::test]
async fn e2e_test_filter() {
    let input = temp_g("#[test]\nfn a() { 0 }\n#[test]\nfn b() { 1 }");
    let source = std::fs::read_to_string(&input).unwrap();
    let config = glyim_testr::config::TestConfig {
        filter: Some("b".into()),
        ..Default::default()
    };
    let results = glyim_testr::run_tests(&source, &config).await;
    assert_eq!(results.len(), 1);
    assert!(matches!(
        results[0].outcome,
        glyim_testr::types::TestOutcome::Failed { .. }
    ));
}

#[tokio::test]
async fn e2e_test_filter_no_match() {
    let input = temp_g("#[test]\nfn a() { 0 }");
    let source = std::fs::read_to_string(&input).unwrap();
    let config = glyim_testr::config::TestConfig {
        filter: Some("nonexistent".into()),
        ..Default::default()
    };
    let results = glyim_testr::run_tests(&source, &config).await;
    assert!(results.is_empty(), "expected no test to match");
}
