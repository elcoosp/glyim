//! M1.9 (first slice) consumer walkthrough: render the computed readiness
//! status — the same marker-delimited Markdown block
//! `glyim-ledger status [ROOT]` prints, followed by the `glyim.status/1`
//! JSON contract for the same run.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example status_report
//! ```
//!
//! The block is delimited by `<!-- ledger:status:start/end -->` markers so
//! it can be embedded in hand-written documents (plan M1.9: generated
//! content never eats neighboring text). An optional first argument
//! overrides the project root; a second is an explicit evidence JSONL
//! path (mirroring `--evidence`).

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let evidence = args.next().map(PathBuf::from);

    let outcome = glyim_ledger::run_status(&root, evidence.as_deref());
    print!("{}", glyim_ledger::format_status_markdown(&outcome.compute));
    print!("{}", glyim_ledger::format_status_json(&outcome.compute));

    if !outcome.is_ok() {
        std::process::exit(1);
    }
}
