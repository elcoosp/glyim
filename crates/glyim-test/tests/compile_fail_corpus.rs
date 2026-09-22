//! Compile-fail corpus: each fixture declares the diagnostic it must produce.
//!
//! Fixtures under `tests/compile-fail/` carry `//~ ERROR <pattern>`
//! annotations (see the harness's `Annotation` parser) and are compiled
//! through the full pipeline. The test asserts every annotated diagnostic is
//! produced at the annotated line, and that no *unexpected* errors appear.
//!
//! This is the "does the compiler reject bad programs correctly?" gate that
//! complements the run-pass corpus. Together they make the compiler's
//! behaviour — accept good, reject bad, in the right place with the right
//! message — a first-class, regression-proof contract.

use glyim_test::harness::executor::TestOutcome;
use glyim_test::harness::{TestMode, TestRunner};

fn corpus_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("compile-fail")
}

#[test]
fn compile_fail_corpus_passes() {
    let plan = TestRunner::new(corpus_root())
        .parallel(false)
        .build()
        .expect("compile-fail corpus collection must succeed");

    assert!(
        !plan.tests.is_empty(),
        "the compile-fail corpus must not be empty (fixtures missing?)"
    );

    // A fixture that does not declare an explicit mode is treated as
    // `CompilePass` by the harness; assert each one is `CompileFail` so a
    // missing `// test-mode: compile-fail` header fails loudly rather than
    // silently passing.
    for t in &plan.tests {
        assert_eq!(
            t.config.mode,
            TestMode::CompileFail,
            "fixture {} must declare `// test-mode: compile-fail`",
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
        "{} compile-fail fixture(s) failed:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}
