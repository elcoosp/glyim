//! Sweep-throughput bench — the plan M4.8 exhaustive oracle, measured.
//!
//! The plan's M4.8 `DoD` bar is a *disagreement-count* bar ("signedness/
//! shift/overflow disagreements = 0 or witnessed"); this bench pins the
//! *cost* side the nightly budget plans around: how many operand pairs
//! per second the oracle drives through the interpreter seam at each
//! opt level, and how fast the const-eval leg folds pairs. The workload
//! is the full exhaustive sweep for one arithmetic operator per operand
//! class (65536 pairs each) plus one sampled const-sum program.
//!
//! Dependency-free harness (`harness = false`), mirroring the repo's
//! bench convention (`std::time::Instant` + `std::hint::black_box`).
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-test --bench exhaustive
//! ```

use glyim_test::exhaustive::{BINARY_SWEEPS, check_sweep, sampled_const_pairs};
use glyim_test::metamorphic::MetamorphicRunner;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let dir = tempfile::tempdir().expect("tempdir");
    let runner = MetamorphicRunner::new(dir.path().to_path_buf());

    // Full exhaustive sweeps: 65536 pairs per op per opt level, both
    // levels, through the interpreter seam.
    println!("== interpreter sweeps (65536 pairs x 2 opt levels) ==");
    for sweep in BINARY_SWEEPS
        .iter()
        .filter(|s| matches!(s.op_id, "add" | "add-u8" | "mul" | "mul-u8"))
    {
        let started = Instant::now();
        let outcome = check_sweep(&runner, sweep);
        let elapsed = started.elapsed();
        let invariant = matches!(outcome, glyim_test::exhaustive::SweepOutcome::Invariant);
        black_box(invariant);
        let pairs = if sweep.bool_domain { 4 } else { 65536 };
        let per_second = f64::from(pairs * 2) / elapsed.as_secs_f64();
        println!(
            "{}: {pairs} pairs x2 opts in {elapsed:.2?} ({per_second:.0} pairs/s), invariant={invariant}",
            sweep.op_id
        );
    }

    // Const-eval leg: sampled pairs folded through the const evaluator.
    println!("== const-eval leg (256 sampled pairs per op) ==");
    for sweep in BINARY_SWEEPS
        .iter()
        .filter(|s| matches!(s.op_id, "add" | "add-u8"))
    {
        let pairs = sampled_const_pairs(sweep);
        let started = Instant::now();
        let outcome = glyim_test::exhaustive::check_const_leg(&runner, sweep, &pairs);
        let elapsed = started.elapsed();
        let agrees = matches!(outcome, glyim_test::exhaustive::ConstOutcome::Agrees { .. });
        black_box(agrees);
        let per_second = f64::from(pairs.len() as u32) / elapsed.as_secs_f64();
        println!(
            "{}: {} const pairs folded in {elapsed:.2?} ({per_second:.0} pairs/s), agrees={agrees}",
            sweep.op_id,
            pairs.len()
        );
    }
}
