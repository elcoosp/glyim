//! Plan §3.9 consumer walkthrough: load the readiness ledger, rank the
//! work queue (`ledger next`), and render the human-readable report —
//! the same queue `glyim-ledger next [ROOT]` prints as
//! `glyim.ledger-next/1` JSON for `glyim-pilot`.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example next_queue
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

    let outcome = glyim_ledger::run_next(&root);
    print!("{}", glyim_ledger::format_next_report(&outcome));

    // Registry load/validation errors fail the exit code; the queue
    // itself is a report, so witness debt ranks items instead of failing.
    if !outcome.is_ok() {
        std::process::exit(1);
    }
}
