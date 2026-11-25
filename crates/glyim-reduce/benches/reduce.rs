//! Reduction-throughput bench — the plan M4.5 mechanism, measured.
//!
//! The plan's M4.5 `DoD` bar is a *quality* bar ("median reduced size
//! < 40 lines on a 20-bug sample", owned by the M4.1 campaign); this
//! bench pins the *cost* side the campaign will plan around: how many
//! driver-seam calls and how much wall time one bloated repro costs at
//! each granularity tier, and that the fixed point still reproduces.
//!
//! Dependency-free harness (`harness = false`), mirroring the repo's
//! bench convention (`std::time::Instant` + `std::hint::black_box`): the
//! mixed workload reduces three repro shapes — an item-only bloat, a
//! body-only bloat, and the combined shape the M4.1 campaign's witnesses
//! look like — and prints lines, seam calls, and seconds per program.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-reduce --bench reduce
//! ```

use glyim_reduce::{DiagnosticRepro, ExitRepro, Reducer};
use std::hint::black_box;
use std::time::Instant;

/// Item-bloat: one defect inside `main` behind `PAD` inert top-level
/// functions. Drives the `"item"` tier to its fixed point.
fn item_bloated(pad: usize) -> String {
    let mut source = String::new();
    for index in 0..pad {
        let _ = std::fmt::Write::write_str(
            &mut source,
            &format!(
                "fn pad_{index}(x: i32) -> i32 {{\n    let mut acc = 0;\n    acc = acc + x * {index};\n    acc\n}}\n\n"
            ),
        );
    }
    source.push_str("fn main() -> i32 {\n    let defect = 5 / 0;\n    return defect;\n}\n");
    source
}

/// Body-bloat: one defect inside `main` behind `PAD` inert statements
/// (inert = individually removable without breaking a reference). Drives
/// the `"statement"` tier.
fn body_bloated(pad: usize) -> String {
    let mut source = String::from("fn main() -> i32 {\n");
    // An independent dead chain: each statement only reads itself, so the
    // statement tier can peel them one sweep at a time.
    for index in 0..pad {
        let _ = std::fmt::Write::write_str(
            &mut source,
            &format!("    let _dead{index} = {index} * 3 + 1;\n"),
        );
    }
    source.push_str("    let defect = 5 / 0;\n    return defect;\n}\n");
    source
}

/// One measured pass: reduce, verify the fixed point reproduces, return
/// (input lines, output lines, seam calls, elapsed).
fn measure(name: &str, check: &(impl glyim_reduce::ReproCheck + ?Sized), source: &str) {
    let started = Instant::now();
    let reduced = Reducer::new(check).reduce(source);
    let elapsed = started.elapsed();
    assert!(
        check.still_reproduces(reduced.source()),
        "{name}: the bench's fixed point must still reproduce"
    );
    black_box(reduced.line_count());
    println!(
        "{name}: {} -> {} lines, {} seam calls, {} passes, {elapsed:.2?}",
        reduced.original_line_count(),
        reduced.line_count(),
        reduced.check_calls(),
        reduced.passes().len()
    );
}

fn main() {
    let exit_check = ExitRepro::new(2, 0);
    println!("== exit-pinned reduction (-O2 silent-fold repro) ==");
    measure("item-bloat x24", &exit_check, &item_bloated(24));
    measure("item-bloat x48", &exit_check, &item_bloated(48));
    measure("body-bloat x32", &exit_check, &body_bloated(32));
    measure("body-bloat x64", &exit_check, &body_bloated(64));

    // Compile-fail reduction goes through the other seam: the GAP-0008
    // shape (leading `|` on a match arm) bloated with inert items, pinned
    // to the full ordered cascade so mangled variants stop reproducing.
    let diag_check = DiagnosticRepro::cascade(&[
        "expected pattern, found Or",
        "expected FatArrow, found IntLit",
        "expected pattern, found FatArrow",
        "expected FatArrow, found IntLit",
        "expected pattern, found FatArrow",
        "expected FatArrow, found IntLit",
    ]);
    let mut compile_fail = String::new();
    for index in 0..12 {
        let _ = std::fmt::Write::write_str(
            &mut compile_fail,
            &format!(
                "fn pad_{index}(x: i32) -> i32 {{\n    let mut acc = 0;\n    acc = acc + x;\n    acc\n}}\n\n"
            ),
        );
    }
    compile_fail.push_str(
        "fn main() -> i32 {\n    let r = match 2 {\n        | 1 => 10\n        | 2 => 20\n        _ => 30\n    };\n    return r;\n}\n",
    );
    println!("== diagnostic-pinned reduction (GAP-0008 shape) ==");
    measure("compile-fail x12", &diag_check, &compile_fail);
}
