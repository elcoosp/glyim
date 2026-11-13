//! Plan §3.7 consumer walkthrough: compose the session handoff from the
//! ledger state — the same document `glyim-ledger handoff [ROOT]`
//! prints. The face merges three existing engine outputs (the M1.8
//! compute levels, the §3.9 work queue, and the C9 waiver health) into
//! one short read at the start of a session.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example handoff_report
//! ```
//!
//! An optional argument overrides the project root (the directory that
//! contains `ledger/`); the default infers the repository root from
//! `CARGO_MANIFEST_DIR`, so the example works from any working directory.
//! The face is read-only — the walkthrough never writes; the write and
//! drift-gate path (`--out` / `--check`) is exercised by the
//! `handoff_out_writes_then_check_passes_and_drift_fails` test and the
//! CLI directly:
//!
//! ```text
//! cargo run -p glyim-ledger -- handoff . --out HANDOFF.md
//! cargo run -p glyim-ledger -- handoff . --check
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileHandoffSource, HANDOFF_DEFAULT_ITEMS, HANDOFF_MAX_ITEMS, HandoffRenderer, HandoffSource,
    JsonHandoffRenderer, TextHandoffRenderer, render_handoff, run_handoff,
};

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));

    // Face 1: the CLI-equivalent run — composed state, 5-item slice, the
    // gate verdict the exit code carries.
    let outcome = run_handoff(&root, &FileHandoffSource, HANDOFF_DEFAULT_ITEMS);
    print!("{}", TextHandoffRenderer.render(&outcome));
    assert_eq!(outcome.schema, "glyim.ledger-handoff/1");
    assert!(
        outcome.queue_total >= outcome.items.len(),
        "the slice never exceeds the queue"
    );
    println!(
        "gate: {} ({} error diagnostic(s), {} expired waiver(s))",
        if outcome.is_ok() { "OK" } else { "FAILED" },
        outcome.error_diagnostics,
        outcome.waivers_expired,
    );

    // Face 2: the engine directly over the same source (the seam the
    // bench and the JSON face drive) — item slicing is a pure function
    // of the state, so a 1-item and a max-item render agree on rank
    // order.
    let one = render_handoff(&FileHandoffSource.load(&root), 1);
    let maxed = render_handoff(&FileHandoffSource.load(&root), HANDOFF_MAX_ITEMS);
    assert_eq!(
        one.items.first().and(maxed.items.first()),
        one.items.first(),
        "the top-ranked item is slice-size independent"
    );

    // Face 3: the JSON contract — one serialized document per run,
    // byte-identical across two renders of the same state (the C1/M1.13
    // determinism contract the golden tests pin).
    let json_a = JsonHandoffRenderer.render(&outcome);
    let json_b = JsonHandoffRenderer.render(&outcome);
    assert_eq!(json_a, json_b, "both renders are byte-identical");
    println!(
        "json face: {} bytes, schema {}",
        json_a.len(),
        outcome.schema,
    );

    // The handoff gate mirrors the faces it composes: an expired waiver
    // or an error diagnostic fails the exit code (the debt is rendered,
    // never silent).
    if !outcome.is_ok() {
        std::process::exit(1);
    }
}
