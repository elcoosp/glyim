//! Trend analyzer tests (plan M2.5 complement): per-record drift across the
//! committed history window — the accumulated multi-PR regression that
//! latest-baseline diffs never see, because every individual PR stayed
//! within its own budget.

use crate::error::BenchError;
use crate::history::{HistoryRecord, HistoryStore, JsonlHistoryStore};
use crate::report::{BenchReport, BenchSample, GateOutcome};
use crate::source::MeasurementSource;
use crate::stats::{DEFAULT_MAX_COV_PCT, summarize};
use crate::trend::{
    BENCH_TREND_SCHEMA, TrendReport, TrendThresholds, execute_trend_from_history,
    trend_over_history,
};

/// A `glyim.bench/1` report over a constant sample set (exact means).
fn report(program: &str, mean_ns: u64, instructions: Option<u64>) -> BenchReport {
    let samples = vec![
        BenchSample {
            wall_ns: mean_ns,
            instructions,
            cycles: None,
            rss_kb: None,
            alloc_count: None,
            alloc_bytes: None,
        };
        10
    ];
    let wall = summarize(&[mean_ns; 10]).expect("non-empty");
    let instr_stats = instructions.map(|i| summarize(&[i; 10]).expect("non-empty"));
    BenchReport::new(
        program,
        Vec::new(),
        MeasurementSource::WallOnly,
        10,
        samples,
        instr_stats,
        wall,
        GateOutcome {
            metric: "wall_ns".to_string(),
            cov_pct: 0.0,
            max_cov_pct: DEFAULT_MAX_COV_PCT,
            passed: true,
        },
    )
}

fn record(program: &str, mean_ns: u64, instructions: Option<u64>) -> HistoryRecord {
    HistoryRecord::new(report(program, mean_ns, instructions))
}

fn thresholds() -> TrendThresholds {
    TrendThresholds::new(8, 5.0)
}

// -- the pure trend engine --------------------------------------------

#[test]
fn trend_pins_the_schema_tag_and_round_trips() {
    let records = vec![record("prog", 1_000, None), record("prog", 1_100, None)];
    let trend = trend_over_history(&records, "prog", &thresholds()).expect("trend");
    assert_eq!(trend.schema, BENCH_TREND_SCHEMA);
    let json = trend.to_json_pretty().expect("serialises");
    assert!(json.contains(BENCH_TREND_SCHEMA), "self-describing JSON");
    let back: TrendReport = serde_json::from_str(&json).expect("parses");
    assert_eq!(back, trend, "deny_unknown_fields round trip");
}

#[test]
fn trend_measures_first_to_last_over_the_window() {
    // Window 8 but only 4 records for `prog`: the trend spans the whole
    // (truncated) window — first 1_000 to last 1_100 = +10%.
    let records = vec![
        record("prog", 1_000, None),
        record("prog", 1_050, None),
        record("prog", 1_080, None),
        record("prog", 1_100, None),
    ];
    let trend = trend_over_history(&records, "prog", &thresholds()).expect("trend");
    assert_eq!(trend.window, 4);
    assert_eq!(trend.records_total, 4);
    assert_eq!(trend.first_wall_mean, 1_000.0);
    assert_eq!(trend.last_wall_mean, 1_100.0);
    assert!((trend.wall_drift_pct - 10.0).abs() < 1e-9);
    assert!(!trend.passed, "+10% drift exceeds the +5% budget");
}

#[test]
fn trend_uses_only_the_last_window_records() {
    // 10 records, window 8: the first two fall out of the window, so the
    // first point of the trend is the third record (1_020).
    let records: Vec<HistoryRecord> = (0..10)
        .map(|i| record("prog", 1_000 + i * 10, None))
        .collect();
    let trend = trend_over_history(&records, "prog", &thresholds()).expect("trend");
    assert_eq!(trend.window, 8);
    assert_eq!(trend.records_total, 10);
    assert_eq!(trend.first_wall_mean, 1_020.0);
    assert_eq!(trend.last_wall_mean, 1_090.0);
    let expected = (1_090.0 - 1_020.0) / 1_020.0 * 100.0;
    assert!((trend.wall_drift_pct - expected).abs() < 1e-9);
    // ≈+6.86% of accumulated drift over the window vs a +5% budget: the
    // accumulated multi-PR regression the latest-baseline diff never sees.
    assert!(!trend.passed);
}

#[test]
fn trend_of_other_programs_is_ignored() {
    let records = vec![
        record("other", 5_000, None),
        record("prog", 1_000, None),
        record("other", 9_000, None),
        record("prog", 1_100, None),
    ];
    let trend = trend_over_history(&records, "prog", &thresholds()).expect("trend");
    assert_eq!(trend.records_total, 2, "only `prog` records count");
    assert_eq!(trend.first_wall_mean, 1_000.0);
    assert_eq!(trend.last_wall_mean, 1_100.0);
}

#[test]
fn trend_unknown_program_is_a_named_error() {
    let err = trend_over_history(&[], "unknown", &thresholds()).expect_err("no records at all");
    assert!(matches!(&err, BenchError::BaselineUnavailable { program } if program == "unknown"));
}

#[test]
fn trend_needs_two_records_and_a_window_of_two() {
    let one = vec![record("prog", 1_000, None)];
    let err = trend_over_history(&one, "prog", &thresholds()).expect_err("single record");
    assert!(matches!(err, BenchError::TrendPlan { .. }));
    assert!(err.to_string().contains("prog"));

    let two = vec![record("prog", 1_000, None), record("prog", 1_100, None)];
    let err = trend_over_history(&two, "prog", &TrendThresholds::new(1, 5.0))
        .expect_err("window below two");
    assert!(matches!(err, BenchError::TrendPlan { .. }));
}

#[test]
fn trend_instructions_drift_requires_full_window_coverage() {
    // Honesty rule: mixed instruction coverage -> `None`, not gated.
    let records = vec![
        record("prog", 1_000, Some(2_000)),
        record("prog", 1_100, None),
    ];
    let trend = trend_over_history(&records, "prog", &thresholds()).expect("trend");
    assert!(trend.instructions_drift_pct.is_none());

    // Full coverage -> measured, and a regression fails the gate.
    let covered = vec![
        record("prog", 1_000, Some(2_000)),
        record("prog", 1_000, Some(2_200)),
    ];
    let trend = trend_over_history(&covered, "prog", &thresholds()).expect("trend");
    let drift = trend
        .instructions_drift_pct
        .expect("every window point measured instructions");
    assert!((drift - 10.0).abs() < 1e-9);
    assert!(!trend.passed);
}

#[test]
fn trend_gate_boundaries_and_improvements() {
    // Exactly at the budget: the inclusive `<=` boundary passes.
    let at_budget = vec![record("prog", 1_000, None), record("prog", 1_050, None)];
    let trend =
        trend_over_history(&at_budget, "prog", &TrendThresholds::new(2, 5.0)).expect("trend");
    assert!(trend.passed, "+5.0% drift at a +5.0% budget passes");

    // An improvement is a negative drift and never fails.
    let improved = vec![record("prog", 1_000, None), record("prog", 900, None)];
    let trend = trend_over_history(&improved, "prog", &thresholds()).expect("trend");
    assert!(trend.wall_drift_pct < 0.0);
    assert!(trend.passed, "improvements ratchet, they do not fail");
}

#[test]
fn trend_zero_first_mean_is_an_honest_infinite_drift() {
    // Degenerate but deterministic (the diff engine's rule): a zero first
    // mean with a nonzero last mean is unbounded growth.
    let records = vec![record("prog", 0, None), record("prog", 1_000, None)];
    let trend = trend_over_history(&records, "prog", &thresholds()).expect("trend");
    assert!(trend.wall_drift_pct.is_infinite());
    assert!(!trend.passed);
}

// -- the store wiring --------------------------------------------------

#[test]
fn execute_trend_from_history_wires_the_history_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    let history_path = dir.path().join("history.jsonl");
    let store = JsonlHistoryStore;
    for i in 0..4 {
        store
            .append(&history_path, &record("prog", 1_000 + i * 40, None))
            .expect("append");
    }
    store
        .append(&history_path, &record("other", 9_999, None))
        .expect("append other");

    let trend =
        execute_trend_from_history(&store, &history_path, "prog", &thresholds()).expect("trend");
    assert_eq!(trend.records_total, 4);
    assert_eq!(trend.last_wall_mean, 1_120.0);
    assert!(!trend.passed, "+12% over the window vs a +5% budget");

    let err = execute_trend_from_history(&store, &history_path, "absent", &thresholds())
        .expect_err("unknown program");
    assert!(matches!(err, BenchError::BaselineUnavailable { .. }));
}

#[test]
fn trend_rendering_is_deterministic_and_names_the_budget() {
    let records = vec![record("prog", 1_000, None), record("prog", 1_100, None)];
    let trend = trend_over_history(&records, "prog", &thresholds()).expect("trend");
    let first = trend.render_text();
    assert_eq!(
        first,
        trend.render_text(),
        "deterministic for a given report"
    );
    assert!(first.contains(BENCH_TREND_SCHEMA));
    assert!(first.contains("+5.0%"), "the drift budget is rendered");
    assert!(first.contains("FAILED"));
    assert!(trend.is_ok() == trend.passed, "ledger-convention alias");
}

#[test]
fn trend_points_carry_the_window_evidence() {
    let records = vec![
        record("prog", 1_000, Some(2_000)),
        record("prog", 1_100, None),
    ];
    let trend = trend_over_history(&records, "prog", &thresholds()).expect("trend");
    assert_eq!(trend.points.len(), 2);
    assert_eq!(trend.points[0].wall_mean, 1_000.0);
    assert_eq!(trend.points[0].instructions_mean, Some(2_000.0));
    assert_eq!(trend.points[1].instructions_mean, None, "as recorded");
}
