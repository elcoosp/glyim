//! M1.13 mutation-slice consumer walkthrough: run the policy-mutation
//! gate — the same report `glyim-ledger policy-mutation [ROOT]` prints —
//! followed by the `glyim.ledger-mutation/1` JSON contract for the same
//! run.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example policy_mutation
//! ```
//!
//! The harness evaluates a fixed catalogue of nine policy mutants (rules
//! dropped, counts inflated, outcomes flipped) through the level engine
//! over the same registry and evidence the production compute pass sees;
//! a surviving mutant names a policy surface the evaluation ignores and
//! the command exits non-zero. An optional first argument overrides the
//! project root; a second is an explicit evidence JSONL path (mirroring
//! `--evidence`).

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let evidence = args.next().map(PathBuf::from);

    let outcome = glyim_ledger::run_policy_mutation(&root, evidence.as_deref());
    print!("{}", glyim_ledger::format_mutation_text(&outcome));
    print!("{}", glyim_ledger::format_mutation_json(&outcome));

    if !outcome.is_ok() {
        std::process::exit(1);
    }
}
