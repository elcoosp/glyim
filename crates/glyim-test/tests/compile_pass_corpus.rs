//! `compile-pass` corpus: programs that must compile with zero error
//! diagnostics. No execution. This is the weakest assertion (it only proves
//! the front half accepts the program) but it is the only corpus covering
//! language features that are not exercised by a run-pass fixture.

use glyim_test::harness::TestRunner;
use glyim_test::harness::executor::TestOutcome;

fn corpus_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("compile-pass")
}

#[test]
fn compile_pass_corpus_passes() {
    let plan = TestRunner::new(corpus_root())
        .parallel(false)
        .build()
        .expect("compile-pass corpus collection must succeed");

    assert!(
        !plan.tests.is_empty(),
        "the compile-pass corpus must not be empty (fixtures missing?)"
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
        "{} compile-pass fixture(s) failed:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}
