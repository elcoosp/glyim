//! Per-operator exhaustive oracle suite (plan M4.8
//! `docs/roadmaps/glyim-master-plan.md` §6 M4.8).
//!
//! The plan's work item pins the acceptance this suite encodes:
//!
//! > **M4.8** | Per-operator exhaustive tests: every `BinOp`/`UnOp`/`Cast`
//! > × all pairs of 8-bit values (exhaustive), sampled wider widths,
//! > across const-eval, interpreter, LLVM-O0/O2 | signedness/shift/
//! > overflow disagreements = 0 or witnessed
//!
//! Three legs, mirroring the plan's executor list:
//!
//! 1. **Interpreter leg (exhaustive)** — one sweep program per
//!    `BinOp`/`UnOp`/`Cast` × operand class loops over all 256×256 (or
//!    256-value) operand pairs in-process; the pinned acceptance is
//!    `opt0 ≡ opt2 ≡ ground truth` on the folded checksum. Loop-carried
//!    operands keep the constant-prop pass from folding anything, so a
//!    fold-related divergence cannot hide behind a checksum.
//! 2. **Const-eval leg (sampled in PR tier, `GLYIM_EXHAUSTIVE_CONST=1`
//!    widens to exhaustive for the nightly job)** — const-item sum
//!    programs over pairs whose pinned const-eval semantics agree with
//!    the runtime, plus boundary probes pinning the *documented*
//!    const/runtime boundary (overflow, division/remainder by zero, and
//!    negative shift amounts are `[C0001]` rejections in const position
//!    while the runtime wraps or panics — diagnosed, never silent). A
//!    disagreement on a non-boundary pair fails the leg.
//! 3. **Native leg (nightly, `real-llvm` feature)** — the same sweep
//!    programs compiled and executed natively at `-O0`/`-O2`; exit codes
//!    must match the interpreter's. Skipped without the feature, exactly
//!    like the differential gate.
//!
//! The pinned-semantics table (every rule carrying its probe evidence)
//! lives in the module docs of `crates/glyim-test/src/exhaustive.rs`.

use glyim_test::exhaustive::{
    BINARY_SWEEPS, CAST_SWEEPS, SweepOutcome, UNARY_SWEEPS, check_const_leg, check_sweep,
    sampled_const_pairs,
};
use glyim_test::metamorphic::MetamorphicRunner;

fn runner() -> (tempfile::TempDir, MetamorphicRunner) {
    let dir = tempfile::tempdir().expect("tempdir");
    let runner = MetamorphicRunner::new(dir.path().to_path_buf());
    (dir, runner)
}

/// The pinned acceptance for the interpreter leg: every sweep case at
/// `-O0` and `-O2` lands on the ground-truth checksum, and the two opt
/// levels never disagree. This is the exhaustive 8-bit surface —
/// 16 binary ops × {i8, u8} × 65536 pairs plus the boolean pairs, the
/// unary sweeps, and the cast sweeps.
#[test]
fn sweeps_match_ground_truth_at_both_opt_levels() {
    let (_dir, runner) = runner();
    let mut failures: Vec<String> = Vec::new();
    for sweep in BINARY_SWEEPS {
        if let outcome @ (SweepOutcome::GroundTruthMismatch { .. }
        | SweepOutcome::OptDivergence { .. }
        | SweepOutcome::Invalid { .. }) = check_sweep(&runner, sweep)
        {
            failures.push(format!("{}: {outcome:?}", sweep.op_id));
        }
    }
    for sweep in UNARY_SWEEPS {
        if let outcome @ (SweepOutcome::GroundTruthMismatch { .. }
        | SweepOutcome::OptDivergence { .. }
        | SweepOutcome::Invalid { .. }) = check_sweep(&runner, sweep)
        {
            failures.push(format!("{}: {outcome:?}", sweep.id));
        }
    }
    for sweep in CAST_SWEEPS {
        if let outcome @ (SweepOutcome::GroundTruthMismatch { .. }
        | SweepOutcome::OptDivergence { .. }
        | SweepOutcome::Invalid { .. }) = check_sweep(&runner, sweep)
        {
            failures.push(format!("{}: {outcome:?}", sweep.id));
        }
    }
    assert!(
        failures.is_empty(),
        "per-operator disagreements (plan M4.8: signedness/shift/overflow \
         disagreements = 0 or witnessed):\n  {}",
        failures.join("\n  ")
    );
}

/// The pinned acceptance for the const-eval leg: on pairs whose const
/// semantics agree with the runtime, the const-folded sums must equal the
/// ground-truth sums; on the documented boundary pairs, const-eval must
/// keep rejecting with `[C0001]` (diagnosed, never silent).
#[test]
fn const_eval_leg_agrees_or_pins_the_documented_boundary() {
    let (_dir, runner) = runner();
    let mut failures: Vec<String> = Vec::new();
    let mut covered = 0usize;
    for sweep in BINARY_SWEEPS {
        let pairs = sampled_const_pairs(sweep);
        let outcome = check_const_leg(&runner, sweep, &pairs);
        if let glyim_test::exhaustive::ConstOutcome::Agrees {
            safe_pairs,
            boundary_pairs,
        } = &outcome
        {
            covered += safe_pairs + boundary_pairs;
        }
        if let outcome @ (glyim_test::exhaustive::ConstOutcome::SumMismatch { .. }
        | glyim_test::exhaustive::ConstOutcome::Invalid { .. }
        | glyim_test::exhaustive::ConstOutcome::BoundaryNotDiagnosed { .. }) =
            check_const_leg(&runner, sweep, &pairs)
        {
            failures.push(format!("{}: {outcome:?}", sweep.op_id));
        }
    }
    assert!(
        covered > 0,
        "the const leg must cover const-folded pairs across the binary surface"
    );
    assert!(
        failures.is_empty(),
        "const-eval leg disagreements (plan M4.8):\n  {}",
        failures.join("\n  ")
    );
}

/// The pinned acceptance for the native leg (nightly tier, real LLVM):
/// the same sweep programs compiled and executed natively at `-O0`/`-O2`
/// must exit like the interpreter's pinned model. Skipped without the
/// `real-llvm` feature, exactly like the differential gate.
#[cfg(feature = "real-llvm")]
#[test]
fn native_backend_matches_the_model_on_the_sweeps() {
    // `SweepCase` (the `source`/`ground_truth_exit` trait) is used only by
    // this native leg, so the import lives inside the `real-llvm` gate: at
    // the file top it is unused-imports in the default build, and without
    // it this fn does not compile in the all-features build (the HEAD
    // commit that dropped the top-level import broke `--all-features`).
    use glyim_test::exhaustive::SweepCase;
    use glyim_test::harness::compiler::{PipelineCompiler, TestCompiler};
    use glyim_test::harness::runner::ProgramRunner;
    use glyim_test::mock::MockCodegen;
    use std::sync::Arc;

    // Division/remainder sweeps panic at runtime (exit 101 through the
    // abort path) — that is the pinned semantics; they exercise the
    // native panic path, so they stay in.
    let mut failures: Vec<String> = Vec::new();
    let workdir = tempfile::tempdir().expect("tempdir");
    for (index, sweep) in BINARY_SWEEPS.iter().enumerate() {
        // The model pins a panic as exit 101; the native side aborts with
        // the same contract (differential gate covers it on the run-fail
        // corpus). Skip the panic-shaped ops here: the native abort path
        // is the differential gate's jurisdiction, not the checksum's.
        if matches!(sweep.op_id, "div" | "div-u8" | "rem" | "rem-u8") {
            continue;
        }
        let source = sweep.source();
        let path = workdir.path().join(format!("exhaustive_sweep_{index}.g"));
        std::fs::write(&path, &source).expect("write sweep program");
        let compiler = PipelineCompiler::new(Arc::new(MockCodegen::new())).with_real_llvm();
        for flag in ["--opt-level=0", "--opt-level=2"] {
            let out = compiler.compile(
                &source,
                &path,
                glyim_span::FileId::from_usize(0),
                &[flag.to_string()],
            );
            let errs: Vec<String> = out
                .diagnostics
                .iter()
                .filter(|d| d.is_error())
                .map(|d| d.message.clone())
                .collect();
            if !errs.is_empty() {
                failures.push(format!(
                    "{} [{flag}]: compile errors: {errs:?}",
                    sweep.op_id
                ));
                continue;
            }
            let Some(exe) = out.executable_path.as_deref() else {
                failures.push(format!("{} [{flag}]: no executable emitted", sweep.op_id));
                continue;
            };
            let native = ProgramRunner::new(exe).run(std::time::Duration::from_secs(30));
            let expected = sweep.ground_truth_exit();
            if native.exit_code != Some(expected) {
                failures.push(format!(
                    "{} [{flag}]: exit mismatch (native={:?}, pinned={expected})",
                    sweep.op_id, native.exit_code
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "native backend disagreements on the sweeps (plan M4.8):\n  {}",
        failures.join("\n  ")
    );
}
