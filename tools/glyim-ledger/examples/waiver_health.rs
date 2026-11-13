//! C9 consumer walkthrough: assess the waiver inventory — the exact
//! outcome `glyim-ledger waivers [ROOT]` reports — through the
//! [`WaiverSource`] seam, then render both faces and evaluate the C9
//! gate (`expired waivers fail CI`).
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example waiver_health
//! ```
//!
//! The example first runs the file-backed source against the real
//! repository (an absent `ledger/waivers.toml` is the empty baseline, not
//! an error), then drives the same engine with a synthetic in-memory set
//! that carries one expired entry so the gate's failure path is visible.
//! An optional first argument overrides the project root.
//!
//! Try the CLI face on the same data:
//!
//! ```text
//! cargo run -p glyim-ledger -- waivers --format json
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileWaiverSource, InMemoryWaiverSource, JsonWaiversRenderer, NaiveDate, TextWaiversRenderer,
    WaiverEntry, WaiverSet, WaiversRenderer, assess_waivers, run_waivers,
};

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));

    // Face 1: the file-backed source over the real repository. The same
    // `load_waivers` loader and diagnostics the `diff` gate uses, so the
    // health report and the regression gate cannot disagree.
    let repo = run_waivers(&root, &FileWaiverSource);
    println!(
        "repo ledger/waivers.toml: {} waiver(s) loaded, {} expired, gate passed: {}",
        repo.waivers,
        repo.expired,
        repo.is_ok(),
    );

    // Face 2: the in-memory source (the test seam) with one expired and
    // one live entry — the assessment is pure date arithmetic pinned to a
    // fixed `today`, so the walkthrough is reproducible byte-for-byte.
    let today = NaiveDate::new_unchecked(2026, 11, 13);
    let set = WaiverSet {
        waivers: vec![
            waiver(
                "WVR-2026-11-13-UNWIND",
                "LANG-UNWIND-CLEANUP",
                "SEH cleanup edges unreachable until GAP-0002 lands",
                "2026-11-19",
            ),
            waiver(
                "WVR-2026-11-01-DROPFLAGS",
                "COMPILER-BCK-DROP-FLAGS",
                "partial-move drop suppression while the fix lands",
                "2026-11-10",
            ),
        ],
        diagnostics: Vec::new(),
    };
    let outcome = assess_waivers(&set, today);
    println!();
    print!("{}", TextWaiversRenderer.render(&outcome));
    println!();
    println!("-- glyim.ledger-waivers/1 (first 6 lines) --");
    let json = JsonWaiversRenderer.render(&outcome);
    for line in json.lines().take(6) {
        println!("{line}");
    }

    // The gate: one expired row fails C9 even though nothing regressed.
    assert!(
        !outcome.is_ok(),
        "the expired WVR-2026-11-01-DROPFLAGS entry must fail the gate"
    );
    let _ = InMemoryWaiverSource { set };
}

/// One synthetic `[[waiver]]` entry (the `waivers.toml` document shape).
fn waiver(id: &str, feature: &str, reason: &str, expires: &str) -> WaiverEntry {
    WaiverEntry {
        id: id.to_string(),
        feature: feature.to_string(),
        reason: reason.to_string(),
        issue: format!("https://github.com/elcoosp/glyim/issues/42 ({id})"),
        expires: expires.to_string(),
    }
}
