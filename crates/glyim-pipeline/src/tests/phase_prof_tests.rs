//! Phase-profile contract tests (M2.1 slice).
//!
//! Written test-first: the profile is the machine-checked backbone of
//! `--time-passes`, `--self-profile` (Chrome trace), and `--emit=stats`.
//! The contract pinned here:
//!
//! * the phase set and its order are a stable, documented list —
//!   renderers and downstream parsers rely on it;
//! * all three renderers (text table, Chrome trace, JSON stats) are
//!   byte-deterministic for a given profile: no clocks, no iteration
//!   order from hash maps, no locale-dependent formatting;
//! * repeated recording of a phase accumulates (lower/borrowck/opt run
//!   once per body) instead of overwriting;
//! * an empty profile renders a valid, zeroed report — never NaN, never
//!   an empty trace-event list that breaks tooling.

use crate::phase_prof::{Phase, PhaseProfile, PhaseRecorder, StatsReport};

/// The phase list is part of the output schema: `--emit=stats` consumers
/// and the Chrome-trace loader both index into it. Reordering is a
/// breaking change and must be a deliberate one.
#[test]
fn phase_order_is_stable() {
    let names: Vec<&str> = Phase::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "load", "parse", "expand", "defmap", "hir", "typeck", "lower", "borrowck", "opt",
            "mono", "codegen",
        ]
    );
    // Display name is the wire format for every renderer.
    assert_eq!(Phase::Codegen.as_str(), "codegen");
}

/// Recording the same phase repeatedly (the lowering loop calls
/// lower/borrowck/opt once per body) must accumulate durations and count
/// hits, not overwrite the previous entry.
#[test]
fn recorder_accumulates_repeated_phases() {
    let recorder = PhaseRecorder::new();
    for _ in 0..3 {
        {
            let _guard = recorder.enter(Phase::Lower);
        }
    }
    for _ in 0..2 {
        {
            let _guard = recorder.enter(Phase::Borrowck);
        }
    }
    let profile = recorder.profile();
    assert_eq!(profile.hits(Phase::Lower), 3);
    assert_eq!(profile.hits(Phase::Borrowck), 2);
    assert_eq!(profile.hits(Phase::Parse), 0);
    // Total is the sum over every phase slot.
    let sum: u64 = Phase::ALL.iter().map(|p| profile.micros(*p)).sum();
    assert_eq!(profile.total_micros(), sum);
}

/// The text table (`--time-passes`) is byte-deterministic and lists every
/// phase in `Phase::ALL` order with a total row.
#[test]
fn text_report_is_deterministic_and_complete() {
    let profile = synthetic_profile();
    let a = profile.render_text();
    let b = profile.render_text();
    assert_eq!(a, b, "render_text must be deterministic");

    let mut cursor = 0usize;
    for phase in Phase::ALL {
        let line = a
            .lines()
            .find(|l| l.contains(phase.as_str()))
            .unwrap_or_else(|| panic!("text report missing row for {}", phase.as_str()));
        let line_idx = a.find(line).expect("line position");
        assert!(
            line_idx >= cursor,
            "rows must appear in Phase::ALL order ({} out of place)",
            phase.as_str()
        );
        cursor = line_idx;
    }
    // Header + one row per phase + total row.
    assert!(
        a.contains("Total"),
        "text report must have a total row:\n{a}"
    );
    assert!(
        a.contains("Micros"),
        "text report must have a micros column:\n{a}"
    );
}

/// The Chrome trace (`--self-profile`) is valid JSON, carries one
/// complete (`ph: "X"`) event per phase in pipeline order with
/// monotonically increasing timestamps, and durations match the profile.
#[test]
fn chrome_trace_is_valid_json_and_ordered() {
    let profile = synthetic_profile();
    let a = profile.render_chrome_trace();
    let b = profile.render_chrome_trace();
    assert_eq!(a, b, "render_chrome_trace must be deterministic");

    let value: serde_json::Value =
        serde_json::from_str(&a).expect("chrome trace must be valid JSON");
    let events = value["traceEvents"]
        .as_array()
        .expect("traceEvents array")
        .clone();

    // One complete event per phase, in Phase::ALL order.
    let complete: Vec<&serde_json::Value> = events.iter().filter(|e| e["ph"] == "X").collect();
    assert_eq!(complete.len(), Phase::ALL.len(), "one X event per phase");
    let mut prev_ts = 0u64;
    for (event, phase) in complete.iter().zip(Phase::ALL.iter()) {
        assert_eq!(event["name"], phase.as_str(), "event order matches phases");
        assert_eq!(event["cat"], "phase");
        let ts = event["ts"].as_u64().expect("ts is a number");
        assert!(ts >= prev_ts, "ts must be monotonically increasing");
        prev_ts = ts + event["dur"].as_u64().expect("dur is a number");
        assert_eq!(
            event["dur"].as_u64(),
            Some(profile.micros(*phase)),
            "dur mirrors the recorded micros"
        );
    }
    // Synthetic total = 990µs; the last event must end exactly there.
    assert_eq!(prev_ts, profile.total_micros());
}

/// `--emit=stats` JSON: schema version 1, every phase present with its
/// micros, plus the mono/warning counters the CLI adds. Deterministic.
#[test]
fn json_stats_schema_v1_is_stable() {
    let report = StatsReport {
        profile: synthetic_profile(),
        mono_bodies: 3,
        warnings: 2,
    };
    let a = report.render_json();
    let b = report.render_json();
    assert_eq!(a, b, "render_json must be deterministic");

    let value: serde_json::Value = serde_json::from_str(&a).expect("stats JSON must be valid JSON");
    assert_eq!(value["version"], 1, "schema version pinned at 1");
    assert_eq!(value["mono_bodies"], 3);
    assert_eq!(value["warnings"], 2);
    let phases = value["phases"].as_array().expect("phases array");
    assert_eq!(phases.len(), Phase::ALL.len());
    for (entry, phase) in phases.iter().zip(Phase::ALL.iter()) {
        assert_eq!(entry["phase"], phase.as_str());
        assert_eq!(
            entry["micros"].as_u64(),
            Some(synthetic_micros(*phase)),
            "phase micros round-trip"
        );
    }
    assert_eq!(
        value["total_micros"].as_u64(),
        Some(synthetic_total()),
        "total_micros echoes the profile"
    );
}

/// An empty profile renders a valid, fully-zeroed report: no NaN, no
/// panic, total row present, and the trace still parses.
#[test]
fn empty_profile_renders_zeroed() {
    let profile = PhaseProfile::EMPTY.clone();
    assert_eq!(profile.total_micros(), 0);
    let text = profile.render_text();
    assert!(
        text.contains("Total"),
        "empty report keeps the total row:\n{text}"
    );
    assert!(!text.contains("NaN"), "no NaN in an empty report");
    let trace: serde_json::Value =
        serde_json::from_str(&profile.render_chrome_trace()).expect("empty trace parses");
    // One X event per phase + process/thread metadata events.
    assert_eq!(
        trace["traceEvents"].as_array().unwrap().len(),
        Phase::ALL.len() + 2
    );
    let stats = StatsReport {
        profile,
        mono_bodies: 0,
        warnings: 0,
    };
    let value: serde_json::Value =
        serde_json::from_str(&stats.render_json()).expect("empty stats parse");
    assert_eq!(value["total_micros"], 0);
}

/// A profile with fixed micros per phase so renderer output is exactly
/// assertable. Total = 660µs (10..=110 step 10).
fn synthetic_profile() -> PhaseProfile {
    let mut profile = PhaseProfile::EMPTY.clone();
    for phase in Phase::ALL {
        profile = profile.with_micros(phase, synthetic_micros(phase));
    }
    profile
}

/// Deterministic per-phase micros: `load` = 1µs, `parse` = 2µs, …
fn synthetic_micros(phase: Phase) -> u64 {
    let idx = Phase::ALL
        .iter()
        .position(|p| *p == phase)
        .expect("known phase");
    (idx + 1) as u64 * 10
}

/// The sum of 10..=110 step 10 = 660µs.
fn synthetic_total() -> u64 {
    Phase::ALL.iter().map(|p| synthetic_micros(*p)).sum()
}
