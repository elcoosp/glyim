//! Release-check bench (plan §10 face).
//!
//! Dependency-free (`harness = false`), mirroring the `handoff` bench:
//! measures the [`glyim_ledger::render_release_check`] gating engine over
//! a synthetic composed state (the plan's 24 declared `RG` gates in
//! mixed verdicts, the M1.13 six-check self-check, and 50 waiver rows),
//! both [`glyim_ledger::ReleaseCheckRenderer`] faces — the text report
//! and the `glyim.ledger-release-check/1` JSON contract — and the full
//! file-composition path (`run_release_check` over a seeded tempdir
//! registry: gates + verify + waiver loads and the honest
//! unmeasured-blocks verdict), so CI logs can track the release face cost
//! as the registry grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench release
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use glyim_ledger::{
    FileReleaseCheckSource, GatesOutcome, InMemoryReleaseCheckSource, JsonReleaseCheckRenderer,
    NaiveDate, ReleaseCheckRenderer, ReleaseCheckState, ReleaseVersion, TextReleaseCheckRenderer,
    VerifyOutcome, WaiverRow, WaiverVerdict, WaiversOutcome, render_release_check,
    run_release_check,
};
use ledger_schema::GateEvidence;

// The M1.12 row shapes the engine counts; the verdicts are mixed so the
// bench exercises all three arms of the blocking-gate classification.
use glyim_ledger::gates::{GateVerdict, GateVerdictRow};
use glyim_ledger::verify::DeterminismCheck;

/// The pinned bench date (no clock in the bench).
fn day() -> NaiveDate {
    NaiveDate::new_unchecked(2026, 11, 13)
}

/// The release version the bench gates.
fn version() -> ReleaseVersion {
    ReleaseVersion::parse("v1.0.0").expect("bench version parses")
}

/// Builds the synthetic composed state: `gates` declared RG rows (every
/// 6th failing, every 5th of the rest unmeasured, the remainder pass),
/// `checks` determinism rows (every 12th drifted), and `waivers` rows
/// (every 4th expired — so an honest verdict is FAIL, not a green one).
fn synthetic_state(gates: usize, checks: usize, waivers: usize) -> ReleaseCheckState {
    let rows: Vec<GateVerdictRow> = (0..gates)
        .map(|i| {
            let verdict = if i % 6 == 5 {
                GateVerdict::Fail
            } else if i % 6 == 4 {
                GateVerdict::Unmeasured
            } else {
                GateVerdict::Pass
            };
            GateVerdictRow {
                id: format!("RG-{:02}", i + 1),
                title: format!("bench gate {i}"),
                criterion: "bench gate: synthetic criterion line".to_string(),
                evidence: if verdict == GateVerdict::Unmeasured {
                    GateEvidence::None
                } else {
                    GateEvidence::Levels
                },
                verdict,
                detail: "bench gate: synthetic detail line".to_string(),
            }
        })
        .collect();
    let failing = rows
        .iter()
        .filter(|row| row.verdict == GateVerdict::Fail)
        .count();
    let unmeasured = rows
        .iter()
        .filter(|row| row.verdict == GateVerdict::Unmeasured)
        .count();
    let checks: Vec<DeterminismCheck> = (0..checks)
        .map(|i| DeterminismCheck {
            name: format!("bench-check-{i}"),
            identical: i % 12 != 11,
            bytes: 128,
        })
        .collect();
    let rows_w: Vec<WaiverRow> = (0..waivers)
        .map(|i| {
            let expired = i % 4 == 3;
            WaiverRow {
                id: format!("WVR-BENCH-{i:04}"),
                feature: format!("LANG-BENCH-{i}"),
                reason: "bench waiver: synthetic exception".to_string(),
                issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
                expires: day().plus_days(if expired { -1 } else { 7 }).to_iso(),
                verdict: if expired {
                    WaiverVerdict::Expired
                } else {
                    WaiverVerdict::Valid
                },
                remaining_days: if expired { -1 } else { 7 },
            }
        })
        .collect();
    let expired = rows_w
        .iter()
        .filter(|row| row.verdict == WaiverVerdict::Expired)
        .count();
    ReleaseCheckState {
        gates: GatesOutcome {
            diagnostics: Vec::new(),
            rows,
            gates,
            failing,
            unmeasured,
        },
        verify: VerifyOutcome {
            checks,
            diagnostics: Vec::new(),
        },
        waivers: WaiversOutcome {
            today: day(),
            diagnostics: Vec::new(),
            waivers,
            valid: waivers - expired,
            expired,
            rows: rows_w,
        },
    }
}

fn main() {
    // Part 1 — the pure engine over the synthetic stress state.
    let state = synthetic_state(24, 6, 50);
    assert_eq!(state.gates.rows.len(), 24, "the plan declares 24 RG gates");
    assert_eq!(state.verify.checks.len(), 6, "the M1.13 fixed check set");
    let active = state.waivers.valid;
    let expired = state.waivers.expired;
    assert!(
        active > 0 && expired > 0,
        "the bench state carries both waiver verdicts"
    );

    let started = Instant::now();
    let outcome = black_box(render_release_check(
        black_box(&state),
        black_box(version()),
    ));
    let engine_elapsed = started.elapsed();
    assert_eq!(outcome.gates_total, 24);
    // Every 6th gate fails (4) and every 6th of the rest is unmeasured
    // (4); the remainder (16) pass.
    assert_eq!(outcome.gates_pass, 16);
    assert!(!outcome.blocking_gates.is_empty());
    assert_eq!(outcome.waivers_active, active);
    assert_eq!(outcome.waivers_expired, expired);
    assert!(
        !outcome.is_ok(),
        "the bench state is a red verdict (unmeasured gates + active waivers)"
    );

    let started = Instant::now();
    let text = black_box(TextReleaseCheckRenderer.render(black_box(&outcome)));
    let text_elapsed = started.elapsed();
    assert!(text.starts_with("# Release check — v1.0.0"));
    assert!(text.contains("## Verdict"));
    assert!(text.contains("- FAIL"));

    let started = Instant::now();
    let json = black_box(JsonReleaseCheckRenderer.render(black_box(&outcome)));
    let json_elapsed = started.elapsed();
    assert!(json.contains("\"glyim.ledger-release-check/1\""));
    assert!(json.contains("\"ok\": false"));

    // The source seam adds one clone per load; measure it so the run
    // pipeline (clone + gate + span) stays visible in CI logs.
    let source = InMemoryReleaseCheckSource { state };
    let started = Instant::now();
    let outcome = black_box(run_release_check(
        black_box(std::path::Path::new(".")),
        black_box(version()),
        black_box(&source),
    ));
    let run_elapsed = started.elapsed();
    assert_eq!(outcome.gates_total, 24);

    // Part 2 — the file-composition path: a seeded registry (gates +
    // verify + waiver loads; no waivers.toml, so the waiver axis is
    // clean) whose gates declare `evidence = "none"` — the honest
    // unmeasured-blocks verdict over a real load pass.
    let dir = tempfile::tempdir().expect("bench tmp");
    let root = dir.path();
    std::fs::create_dir_all(root.join("ledger/features")).expect("mkdir features");
    std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
    std::fs::write(
        root.join("docs/specs/v0.1.0.md"),
        "# spec fixture\n\n## evidence\n",
    )
    .expect("write spec");
    std::fs::write(
        root.join("ledger/policy.toml"),
        "schema = \"glyim.ledger-policy/1\"\n\n\
         [levels.L1]\nrules = [{ mode = \"compile-pass\", outcome = \"passed\", count = 1 }]\n\n\
         [gap_caps]\nenabled = true\n\n\
         [hardening]\nfuzz_cpu_hours = 24\nmutation_kill_rate_pct = 75\n",
    )
    .expect("write policy");
    let mut features = String::new();
    for i in 0..40 {
        let _ = write!(
            &mut features,
            "[[feature]]\n\
             id = \"LANG-BENCH-FILE-{i}\"\n\
             title = \"release bench feature {i}\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\"]\n\
             owner_lane = \"M\"\n\n"
        );
    }
    std::fs::write(root.join("ledger/features/release.toml"), &features).expect("write features");
    let mut gates = String::from("schema = \"glyim.ledger-gates-registry/1\"\n\n");
    for i in 1..=24 {
        let _ = write!(
            &mut gates,
            "[[gate]]\n\
             id = \"RG-{i:02}\"\n\
             title = \"bench gate {i}\"\n\
             criterion = \"bench gate: synthetic criterion\"\n\
             evidence = \"none\"\n\n"
        );
    }
    std::fs::write(root.join("ledger/gates.toml"), &gates).expect("write gates");

    let started = Instant::now();
    let outcome = black_box(run_release_check(
        black_box(root),
        black_box(version()),
        black_box(&FileReleaseCheckSource),
    ));
    let file_elapsed = started.elapsed();
    assert_eq!(outcome.gates_total, 24, "all declared gates loaded");
    assert_eq!(
        outcome.gates_unmeasured, 24,
        "no evidence source: unmeasured"
    );
    assert_eq!(outcome.blocking_gates.len(), 24);
    assert_eq!(outcome.verify_checks, 6, "the self-check ran its fixed set");
    assert_eq!(outcome.verify_drifted, 0, "the pipeline is deterministic");
    assert_eq!(
        outcome.waivers_active, 0,
        "no waivers.toml: no active waiver"
    );
    assert_eq!(outcome.error_diagnostics, 0, "the registry loads cleanly");
    assert!(!outcome.is_ok(), "unmeasured gates block the release");

    println!(
        "release bench: 24 gates / 6 self-checks / 50 waivers (synthetic) + 40-feature, 24-gate tempdir registry\n  \
         engine     : {engine_elapsed:.1?}\n  \
         text face  : {text_elapsed:.1?} ({} bytes)\n  \
         json face  : {json_elapsed:.1?} ({} bytes)\n  \
         in-memory  : {run_elapsed:.1?}\n  \
         file path  : {file_elapsed:.1?}",
        text.len(),
        json.len(),
    );
}
