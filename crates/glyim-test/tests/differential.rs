//! Differential test: for every `run-pass` fixture, compile **once** and run
//! BOTH backends:
//!
//!   1. the LLVM-native binary (`--emit=exec` path, real `LlvmBackend`), and
//!   2. the MIR interpreter (`glyim-mir-interp`, the portable fallback).
//!
//! and assert they agree on observable behaviour (exit code + stdout).
//!
//! This is the class of test the existing corpora cannot provide: each corpus
//! exercises *one* backend in isolation, so a silent miscompile (the §4 bug:
//! `Ok(0)` vs `Ok(n)` discriminant collision) passes every test that only
//! checks the interp, and ships. Here, if either backend emits wrong code that
//! the other doesn't, the assertions fail and point at the fixture.
//!
//! Gated on `GLYIM_TEST_REAL_LLVM=1` (and the `real-llvm` feature) so it is a
//! no-op on hosts without an LLVM toolchain — matching the existing
//! `test-linux-runtime` CI job's contract.

#![cfg(feature = "real-llvm")]

use glyim_test::harness::TestRunner;
use glyim_test::harness::compiler::{PipelineCompiler, TestCompiler};
use glyim_test::harness::interpreter_runner::InterpRunner;
use glyim_test::harness::runner::ProgramRunner;
use glyim_test::mock::MockCodegen;

use std::sync::Arc;
use std::time::Duration;

// Interp + LLVM both run under load; a 10s wall-clock budget is tight
// enough that three consecutive full-suite re-runs (session 15) each
// flaked on a *different* fixture — always `llvm=true timeout`, always
// passing on isolated re-run. That's load-sensitive, not a disagreement.
// 30s gives the native path room under parallel load while still
// catching a genuine hang.
const TIMEOUT: Duration = Duration::from_secs(30);

fn corpus_root(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

/// Drive both backends over `corpus` and assert they agree on observable
/// behaviour (exit code + stdout).
///
/// Run over both `run-pass` and `run-fail`. The `run-fail` half is what
/// catches backend divergence on *panics*: before it existed, three
/// independent bugs made a panicking program die by signal on the LLVM
/// backend (SIGABRT / SIGILL / SIGFPE) while the interpreter reported the
/// declared `exit-code: 101`, and nothing failed because the run-fail
/// corpus only ever ran the interpreter.
fn check_corpus_agreement(corpus: &str) {
    if std::env::var("GLYIM_TEST_REAL_LLVM").is_err() {
        eprintln!(
            "differential[{corpus}]: GLYIM_TEST_REAL_LLVM not set — skipping \
             (set it to 1 with the `real-llvm` feature to run the LLVM side)."
        );
        return;
    }

    let plan = TestRunner::new(corpus_root(corpus))
        .parallel(false)
        .build()
        .expect("corpus collection must succeed");

    assert!(
        !plan.tests.is_empty(),
        "the differential[{corpus}] corpus must not be empty (fixtures missing?)"
    );

    let compiler = PipelineCompiler::new(Arc::new(MockCodegen::new())).with_real_llvm();

    let mut disagreements: Vec<String> = Vec::new();
    let mut interp_only: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut compared: usize = 0;

    for test in &plan.tests {
        // ---- skip rules: same contract as the executor's `execute_test`. ----
        if test.config.ignore {
            skipped.push(test.name.clone());
            continue;
        }
        if test.config.needs_llvm && std::env::var("GLYIM_LLVM").is_err() {
            skipped.push(test.name.clone());
            continue;
        }
        if test.config.only_target.is_some() {
            skipped.push(test.name.clone());
            continue;
        }

        let mut flags = test.config.compile_flags.clone();
        for rev in &test.revisions {
            if let Some(rf) = test.config.revision_compile_flags.get(rev) {
                flags.extend(rf.iter().cloned());
            }
        }

        let file_id = glyim_span::FileId::from_raw(0);
        // Per-fixture panic fence: the LLVM backend is currently known to ICE
        // on some monomorphizations (`fn_abi_of failed: UnknownType(Ty(NNN))`).
        // Without this, the first ICE aborts the whole test and we can't see
        // the rest of the disagreement inventory. Capture the panic and record
        // it as a disagreement so the report is complete.
        let compile_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compiler.compile(&test.source, &test.path, file_id, &flags)
        }));
        let out = match compile_result {
            Ok(o) => o,
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| {
                        payload
                            .downcast_ref::<&'static str>()
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                // Keep the fixture name + first line of the panic for the
                // report. Full backtraces go to stderr already.
                let first_line = msg.lines().next().unwrap_or("").to_string();
                disagreements.push(format!(
                    "{}: LLVM backend panicked: {}",
                    test.name, first_line
                ));
                continue;
            }
        };

        // A fixture that fails to compile cannot be diffed; the run-pass
        // corpus already covers "compiles cleanly", so record it here to
        // keep the output honest rather than silently swallowing it.
        let errs: Vec<String> = out
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| d.message.clone())
            .collect();
        if !errs.is_empty() {
            disagreements.push(format!("{}: compile errors: {:?}", test.name, errs));
            continue;
        }

        // ---- interp side: always runnable. ----
        let Some(ty_ctx) = out.ty_ctx.clone() else {
            disagreements.push(format!("{}: no ty_ctx produced", test.name));
            continue;
        };
        let interp = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            InterpRunner::new(out.mir_bodies.clone(), ty_ctx, out.entry_main).run(TIMEOUT)
        })) {
            Ok(r) => r,
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| {
                        payload
                            .downcast_ref::<&'static str>()
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                disagreements.push(format!(
                    "{}: interpreter panicked: {}",
                    test.name,
                    msg.lines().next().unwrap_or("")
                ));
                continue;
            }
        };

        // ---- LLVM side: only when the backend produced an executable. ----
        let Some(exe) = out.executable_path.as_deref() else {
            // The mock backend (or a host without a linker) doesn't emit an
            // executable. Record it; do not fail. This is the "no LLVM side
            // to compare against" bucket, distinct from a real disagreement.
            interp_only.push(test.name.clone());
            continue;
        };
        let native = ProgramRunner::new(exe).run(TIMEOUT);

        if native.timed_out || interp.timed_out {
            disagreements.push(format!(
                "{}: timeout (interp={}, llvm={})",
                test.name, interp.timed_out, native.timed_out
            ));
            continue;
        }

        // ---- compare observable behaviour. ----
        compared += 1;
        if native.exit_code != interp.exit_code {
            disagreements.push(format!(
                "{}: exit mismatch (interp={:?}, llvm={:?})",
                test.name, interp.exit_code, native.exit_code
            ));
        }
        if native.stdout != interp.stdout {
            disagreements.push(format!(
                "{}: stdout mismatch\n    interp: {:?}\n    llvm:   {:?}",
                test.name, interp.stdout, native.stdout
            ));
        }
    }

    eprintln!(
        "differential[{corpus}]: compared={} disagreements={} interp_only={} skipped={}",
        compared,
        disagreements.len(),
        interp_only.len(),
        skipped.len()
    );
    for d in &interp_only {
        eprintln!("  interp-only (no exec binary): {}", d);
    }
    for d in &skipped {
        eprintln!("  skipped: {}", d);
    }

    assert!(
        disagreements.is_empty(),
        "LLVM/interp disagreement[{corpus}] on {} of {} compared fixture(s):\n  {}",
        disagreements.len(),
        compared,
        disagreements.join("\n  "),
    );
}

#[test]
fn llvm_and_interp_agree_on_run_pass_corpus() {
    check_corpus_agreement("run-pass");
}

#[test]
fn llvm_and_interp_agree_on_run_fail_corpus() {
    check_corpus_agreement("run-fail");
}
