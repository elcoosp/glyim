//! M1.10 (first slice) consumer walkthrough: the readiness-regression gate
//! — the same pipeline `glyim-ledger compute --snapshot` plus
//! `glyim-ledger diff <BASE> <CURRENT>` runs, ending in the
//! `glyim.ledger-diff/1` JSON contract for the same pair.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example readiness_diff
//! ```
//!
//! The example computes the real repository's readiness state twice: once
//! as the baseline snapshot, once as a "head" snapshot where one feature's
//! computed level was dropped by hand (simulating a readiness regression).
//! It then runs the diff twice — unwaived (gate fails) and covered by a
//! fresh 7-day waiver, exactly the entry a `ledger/waivers.toml` block
//! would carry (gate passes) — so both faces of the C9 gate are visible.
//! An optional first argument overrides the project root (mirroring the
//! subcommands' ROOT).

use std::path::PathBuf;

use glyim_ledger::{
    ComputeSnapshot, FileSnapshotStore, NaiveDate, SnapshotStore, WaiverEntry, WaiverSet,
    execute_diff, format_diff_json, format_diff_text, run_compute, today_iso,
};
use ledger_schema::Level;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));

    // 1. Baseline: the repository's real computed readiness state.
    let outcome = run_compute(&root, None);
    if !outcome.is_ok() {
        eprintln!("baseline compute failed (diagnostics below):");
        for diagnostic in &outcome.diagnostics {
            eprintln!("  {diagnostic}");
        }
        std::process::exit(1);
    }
    let baseline = ComputeSnapshot::from_outcome(&outcome);
    println!(
        "baseline snapshot: {} features, all computed from the real registry",
        baseline.entries.len()
    );

    // 2. Head: the same snapshot with one feature dropped one level — the
    //    shape of a readiness regression a CI run must catch.
    let mut head = baseline.clone();
    let regressed = head
        .entries
        .first_mut()
        .expect("the registry is never empty");
    let regressed_id = regressed.id.clone();
    regressed.computed_level = Level::L0;
    println!("head snapshot: {regressed_id} dropped from its computed level to L0");

    let scratch = tempfile::tempdir().expect("scratch dir");
    let baseline_path = scratch.path().join("baseline.json");
    let head_path = scratch.path().join("head.json");
    let store = FileSnapshotStore;
    store
        .save(&baseline_path, &baseline)
        .expect("save baseline");
    store.save(&head_path, &head).expect("save head");

    // 3. Unwaived diff: the gate fails and names the regression.
    let today = NaiveDate::parse_iso(&today_iso()).expect("the clock renders ISO dates");
    let unwaived = execute_diff(
        &store,
        &WaiverSet::empty(),
        &baseline_path,
        &head_path,
        today,
    );
    print!("{}", format_diff_text(&unwaived));

    // 4. Waived diff: a fresh waiver (7-day expiry, inside the C9 horizon)
    //    covers the same regression, and the gate passes.
    let waiver_set = WaiverSet {
        waivers: vec![WaiverEntry {
            id: "W-EXAMPLE".to_string(),
            feature: regressed_id.clone(),
            reason: "example walkthrough waiver".to_string(),
            issue: "https://example.com/issue/0".to_string(),
            expires: today.plus_days(7).to_iso(),
        }],
        diagnostics: Vec::new(),
    };
    let waived = execute_diff(&store, &waiver_set, &baseline_path, &head_path, today);
    print!("{}", format_diff_text(&waived));
    print!("{}", format_diff_json(&waived));

    if !unwaived.is_ok() && waived.is_ok() {
        println!("gate: unwaived regression FAILED, waived regression PASSED");
    } else {
        std::process::exit(1);
    }
}
