//! Plan §10 consumer walkthrough: compose the final release verdict from
//! the ledger state — the same report `glyim-ledger release-check
//! <VERSION> [ROOT]` prints. The face merges three existing engine
//! outputs (the M1.12 RG-gate verdicts, the M1.13 determinism
//! self-check, and the C9 waiver health) into the one verdict a release
//! commit needs: every declared gate green, evidence reproducible, and
//! no waiver active.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example release_check
//! ```
//!
//! An optional argument overrides the project root (the directory that
//! contains `ledger/`); the default infers the repository root from
//! `CARGO_MANIFEST_DIR`, so the example works from any working
//! directory. The face is read-only — the walkthrough never writes.
//!
//! The walkthrough demonstrates both halves of the contract: a synthetic
//! green state gates as releasable (the seam consumers inject), and the
//! real registry under `ROOT/` gates honestly — with all 24 RG gates
//! still `unmeasured` today, the verdict is (correctly) red, which is
//! exactly what the face must say rather than silently passing. The
//! example exits 0 as long as the engine honours its contract in both
//! halves.

use std::path::PathBuf;

use glyim_ledger::{
    FileReleaseCheckSource, GatesOutcome, InMemoryReleaseCheckSource, JsonReleaseCheckRenderer,
    NaiveDate, ReleaseCheckRenderer, ReleaseCheckSource, ReleaseCheckState, ReleaseVersion,
    TextReleaseCheckRenderer, VerifyOutcome, WaiverRow, WaiverVerdict, WaiversOutcome,
    render_release_check, run_release_check,
};
use ledger_schema::GateEvidence;

// The row shapes the synthetic green state is built from (the same
// constructors the unit tests and the bench use).
use glyim_ledger::gates::{GateVerdict, GateVerdictRow};
use glyim_ledger::verify::DeterminismCheck;

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let version = ReleaseVersion::parse("v1.0.0").expect("walkthrough version parses");

    // Face 1: the CLI-equivalent run over the real registry — the full
    // report (verdict honest, debt rendered, never silent), printed and
    // asserted on its *shape*, not its colour.
    let outcome = run_release_check(&root, version, &FileReleaseCheckSource);
    print!("{}", TextReleaseCheckRenderer.render(&outcome));
    assert_eq!(outcome.schema, "glyim.ledger-release-check/1");
    assert_eq!(outcome.gates_total, 24, "the plan declares 24 RG gates");
    assert_eq!(
        outcome.blocking_gates.len(),
        outcome.gates_fail + outcome.gates_unmeasured,
        "the blocking list names exactly the gates that are not green"
    );
    assert_eq!(
        outcome.verify_checks, 6,
        "the M1.13 self-check runs its fixed set"
    );
    println!(
        "real registry: verdict {} ({} blocking gate(s), {} drifted check(s), \
         {} active waiver(s))",
        if outcome.is_ok() { "PASS" } else { "FAIL" },
        outcome.blocking_gates.len(),
        outcome.verify_drifted,
        outcome.waivers_active,
    );

    // Face 2: the engine directly over the seam (the `handoff`
    // walkthrough shape): a synthetic green state — all gates pass, the
    // self-check is clean, no waivers — gates as releasable, and the
    // verdict is a pure function of the state (two renders agree).
    let green = ReleaseCheckState {
        gates: GatesOutcome {
            diagnostics: Vec::new(),
            rows: (1..=24)
                .map(|i| GateVerdictRow {
                    id: format!("RG-{i:02}"),
                    title: format!("walkthrough gate {i}"),
                    criterion: "walkthrough: synthetic criterion".to_string(),
                    evidence: GateEvidence::Levels,
                    verdict: GateVerdict::Pass,
                    detail: "walkthrough: synthetic detail".to_string(),
                })
                .collect(),
            gates: 24,
            failing: 0,
            unmeasured: 0,
        },
        verify: VerifyOutcome {
            checks: (0..6)
                .map(|i| DeterminismCheck {
                    name: format!("walkthrough-check-{i}"),
                    identical: true,
                    bytes: 128,
                })
                .collect(),
            diagnostics: Vec::new(),
        },
        waivers: WaiversOutcome {
            today: NaiveDate::new_unchecked(2026, 11, 13),
            diagnostics: Vec::new(),
            waivers: 0,
            valid: 0,
            expired: 0,
            rows: Vec::<WaiverRow>::new(),
        },
    };
    let green_outcome = render_release_check(&green, version);
    let again = render_release_check(
        &InMemoryReleaseCheckSource {
            state: green.clone(),
        }
        .load(&root),
        version,
    );
    assert_eq!(green_outcome, again, "the engine is a pure function");
    assert!(
        green_outcome.is_ok(),
        "all gates green + clean self-check + no waiver: releasable"
    );
    assert!(green_outcome.blocking_gates.is_empty());
    let text = TextReleaseCheckRenderer.render(&green_outcome);
    assert!(text.contains("- PASS — v1.0.0 is releasable"));
    println!("green seam: verdict PASS (24 gates, 0 blocking, 0 active waivers)");

    // Face 3: one waiver flips the verdict — the plan §10 "no waiver is
    // active" clause, exercised through the same seam.
    let mut waived = green;
    waived.waivers.rows = vec![WaiverRow {
        id: "WVR-20261113-walkthrough".to_string(),
        feature: "LANG-RELEASE-DEMO".to_string(),
        reason: "regressions covered while the fix lands".to_string(),
        issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
        expires: "2026-11-20".to_string(),
        verdict: WaiverVerdict::Valid,
        remaining_days: 7,
    }];
    waived.waivers.waivers = 1;
    waived.waivers.valid = 1;
    let blocked = render_release_check(&waived, version);
    assert!(!blocked.is_ok(), "one active waiver blocks the release");
    assert_eq!(blocked.waivers_active, 1);
    assert!(TextReleaseCheckRenderer.render(&blocked).contains("- FAIL"));

    // Face 4: the JSON contract — one serialized document per run,
    // byte-identical across two renders of the same state (the C1/M1.13
    // determinism contract the golden tests pin).
    let json_a = JsonReleaseCheckRenderer.render(&outcome);
    let json_b = JsonReleaseCheckRenderer.render(&outcome);
    assert_eq!(json_a, json_b, "both renders are byte-identical");
    println!(
        "json face: {} bytes, schema {}",
        json_a.len(),
        outcome.schema,
    );
}
