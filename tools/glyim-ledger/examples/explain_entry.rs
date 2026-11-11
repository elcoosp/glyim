//! Plan §3.7 consumer walkthrough: resolve one registry ID and render
//! its explanation — the same report `glyim-ledger explain <ID> [ROOT]`
//! prints. The why/evidence lines come from the same queue engine as
//! `ledger next`, so the two surfaces agree word for word.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example explain_entry -- GAP-0002
//! ```
//!
//! The first argument is the registry ID to explain; an optional second
//! argument overrides the project root (the directory that contains
//! `ledger/`), defaulting to the repository root inferred from
//! `CARGO_MANIFEST_DIR`.

use std::path::PathBuf;

fn main() {
    let id = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: cargo run -p glyim-ledger --example explain_entry -- <ID> [ROOT]");
        std::process::exit(2);
    });
    let root = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));

    let outcome = glyim_ledger::run_explain(&root, &id);
    print!("{}", glyim_ledger::format_explain_report(&outcome));

    // An unknown ID prints the "no registry entry" report and exits 1,
    // so scripted consumers never mistake a typo for an explanation.
    if !outcome.is_ok() || outcome.entry.is_none() {
        std::process::exit(1);
    }
}
