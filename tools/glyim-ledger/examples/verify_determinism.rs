//! M1.13 (first slice) consumer walkthrough: run the determinism
//! self-check — the same report `glyim-ledger verify [ROOT]` prints,
//! followed by the `glyim.ledger-verify/1` JSON contract for the same run.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example verify_determinism
//! ```
//!
//! Every ledger pipeline (compute text/JSON/snapshot, status Markdown,
//! the M1.12 release-gate report text/JSON) runs twice from independent
//! load passes over the same root; the report names the checks whose
//! rendered bytes drifted. The command exits non-zero on drift or
//! error-severity diagnostics — this is a gate, not a report. An optional
//! first argument overrides the project root; a second is an explicit
//! evidence JSONL path (mirroring `--evidence`).

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let evidence = args.next().map(PathBuf::from);

    let outcome = glyim_ledger::verify_determinism(&root, evidence.as_deref());
    print!("{}", glyim_ledger::format_verify_text(&outcome));
    print!("{}", glyim_ledger::format_verify_json(&outcome));

    if !outcome.is_ok() {
        std::process::exit(1);
    }
}
