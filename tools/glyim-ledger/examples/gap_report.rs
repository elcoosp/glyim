//! M1.7 consumer walkthrough: load the readiness ledger's gap registry
//! and render the witness-debt report — the same report
//! `glyim-ledger gaps [ROOT]` prints.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example gap_report
//! ```
//!
//! An optional argument overrides the project root (the directory that
//! contains `ledger/`); the default infers the repository root from
//! `CARGO_MANIFEST_DIR`, so the example works from any working directory.

use std::path::PathBuf;

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));

    let outcome = glyim_ledger::run_gaps(&root);
    print!("{}", glyim_ledger::format_gap_report(&outcome));

    // Witness debt is rendered, not failed (`check` is the gate); only
    // error-severity registry diagnostics fail the exit code.
    if !outcome.is_ok() {
        std::process::exit(1);
    }
}
