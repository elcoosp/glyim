//! M5 (async v1) runtime-proof integration driver.
//!
//! Discovers the `tests/runtime/m5/**` glyph fixtures (run-pass / compile-fail
//! modes) and drives them through the real full-pipeline compiler + native
//! linker + executor. This is the host-run `two_step` proof path: compile an
//! `async fn`, link it to a binary, and (for run-pass) execute it and assert its
//! output.
//!
//! Fixtures are gated with `// only-target: x86_64-unknown-linux-gnu`. The
//! executor is configured with that same target triple, so on a non-Linux host
//! the pipeline still links a Linux ELF but cannot *run* it; the run-pass
//! fixture then `Ignored` rather than a silent miscompile. On the
//! `test-linux-runtime` (ubuntu-latest) job the produced ELF actually runs, so
//! the contract asserted here is:
//!   * `m5/two_step.g` (multi-await) MUST compile cleanly through the real
//!     desugar (it must NOT surface the `async-v2` diagnostic, error 61, which
//!     is reserved for genuinely non-nameable futures) and must `Passed`
//!     (run and print `3`).
//!   * `m5/one_step.g` (single-await run-pass) must `Passed` (run and print
//!     `42`). The MIR interpreter already verifies both shapes end-to-end
//! (see `glyim-pipeline::async_multi_await_runtime`); this driver enforces the
//! *native* LLVM-codegen + link + execute proof on Linux.
//!
//! STATUS (2026-08-28): the harness now compiles with the REAL `LlvmBackend`
//! (feature `real-llvm`, enabled by `GLYIM_TEST_REAL_LLVM` on the Linux job)
//! and links a runnable ELF, and this driver is now STRICT — it requires
//! `Passed` (real codegen + link + run) and no longer tolerates
//! `CompilationFailed`. That closes the previous blind spot where a native
//! codegen gap was silently accepted. NOTE: the native async codegen path is
//! CURRENTLY BROKEN (tracked gap) — `glyim-codegen-llvm` panics in
//! `fn_abi_of` (`lower.rs`) for the monomorphized `block_on<F>` / `poll` types,
//! and the bytecode backend ICEs in `type_arena` (`ty_kind` OOB). So this job
//! will currently FAIL on these fixtures until that codegen gap is fixed; the
//! failure is the correct, honest signal (it used to pass by tolerance). The
//! MIR-interpreter runtime proof (`async_multi_await_runtime`) remains the
//! verified end-to-end execution evidence today.

use glyim_test::harness::executor::TestOutcome;
use glyim_test::harness::{TestMode, TestRunner};

fn m5_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("runtime")
        .join("m5")
}

/// The async-v2 diagnostic (error 61) message substring asserted for multi-await.
const ASYNC_V2_SUBSTRING: &str = "multi-`.await` bodies are not yet supported";

#[test]
fn m5_two_step_multi_await_compiles_cleanly() {
    // M4 contract: the supported multi-await shape (`two_step`, two direct
    // calls to `async fn`) compiles cleanly through the REAL
    // `desugar_multi_async_fn` HIR state-machine transform — it must NOT emit
    // the `async-v2` diagnostic (error 61), which is reserved for genuinely
    // non-nameable futures. This guards against regressing to the old broken
    // behavior where the supported shape was rejected.
    //
    // On a non-Linux host the harness `Ignored` this run-pass fixture (the
    // executor is Linux-gated), so it does not execute there. On the
    // `test-linux-runtime` (ubuntu-latest) job the real `LlvmBackend` path
    // (enabled via `GLYIM_TEST_REAL_LLVM`) links a runnable ELF and the runner
    // executes it; a wrong output or a `CompilationFailed` (no executable) is a
    // genuine regression we MUST catch, so we no longer tolerate that failure
    // mode there.
    let real_llvm = std::env::var("GLYIM_TEST_REAL_LLVM").is_ok();
    let plan = TestRunner::new(m5_root())
        .parallel(false)
        .build()
        .expect("m5 fixture collection must succeed");
    let result = plan.execute();

    let two_step = result
        .results
        .iter()
        .find(|r| r.test.name.contains("two_step"))
        .expect("m5/two_step.g fixture must be discovered");

    // The supported shape must NOT be rejected with the async-v2 diagnostic.
    let emitted_async_v2 = two_step
        .diagnostics
        .iter()
        .any(|d| d.message.contains(ASYNC_V2_SUBSTRING));
    assert!(
        !emitted_async_v2,
        "supported multi-await (direct async-fn calls) must NOT emit the async-v2 \
         diagnostic (error 61); diagnostics: {:?}",
        two_step.diagnostics,
    );

    // On the Linux runner (real LLVM backend) the fixture must have produced a
    // runnable ELF and executed it (outcome `Passed`). A `CompilationFailed` /
    // wrong-output `Failed` is a real regression and is no longer tolerated. On
    // other hosts the mock backend cannot link, so we accept `CompilationFailed`
    // (it only proves the compile/desugar contract, not native execution).
    match &two_step.outcome {
        TestOutcome::Passed => { /* the real runtime proof: ran and printed 3 */ }
        TestOutcome::Failed { reason } if !real_llvm => {
            assert!(
                matches!(reason, glyim_test::error::FailureReason::CompilationFailed { .. }),
                "without the real LLVM backend the only tolerated failure is the known \
                 codegen gap (CompilationFailed / no executable); got {:?}",
                reason
            );
        }
        other => panic!(
            "m5/two_step.g must Pass on the Linux runner (real codegen + link + run); \
             got {:?} — this is a genuine regression, not a tolerated gap",
            other
        ),
    }
}

#[test]
fn m5_one_step_single_await_must_not_miscompile() {
    // Single-await is the verified-supported shape. On the Linux runner (real
    // LLVM backend via `GLYIM_TEST_REAL_LLVM`) it must produce a runnable ELF
    // and execute it, printing `42`. A `CompilationFailed` (no executable
    // produced) or a wrong-output `Failed` is a genuine regression — the
    // generic `Future`/`block_on` codegen gap is closed — and is no longer
    // tolerated there. On a non-Linux host the harness `Ignored` the fixture
    // (the executor is Linux-gated) or the mock backend cannot link (so we
    // tolerate `CompilationFailed`).
    let real_llvm = std::env::var("GLYIM_TEST_REAL_LLVM").is_ok();
    let plan = TestRunner::new(m5_root())
        .parallel(false)
        .build()
        .expect("m5 fixture collection must succeed");
    let result = plan.execute();

    let one_step = result
        .results
        .iter()
        .find(|r| r.test.name.contains("one_step"))
        .expect("m5/one_step.g fixture must be discovered");

    match &one_step.outcome {
        TestOutcome::Passed => { /* the real runtime proof: ran and printed 42 */ }
        TestOutcome::Ignored => { /* non-Linux host: executor is Linux-gated */ }
        TestOutcome::Failed { reason } if !real_llvm => {
            assert!(
                matches!(reason, glyim_test::error::FailureReason::CompilationFailed { .. }),
                "without the real LLVM backend the only tolerated failure is the known \
                 codegen gap (CompilationFailed / no executable); got {:?}",
                reason
            );
        }
        other => panic!(
            "m5/one_step.g must Pass on the Linux runner (real codegen + link + run); \
             got {:?} — this is a genuine regression, not a tolerated gap",
            other
        ),
    }
}

#[test]
fn m5_fixtures_present_and_moded_correctly() {
    let plan = TestRunner::new(m5_root())
        .parallel(false)
        .build()
        .expect("collection");
    let names: Vec<String> = plan.tests.iter().map(|t| t.name.clone()).collect();

    assert!(
        names.iter().any(|n| n.contains("one_step")),
        "missing m5/one_step.g fixture; have: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n.contains("two_step")),
        "missing m5/two_step.g fixture; have: {:?}",
        names
    );

    let one = plan
        .tests
        .iter()
        .find(|t| t.name.contains("one_step"))
        .expect("one_step fixture");
    assert_eq!(one.config.mode, TestMode::RunPass, "m5/one_step must be run-pass");
    assert_eq!(
        one.config.only_target.as_deref(),
        Some("x86_64-unknown-linux-gnu"),
        "m5/one_step must be gated to linux"
    );

    let two = plan
        .tests
        .iter()
        .find(|t| t.name.contains("two_step"))
        .expect("two_step fixture");
    assert_eq!(
        two.config.mode,
        TestMode::RunPass,
        "m5/two_step must be run-pass (M4 desugar compiles it cleanly)"
    );
}
