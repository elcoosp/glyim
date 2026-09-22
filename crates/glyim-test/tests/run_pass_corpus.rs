//! End-to-end run-pass / run-fail corpus.
//!
//! Each `.g` fixture under `tests/run-pass/` declares an expected exit code
//! (`// exit-code: N`) and must compile through the full pipeline and produce
//! that code. When the `real-llvm` feature is enabled *and*
//! `GLYIM_TEST_REAL_LLVM` is set the harness links and runs a native binary;
//! on every other host it falls back to the MIR interpreter, so this corpus
//! runs everywhere.
//!
//! This is the "does the compiler produce correct programs?" gate that the
//! per-crate unit tests cannot provide: it exercises parse → HIR → typeck →
//! MIR → opt → codegen/interp end-to-end, so a silent miscompile (the class
//! of bug that motivated this corpus — const-folded enum aggregates, wrong
//! field offsets, spurious generic instantiations) fails here instead of
//! surviving to a user.

use glyim_test::harness::executor::TestOutcome;
use glyim_test::harness::{TestRunner};

fn corpus_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("run-pass")
}

#[test]
fn run_pass_corpus_passes() {
    let plan = TestRunner::new(corpus_root())
        .parallel(false)
        .build()
        .expect("run-pass corpus collection must succeed");

    assert!(
        !plan.tests.is_empty(),
        "the run-pass corpus must not be empty (fixtures missing?)"
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
        "{} run-pass fixture(s) failed:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}
