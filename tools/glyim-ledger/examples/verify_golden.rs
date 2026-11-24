//! M1.13 reproducibility-golden consumer walkthrough: compare the current
//! renders against the committed golden (`ledger/verify-golden.json`) —
//! the same comparison the `reproducibility` CI job runs — and print the
//! `glyim.ledger-golden-verdict/1` JSON contract for the same verdict.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example verify_golden
//! ```
//!
//! Pass `write <PATH>` as the first argument to regenerate the golden
//! instead (the committed artifact lives at `ledger/verify-golden.json`;
//! regenerate only when the render change is intended, in the same PR
//! that changes it). An optional second argument overrides the project
//! root.

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));

    let golden_path = root.join("ledger/verify-golden.json");
    let outcome = if mode == "write" {
        glyim_ledger::write_verify_golden(&root, None, &golden_path)
    } else {
        glyim_ledger::verify_against_golden_path(&root, None, &golden_path)
    };
    print!("{}", glyim_ledger::format_golden_text(&outcome));
    print!("{}", glyim_ledger::format_golden_json(&outcome));

    if !outcome.is_ok() {
        std::process::exit(1);
    }
}
