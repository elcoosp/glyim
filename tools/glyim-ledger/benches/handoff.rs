//! Handoff bench (plan §3.7 face).
//!
//! Dependency-free (`harness = false`), mirroring the `waivers` bench:
//! measures the [`glyim_ledger::render_handoff`] composition engine over
//! a synthetic 2000-item queue, both [`glyim_ledger::HandoffRenderer`]
//! faces — the marker-delimited Markdown document and the
//! `glyim.ledger-handoff/1` JSON contract — and the full file-composition
//! path (`run_handoff_out` over a seeded 200-feature tempdir registry:
//! compute + next + waivers loads, render, write, and the `--check`
//! byte-compare), so CI logs can track the handoff face cost as the
//! registry grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench handoff
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use glyim_ledger::{
    ComputeOutcome, FeatureLevel, FileHandoffSource, HandoffFormat, HandoffRenderer, HandoffState,
    InMemoryHandoffSource, JsonHandoffRenderer, NaiveDate, NextOutcome, TextHandoffRenderer,
    WaiverRow, WaiverVerdict, WaiversOutcome, WorkItem, WorkKind, render_handoff, run_handoff,
    run_handoff_out,
};
use ledger_schema::{GapClass, Lane, Level, Tier};

/// The pinned bench date (no clock in the bench).
fn day() -> NaiveDate {
    NaiveDate::new_unchecked(2026, 11, 13)
}

/// Builds the synthetic state: `features` compute entries spread across
/// L0–L2, a `queue`-length rank-sorted work queue (every 5th item an
/// unwitnessed class-b gap), and `waivers` rows (every 4th expired).
fn synthetic_state(features: usize, queue: usize, waivers: usize) -> HandoffState {
    let levels = [Level::L0, Level::L1, Level::L2];
    let entries: Vec<FeatureLevel> = (0..features)
        .map(|i| FeatureLevel {
            id: format!("LANG-BENCH-{i}"),
            tier: Tier::Core,
            target_level: Level::L5,
            computed_level: levels[i % 3],
            capped_by: Vec::new(),
            unmeasured: Vec::new(),
            missing: Vec::new(),
        })
        .collect();
    let items: Vec<WorkItem> = (0..queue)
        .map(|i| WorkItem {
            rank: i + 1,
            id: if i % 5 == 0 {
                format!("GAP-{:04}", 10 + i / 5)
            } else {
                format!("LANG-BENCH-Q{i}")
            },
            title: format!("bench item {i}: synthetic queue row"),
            kind: if i % 5 == 0 {
                WorkKind::Gap {
                    class: GapClass::B,
                    witnessed: false,
                }
            } else {
                WorkKind::Feature
            },
            why: "bench queue: synthetic reason line".to_string(),
            evidence_needed: "bench queue: synthetic evidence line".to_string(),
            suggested_files: vec![format!("./ledger/features/bench/{i}.toml")],
            lane: Some(Lane::M),
            related: Vec::new(),
            depth: if i % 5 == 0 { 0 } else { 2 },
        })
        .collect();
    let rows: Vec<WaiverRow> = (0..waivers)
        .map(|i| {
            let remaining = if i % 4 == 0 { -1 } else { (i as i64 % 7) + 1 };
            WaiverRow {
                id: format!("WVR-BENCH-{i:04}"),
                feature: format!("LANG-BENCH-{i}"),
                reason: "bench waiver: synthetic exception".to_string(),
                issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
                expires: day().plus_days(remaining).to_iso(),
                verdict: if remaining < 0 {
                    WaiverVerdict::Expired
                } else {
                    WaiverVerdict::Valid
                },
                remaining_days: remaining,
            }
        })
        .collect();
    let expired = rows
        .iter()
        .filter(|row| row.verdict == WaiverVerdict::Expired)
        .count();
    HandoffState {
        compute: ComputeOutcome {
            entries,
            diagnostics: Vec::new(),
            features,
            records: features * 2,
        },
        next: NextOutcome {
            diagnostics: Vec::new(),
            total: queue,
            queue: items,
        },
        waivers: WaiversOutcome {
            today: day(),
            diagnostics: Vec::new(),
            waivers,
            valid: waivers - expired,
            expired,
            rows,
        },
    }
}

fn main() {
    // Part 1 — the pure engine over the synthetic stress state.
    let state = synthetic_state(300, 2000, 20);
    let expired = state.waivers.expired;
    assert_eq!(state.compute.features, 300);
    assert_eq!(state.next.queue.len(), 2000);

    let started = Instant::now();
    let outcome = black_box(render_handoff(black_box(&state), black_box(5)));
    let engine_elapsed = started.elapsed();
    assert_eq!(outcome.features, 300);
    assert_eq!(outcome.queue_total, 2000);
    assert_eq!(outcome.items.len(), 5);
    assert_eq!(outcome.witness_debt.len(), 400);
    assert_eq!(outcome.waivers_expired, expired);
    assert!(
        !outcome.is_ok(),
        "the bench state carries expired waivers (C9)"
    );

    let started = Instant::now();
    let text = black_box(TextHandoffRenderer.render(black_box(&outcome)));
    let text_elapsed = started.elapsed();
    assert!(text.contains(HANDOFF_MD_START_CHECK));
    assert!(text.contains("## Witness debt"));

    let started = Instant::now();
    let json = black_box(JsonHandoffRenderer.render(black_box(&outcome)));
    let json_elapsed = started.elapsed();
    assert!(json.contains("\"glyim.ledger-handoff/1\""));

    // The source seam adds one clone per load; measure it so the run
    // pipeline (clone + compose + span) stays visible in CI logs.
    let source = InMemoryHandoffSource { state };
    let started = Instant::now();
    let outcome = black_box(run_handoff(
        black_box(std::path::Path::new(".")),
        black_box(&source),
        black_box(5),
    ));
    let run_elapsed = started.elapsed();
    assert_eq!(outcome.queue_total, 2000);

    // Part 2 — the file-composition path: a seeded 200-feature registry
    // (compute + next + waivers loads, render, write, and --check).
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
    let mut registry = String::new();
    for i in 0..200 {
        let _ = write!(
            registry,
            "[[feature]]\n\
             id = \"LANG-BENCH-FILE-{i}\"\n\
             title = \"handoff bench feature {i}\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\"]\n\
             owner_lane = \"M\"\n\n"
        );
    }
    std::fs::write(root.join("ledger/features/handoff.toml"), &registry).expect("write features");

    let started = Instant::now();
    let written = black_box(run_handoff_out(
        black_box(root),
        black_box(&FileHandoffSource),
        black_box(5),
        black_box(HandoffFormat::Markdown),
        black_box(Some(root.join("HANDOFF.md").as_path())),
        black_box(false),
    ))
    .expect("the bench write face must pass");
    let compose_elapsed = started.elapsed();
    assert_eq!(written.outcome.features, 200);
    assert_eq!(written.outcome.queue_total, 200);

    let started = Instant::now();
    let checked = black_box(run_handoff_out(
        black_box(root),
        black_box(&FileHandoffSource),
        black_box(5),
        black_box(HandoffFormat::Markdown),
        black_box(None),
        black_box(true),
    ))
    .expect("the bench check face must pass");
    let check_elapsed = started.elapsed();
    assert!(checked.checked);

    println!(
        "handoff bench: 300 features / 2000 queue items / 20 waivers (synthetic) + 200-feature tempdir registry\n  \
         engine     : {engine_elapsed:.1?}\n  \
         text face  : {text_elapsed:.1?} ({} bytes)\n  \
         json face  : {json_elapsed:.1?} ({} bytes)\n  \
         run (clone+compose+span): {run_elapsed:.1?}\n  \
         compose (200-feature fs loads + render + write): {compose_elapsed:.1?}\n  \
         check (fs loads + render + byte-compare): {check_elapsed:.1?}",
        text.len(),
        json.len(),
    );
}

/// The generated-content marker the text face must open with (asserted
/// through a const to keep the bench honest about the contract bytes).
const HANDOFF_MD_START_CHECK: &str =
    "<!-- ledger:handoff:start (generated by `glyim-ledger handoff`; DO NOT EDIT) -->";
