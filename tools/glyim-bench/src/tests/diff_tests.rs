//! Report-to-report diff tests (M2.5 seam): the delta engine, the
//! `ReportStore` seam, the outcome renderers, and the regression gate.

use crate::diff::{
    BENCH_DIFF_SCHEMA, DiffThresholds, DiffVerdict, FileReportStore, InMemoryReportStore,
    ReportStore, execute_diff,
};
use crate::error::BenchError;
use crate::report::{BenchReport, BenchSample, GateOutcome};
use crate::source::MeasurementSource;
use crate::stats::{DEFAULT_MAX_COV_PCT, summarize};
use std::path::Path;

/// A `glyim.bench/1` report over a constant sample set.
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

/// Identical reports diff to `Unchanged` on every metric and the gate passes.
#[test]
fn identical_reports_diff_to_unchanged() {
    let base = report("prog", 1_000, Some(2_000));
    let cur = report("prog", 1_000, Some(2_000));
    let diff =
        crate::diff::diff_reports(&base, &cur, &DiffThresholds::new(5.0)).expect("same program");
    assert_eq!(diff.wall.verdict, DiffVerdict::Unchanged);
    assert_eq!(diff.wall.delta_pct, 0.0);
    assert_eq!(
        diff.instructions.as_ref().expect("both have").verdict,
        DiffVerdict::Unchanged
    );
}

/// A slower current run beyond the budget regresses and fails the gate.
#[test]
fn slower_report_beyond_budget_regresses() {
    let base = report("prog", 1_000, Some(2_000));
    let cur = report("prog", 1_200, Some(2_000));
    let diff =
        crate::diff::diff_reports(&base, &cur, &DiffThresholds::new(5.0)).expect("same program");
    assert_eq!(diff.wall.verdict, DiffVerdict::Regressed);
    assert!((diff.wall.delta_pct - 20.0).abs() < 1e-9);
    // Instructions unchanged: the verdict is per-metric.
    assert_eq!(
        diff.instructions.as_ref().expect("both have").verdict,
        DiffVerdict::Unchanged
    );
}

/// A slower current run *within* the budget stays `Unchanged`.
#[test]
fn slower_within_budget_is_unchanged() {
    let base = report("prog", 1_000, None);
    let cur = report("prog", 1_040, None);
    let diff =
        crate::diff::diff_reports(&base, &cur, &DiffThresholds::new(5.0)).expect("same program");
    assert_eq!(diff.wall.verdict, DiffVerdict::Unchanged);
}

/// Any strict improvement reports `Improved`.
#[test]
fn faster_report_improves() {
    let base = report("prog", 1_000, Some(2_000));
    let cur = report("prog", 800, Some(1_000));
    let diff =
        crate::diff::diff_reports(&base, &cur, &DiffThresholds::new(5.0)).expect("same program");
    assert_eq!(diff.wall.verdict, DiffVerdict::Improved);
    assert_eq!(
        diff.instructions.as_ref().expect("both have").verdict,
        DiffVerdict::Improved
    );
}

/// Instructions are compared only when both reports carry them (the M1.5
/// evidence columns are optional per counter source).
#[test]
fn instructions_compared_only_when_both_present() {
    let base = report("prog", 1_000, Some(2_000));
    let cur = report("prog", 1_000, None);
    let diff =
        crate::diff::diff_reports(&base, &cur, &DiffThresholds::new(5.0)).expect("same program");
    assert!(diff.instructions.is_none(), "current lacks instructions");

    let base = report("prog", 1_000, None);
    let cur = report("prog", 1_000, Some(2_000));
    let diff =
        crate::diff::diff_reports(&base, &cur, &DiffThresholds::new(5.0)).expect("same program");
    assert!(diff.instructions.is_none(), "baseline lacks instructions");
}

/// Diffing two different programs is an invalid diff plan, not a spurious
/// verdict.
#[test]
fn program_mismatch_is_invalid_plan() {
    let base = report("a", 1_000, None);
    let cur = report("b", 1_000, None);
    let err =
        crate::diff::diff_reports(&base, &cur, &DiffThresholds::new(5.0)).expect_err("mismatch");
    assert!(matches!(err, BenchError::InvalidPlan { .. }));
}

/// The outcome envelope: schema tag, paths, gate verdict, and the ledger
/// convention's `is_ok()` trio.
#[test]
fn outcome_envelope_and_gate() {
    let base = report("prog", 1_000, Some(2_000));
    let cur = report("prog", 2_000, Some(2_000));
    let outcome = crate::diff::execute_diff(
        &InMemoryReportStore::default(),
        Path::new("unused"),
        Path::new("unused"),
        &DiffThresholds::new(5.0),
    );
    assert!(outcome.is_err(), "empty store cannot load");

    let store = InMemoryReportStore::default();
    store
        .save(Path::new("/ledger/base.json"), &base)
        .expect("save");
    store
        .save(Path::new("/ledger/cur.json"), &cur)
        .expect("save");
    let outcome = execute_diff(
        &store,
        Path::new("/ledger/base.json"),
        Path::new("/ledger/cur.json"),
        &DiffThresholds::new(5.0),
    )
    .expect("both present");
    assert_eq!(outcome.schema, BENCH_DIFF_SCHEMA);
    assert_eq!(outcome.baseline_path, "/ledger/base.json");
    assert_eq!(outcome.current_path, "/ledger/cur.json");
    assert!(!outcome.is_ok(), "doubled wall time must fail the gate");
    assert_eq!(outcome.diffs.len(), 1);
}

/// The outcome round-trips through its `glyim.bench-diff/1` JSON with the
/// same strictness as the report document.
#[test]
fn outcome_json_round_trip() {
    let base = report("prog", 1_000, Some(2_000));
    let cur = report("prog", 1_100, Some(2_000));
    let store = InMemoryReportStore::default();
    store.save(Path::new("b"), &base).expect("save");
    store.save(Path::new("c"), &cur).expect("save");
    let outcome = execute_diff(
        &store,
        Path::new("b"),
        Path::new("c"),
        &DiffThresholds::new(5.0),
    )
    .expect("both present");
    let json = outcome.to_json_pretty().expect("serialise");
    let back: crate::diff::BenchDiffOutcome = serde_json::from_str(&json).expect("strict parse");
    assert_eq!(back, outcome);
    // Determinism: two renders are byte-identical (plan C6).
    assert_eq!(json, outcome.to_json_pretty().expect("serialise"));
}

/// The text renderer pins the verdict lines and the gate summary.
#[test]
fn text_renderer_is_deterministic_and_complete() {
    let base = report("prog", 1_000, Some(2_000));
    let cur = report("prog", 1_100, Some(2_000));
    let store = InMemoryReportStore::default();
    store.save(Path::new("b"), &base).expect("save");
    store.save(Path::new("c"), &cur).expect("save");
    let outcome = execute_diff(
        &store,
        Path::new("b"),
        Path::new("c"),
        &DiffThresholds::new(5.0),
    )
    .expect("both present");
    let text = outcome.render_text();
    assert!(text.contains("glyim.bench-diff/1"));
    assert!(text.contains("wall_ns"));
    assert!(text.contains("+10.000%"));
    assert!(text.contains("REGRESSED"));
    assert_eq!(text, outcome.render_text());
}

/// `FileReportStore` loads what a previous run wrote, through the trait.
#[test]
fn file_store_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("report.json");
    let rep = report("prog", 1_000, Some(2_000));
    let store = FileReportStore;
    store.save(&path, &rep).expect("save");
    let loaded: BenchReport = store.load(&path).expect("load");
    assert_eq!(loaded, rep);
    let missing = store.load(&dir.path().join("nope.json"));
    assert!(matches!(
        missing,
        Err(crate::error::BenchError::ReportRead { .. })
    ));
}
