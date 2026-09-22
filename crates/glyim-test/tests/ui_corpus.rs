//! UI diagnostic-snapshot corpus.
//!
//! Fixtures under `tests/ui/` compile (and fail), and the exact rendered
//! diagnostic is snapshotted into a sibling `.expected` file. Run with
//! `GLYIM_BLESS=1` to (re)write the snapshots.
//!
//! This freezes the *user-visible* diagnostic — message, code, location, and
//! source excerpt — so a change to any of them fails loudly instead of
//! silently drifting.

use glyim_test::harness::executor::TestOutcome;
use glyim_test::harness::{TestMode, TestRunner};

fn corpus_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("ui")
}

#[test]
fn ui_corpus_matches_snapshots() {
    let plan = TestRunner::new(corpus_root())
        .parallel(false)
        .build()
        .expect("ui corpus collection must succeed");

    assert!(
        !plan.tests.is_empty(),
        "the ui corpus must not be empty (fixtures missing?)"
    );

    for t in &plan.tests {
        assert_eq!(
            t.config.mode,
            TestMode::Ui,
            "fixture {} must declare `// test-mode: ui`",
            t.name
        );
    }

    let result = plan.execute();

    let mut failures = Vec::new();
    for r in &result.results {
        if let TestOutcome::Failed { reason } = &r.outcome {
            failures.push(format!("  {}: {:?}", r.test.name, reason));
        }
    }

    assert!(
        failures.is_empty(),
        "{} ui fixture(s) failed:\\n{}\\n\\n(run with GLYIM_BLESS=1 to update snapshots)",
        failures.len(),
        failures.join("\\n"),
    );
}
