//! `run-fail` corpus: programs that must compile, execute, and exit with
//! the exit code declared by `// exit-code: N` (default 1).
//!
//! Distinct from `compile-fail` (rejected at compile time) and `run-pass`
//! (must exit 0). This is the only corpus that exercises runtime error
//! paths — panics, `assert!` failures, division by zero — end to end.

use glyim_test::harness::TestRunner;
use glyim_test::harness::executor::TestOutcome;

fn corpus_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("run-fail")
}

#[test]
fn run_fail_corpus_passes() {
    let plan = TestRunner::new(corpus_root())
        .parallel(false)
        .build()
        .expect("run-fail corpus collection must succeed");

    assert!(
        !plan.tests.is_empty(),
        "the run-fail corpus must not be empty (fixtures missing?)"
    );

    let result = plan.execute();

    let mut failures = Vec::new();
    for r in &result.results {
        if let TestOutcome::Failed { reason } = &r.outcome {
            failures.push(format!("  {}: {:?}", r.test.name, reason));
        }
    }

    assert!(
        failures.is_empty(),
        "{} run-fail fixture(s) failed:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}
