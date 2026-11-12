//! M1.8 consumer walkthrough: run the level-computation engine over the
//! readiness ledger and print the per-feature readiness report — the same
//! report `glyim-ledger compute [ROOT]` prints.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example compute_levels
//! ```
//!
//! The default evidence source is `ledger/evidence/*.jsonl`; with no
//! committed evidence yet every feature computes an honest L0 (declared),
//! which is the point: levels move only when fixtures record evidence.
//! An optional first argument overrides the project root (the directory
//! that contains `ledger/`); the default infers the repository root from
//! `CARGO_MANIFEST_DIR`, so the example works from any working directory.
//! A second optional argument is an explicit evidence JSONL path
//! (mirroring `--evidence`).
//!
//! The tail of the example also renders the `glyim.ledger-compute/1` JSON
//! contract for the same run — the machine-readable form `glyim-pilot`
//! and future CI gates consume — so one pass shows both faces of the seam.

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let evidence = args.next().map(PathBuf::from);

    let outcome = glyim_ledger::run_compute(&root, evidence.as_deref());
    print!("{}", glyim_ledger::format_compute_report(&outcome));

    // Per-level histogram: the one-number-per-level summary the STATUS
    // renderers (plan M1.9) build on.
    let spellings = ["L0", "L1", "L2", "L3", "L4", "L5"];
    let mut histogram = [0usize; 6];
    for entry in &outcome.entries {
        histogram[entry.computed_level as usize] += 1;
    }
    print!("levels:");
    for (index, count) in histogram.iter().enumerate() {
        print!(" {}={count}", spellings[index]);
    }
    println!();

    if !outcome.is_ok() {
        std::process::exit(1);
    }
}
