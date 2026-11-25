//! Consumer walkthrough: reduce a real, committed strict-xfail witness
//! fixture to a minimal repro (plan M4.5's "minimal repro + witness
//! fixture + gap stub per new bug").
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-reduce --example reduce_witness
//! ```
//!
//! The witness is GAP-0008's compile-fail fixture
//! (`crates/glyim-test/tests/compile-fail/match-arm-leading-or-known-gap.g`):
//! a leading `|` on a match arm emits a spurious `[P0001] expected
//! pattern` cascade. The example drives the same in-process seam the
//! compile-fail corpus drives, pins the fixture's full ordered diagnostic
//! cascade (the corpus' "every diagnostic stays consumed, nothing else
//! may appear" contract, read off the fixture's own `//~` annotations),
//! and lets the CST ladder shrink the witness to its fixed point — which
//! still reproduces the identical cascade.
//!
//! This is the workflow a driver (M4.4's differential, the M4.1
//! re-verification campaign) runs per new bug: pin the observed failure,
//! reduce, and the fixed point is what becomes (or replaces) the
//! witnessed gap's fixture.

use glyim_reduce::{DiagnosticRepro, Reducer, ReproCheck};
use std::fs;
use std::path::PathBuf;

/// The GAP-0008 cascade, read off the witness fixture's own `//~`
/// annotation column: an `Or` in pattern position cascades into a
/// strict, ordered recovery sequence, emitted once per arm shape.
const GAP_0008_CASCADE: [&str; 6] = [
    "expected pattern, found Or",
    "expected FatArrow, found IntLit",
    "expected pattern, found FatArrow",
    "expected FatArrow, found IntLit",
    "expected pattern, found FatArrow",
    "expected FatArrow, found IntLit",
];

fn main() {
    let witness = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("glyim-test")
        .join("tests")
        .join("compile-fail")
        .join("match-arm-leading-or-known-gap.g");
    let source = fs::read_to_string(&witness)
        .unwrap_or_else(|error| panic!("read witness fixture {}: {error}", witness.display()));
    // The witness carries `//@` header directives; the compiler seam
    // consumes them as comments, so the raw fixture body is what the
    // reducer sees — matching what the corpus harness reads.
    let original_lines = source.trim_end().lines().count();

    println!("witness: {}", witness.display());
    println!("original: {original_lines} lines");

    let check = DiagnosticRepro::cascade(&GAP_0008_CASCADE);
    let reduced = Reducer::new(&check).reduce(&source);

    println!("{reduced}");
    println!("passes:");
    for pass in reduced.passes() {
        println!(
            "  {} tier: {} committed deletion(s)",
            pass.granularity, pass.committed
        );
    }
    println!("--- reduced repro ---");
    println!("{}", reduced.source());
    assert!(
        check.still_reproduces(reduced.source()),
        "the reduced witness must still reproduce the pinned cascade"
    );
    assert!(
        reduced.line_count() < original_lines,
        "the reducer must shrink the witness (M4.5: minimal repro)"
    );
}
