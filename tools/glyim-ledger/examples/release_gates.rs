//! M1.12 consumer walkthrough: print the release-gate report — the same
//! table `glyim-ledger gates [ROOT]` prints, followed by the
//! `glyim.ledger-gates/1` JSON contract for the same run.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example release_gates
//! ```
//!
//! The report evaluates every `RG-NN` gate declared in `ledger/gates.toml`
//! against the evidence sources the ledger has today: RG-09 reads the M1.8
//! compute outcome, RG-10 reads the M1.7 gap registry, and every gate
//! declared with `evidence = "none"` renders `unmeasured` — displayed, not
//! silently passed and not failed by itself (plan M1.12). An optional
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

    let outcome = glyim_ledger::run_gates(&root, evidence.as_deref());
    print!("{}", glyim_ledger::format_gates_text(&outcome));
    print!("{}", glyim_ledger::format_gates_json(&outcome));

    // Load/validation diagnostics fail; gate verdicts are report content —
    // a `fail` row is information for the release checklist, not a command
    // exit code (see the `gates` module docs).
    if !outcome.is_ok() {
        std::process::exit(1);
    }
}
