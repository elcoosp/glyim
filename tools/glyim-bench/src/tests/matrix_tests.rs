//! Matrix gate tests (plan M2.5 remainder): the multi-report diff matrix
//! over the benchmark set, the geo-mean aggregate budget (the plan's
//! "fail >2% per benchmark or >0.5% geo-mean" criterion, §325/§6 M2.5),
//! and the `BaselineSource` seam that decouples the gate from where
//! baselines live.

use crate::diff::{DiffThresholds, DiffVerdict, InMemoryReportStore, ReportStore};
use crate::error::BenchError;
use crate::history::{HistoryRecord, HistoryStore, InMemoryHistoryStore, JsonlHistoryStore};
use crate::matrix::{
    BENCH_MATRIX_SCHEMA, BaselineSource, HistoryBaselineSource, InMemoryBaselineSource,
    MatrixOutcome, MatrixThresholds, diff_matrix, execute_diff_matrix, geo_mean_delta_pct,
};
use crate::report::{BenchReport, BenchSample, GateOutcome};
use crate::source::MeasurementSource;
use crate::stats::{DEFAULT_MAX_COV_PCT, summarize};
use std::path::{Path, PathBuf};

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

/// The plan's default matrix budgets: `+2%` per benchmark, `+0.5%` geo-mean.
fn thresholds() -> MatrixThresholds {
    MatrixThresholds::new(2.0, 0.5)
}

// -- the geo-mean aggregate -------------------------------------------

#[test]
fn geo_mean_of_equal_deltas_is_the_delta() {
    let geo = geo_mean_delta_pct(&[3.0, 3.0, 3.0]);
    assert!((geo - 3.0).abs() < 1e-9, "geo of equal deltas: {geo}");
}

#[test]
fn geo_mean_matches_the_hand_computed_value() {
    // (1.01 · 1.03)^(1/2) − 1 = 1.995012…%
    let geo = geo_mean_delta_pct(&[1.0, 3.0]);
    let expected = ((1.01_f64 * 1.03).sqrt() - 1.0) * 100.0;
    assert!((geo - expected).abs() < 1e-9, "geo {geo} vs {expected}");
}

#[test]
fn geo_mean_is_below_the_arithmetic_mean_for_mixed_deltas() {
    let geo = geo_mean_delta_pct(&[-10.0, 30.0]);
    assert!(
        geo < 10.0,
        "geometric mean {geo} must stay below the arithmetic +10%"
    );
}

#[test]
fn geo_mean_is_deterministic_in_input_order() {
    let forward = geo_mean_delta_pct(&[1.0, 2.0, 4.0]);
    let forward_again = geo_mean_delta_pct(&[1.0, 2.0, 4.0]);
    assert_eq!(forward, forward_again, "no clock, no PRNG (plan C6)");
}

#[test]
fn geo_mean_of_an_improvement_is_negative() {
    let geo = geo_mean_delta_pct(&[-50.0, -50.0]);
    assert!((geo + 50.0).abs() < 1e-9, "geo {geo}");
}

// -- the pure matrix engine -------------------------------------------

#[test]
fn matrix_rows_follow_the_input_order_with_per_row_verdicts() {
    let baselines = InMemoryBaselineSource::new()
        .with_baseline("fib", report("fib", 1_000, None))
        .with_baseline("parse", report("parse", 2_000, None));
    let reports = vec![
        (PathBuf::from("fib.json"), report("fib", 1_100, None)),
        (PathBuf::from("parse.json"), report("parse", 2_000, None)),
    ];
    let outcome = diff_matrix(&reports, &baselines, &thresholds()).expect("matrix");
    assert_eq!(outcome.rows.len(), 2);
    assert_eq!(outcome.rows[0].diff.program, "fib");
    assert_eq!(outcome.rows[0].report_path, "fib.json");
    assert_eq!(outcome.rows[0].diff.wall.verdict, DiffVerdict::Regressed);
    assert_eq!(outcome.rows[1].diff.wall.verdict, DiffVerdict::Unchanged);
    assert!(!outcome.passed, "a regressed row fails the matrix gate");
}

#[test]
fn matrix_reuses_the_m2_5_diff_engine_for_rows() {
    // A +5% delta against a +2% per-benchmark budget is a per-row
    // regression with the same boundary the `diff` gate applies.
    let baselines = InMemoryBaselineSource::new().with_baseline("fib", report("fib", 1_000, None));
    let reports = vec![(PathBuf::from("c.json"), report("fib", 1_050, None))];
    let outcome = diff_matrix(&reports, &baselines, &thresholds()).expect("matrix");
    assert_eq!(outcome.rows[0].diff.wall.delta_pct, 5.0);
    assert_eq!(outcome.rows[0].diff.wall.verdict, DiffVerdict::Regressed);
}

#[test]
fn matrix_missing_baseline_is_a_named_error() {
    let baselines = InMemoryBaselineSource::new();
    let reports = vec![(PathBuf::from("c.json"), report("unknown", 1_000, None))];
    let err =
        diff_matrix(&reports, &baselines, &thresholds()).expect_err("no baseline for `unknown`");
    assert!(matches!(&err, BenchError::BaselineUnavailable { program } if program == "unknown"));
    assert!(err.to_string().contains("unknown"));
}

#[test]
fn matrix_rejects_a_duplicate_program_and_an_empty_set() {
    let baselines = InMemoryBaselineSource::new().with_baseline("fib", report("fib", 1_000, None));
    let duplicated = vec![
        (PathBuf::from("a.json"), report("fib", 1_000, None)),
        (PathBuf::from("b.json"), report("fib", 1_000, None)),
    ];
    let err = diff_matrix(&duplicated, &baselines, &thresholds()).expect_err("same program twice");
    assert!(matches!(err, BenchError::MatrixPlan { .. }));
    assert!(err.to_string().contains("fib"));

    let err = diff_matrix(&[], &baselines, &thresholds()).expect_err("empty matrix");
    assert!(matches!(err, BenchError::MatrixPlan { .. }));
}

#[test]
fn the_plan_scenario_uniform_sub_budget_drift_fails_only_the_geo_gate() {
    // §325/§6 M2.5: ">2% on a benchmark or >0.5% geo-mean = fail". A change
    // that slows *every* benchmark by exactly +1% passes each per-benchmark
    // budget (+1% <= +2%) and must still fail the matrix on the geo-mean
    // (+1% > +0.5%) — that is the class of regression this gate exists for.
    let baselines = InMemoryBaselineSource::new()
        .with_baseline("fib", report("fib", 1_000, None))
        .with_baseline("parse", report("parse", 2_000, None))
        .with_baseline("lower", report("lower", 4_000, None));
    let reports = vec![
        (PathBuf::from("a.json"), report("fib", 1_010, None)),
        (PathBuf::from("b.json"), report("parse", 2_020, None)),
        (PathBuf::from("c.json"), report("lower", 4_040, None)),
    ];
    let outcome = diff_matrix(&reports, &baselines, &thresholds()).expect("matrix");
    for row in &outcome.rows {
        assert_eq!(
            row.diff.wall.verdict,
            DiffVerdict::Unchanged,
            "within the per-benchmark budget"
        );
    }
    let geo = outcome
        .wall_geo_delta_pct
        .expect("wall geo over three rows");
    assert!((geo - 1.0).abs() < 1e-9, "uniform +1%: geo {geo}");
    assert!(!outcome.passed, "the geo-mean budget catches the drift");
}

#[test]
fn matrix_geo_within_budget_passes_and_improvements_never_fail() {
    let baselines = InMemoryBaselineSource::new()
        .with_baseline("fib", report("fib", 1_000, None))
        .with_baseline("parse", report("parse", 2_000, None));
    let reports = vec![
        (PathBuf::from("a.json"), report("fib", 1_010, None)),
        (PathBuf::from("b.json"), report("parse", 1_900, None)),
    ];
    let outcome = diff_matrix(&reports, &baselines, &thresholds()).expect("matrix");
    assert_eq!(outcome.rows[1].diff.wall.verdict, DiffVerdict::Improved);
    assert!(outcome.passed, "an improved row never fails the gate");
}

#[test]
fn matrix_geo_mean_over_instructions_requires_full_coverage() {
    // Honesty rule (the budget gate's `Unmeasured` philosophy): a geo-mean
    // over a *subset* of the rows would look like coverage it does not have.
    // Mixed instruction coverage -> `None`, and the instruction geo budget
    // is not applied.
    let baselines = InMemoryBaselineSource::new()
        .with_baseline("fib", report("fib", 1_000, Some(2_000)))
        .with_baseline("parse", report("parse", 2_000, None));
    let reports = vec![
        (PathBuf::from("a.json"), report("fib", 1_100, Some(2_400))),
        (PathBuf::from("b.json"), report("parse", 2_000, None)),
    ];
    let outcome = diff_matrix(&reports, &baselines, &thresholds()).expect("matrix");
    assert!(outcome.instructions_geo_delta_pct.is_none());

    // Full coverage -> the geo-mean is measured and a regression fails.
    let covered = InMemoryBaselineSource::new()
        .with_baseline("fib", report("fib", 1_000, Some(2_000)))
        .with_baseline("parse", report("parse", 2_000, Some(4_000)));
    let reports = vec![
        (PathBuf::from("a.json"), report("fib", 1_000, Some(2_100))),
        (PathBuf::from("b.json"), report("parse", 2_000, Some(4_100))),
    ];
    let outcome = diff_matrix(&reports, &covered, &thresholds()).expect("matrix");
    let geo = outcome
        .instructions_geo_delta_pct
        .expect("every row measured instructions");
    assert!(geo > 0.5, "uniform +5% instructions: geo {geo}");
    assert!(!outcome.passed, "the instruction geo budget fails the gate");
}

// -- the BaselineSource seam ------------------------------------------

#[test]
fn in_memory_baseline_source_describes_itself() {
    let empty = InMemoryBaselineSource::new();
    assert_eq!(empty.len(), 0);
    assert!(empty.describe().contains("in-memory"));
    let source = empty.with_baseline("fib", report("fib", 1_000, None));
    assert_eq!(source.len(), 1);
    assert_eq!(
        source.baseline_for("fib").expect("baseline").wall.mean,
        1_000.0
    );
    assert!(source.baseline_for("absent").is_err());
}

#[test]
fn history_baseline_source_selects_the_latest_record_per_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let history_path = dir.path().join("history.jsonl");
    let store = JsonlHistoryStore;
    store
        .append(&history_path, &record("fib", 1_000, None))
        .expect("append older");
    store
        .append(&history_path, &record("parse", 2_000, None))
        .expect("append other program");
    store
        .append(&history_path, &record("fib", 1_200, None))
        .expect("append latest");

    let source = HistoryBaselineSource::new(&store, &history_path);
    let baseline = source.baseline_for("fib").expect("baseline");
    assert_eq!(baseline.wall.mean, 1_200.0, "file order is append order");
    assert!(
        source
            .describe()
            .contains(history_path.to_string_lossy().as_ref()),
        "the outcome names where the baselines came from: {}",
        source.describe()
    );
    assert!(matches!(
        source.baseline_for("absent"),
        Err(BenchError::BaselineUnavailable { .. })
    ));
}

#[test]
fn baseline_sources_stay_behind_the_trait() {
    // Both impls satisfy the same object-safe seam the wiring consumes.
    fn takes_dyn(source: &dyn BaselineSource) -> f64 {
        source.baseline_for("fib").expect("baseline").wall.mean
    }
    let in_memory = InMemoryBaselineSource::new().with_baseline("fib", report("fib", 1_000, None));
    assert_eq!(takes_dyn(&in_memory), 1_000.0);

    let dir = tempfile::tempdir().expect("tempdir");
    let history_path = dir.path().join("history.jsonl");
    JsonlHistoryStore
        .append(&history_path, &record("fib", 1_500, None))
        .expect("append");
    let from_history = HistoryBaselineSource::new(&JsonlHistoryStore, &history_path);
    assert_eq!(takes_dyn(&from_history), 1_500.0);
}

// -- the store wiring --------------------------------------------------

#[test]
fn execute_diff_matrix_loads_reports_through_the_report_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    let reports = InMemoryReportStore::new();
    reports
        .save(&dir.path().join("fib.json"), &report("fib", 1_000, None))
        .expect("save fib");
    reports
        .save(
            &dir.path().join("parse.json"),
            &report("parse", 2_000, None),
        )
        .expect("save parse");

    let history = InMemoryHistoryStore::new();
    history
        .append(Path::new("perf/history.jsonl"), &record("fib", 1_000, None))
        .expect("append fib");
    history
        .append(
            Path::new("perf/history.jsonl"),
            &record("parse", 2_000, None),
        )
        .expect("append parse");
    let baselines = HistoryBaselineSource::new(&history, Path::new("perf/history.jsonl"));

    let outcome = execute_diff_matrix(
        &reports,
        &baselines,
        &[dir.path().join("fib.json"), dir.path().join("parse.json")],
        &thresholds(),
    )
    .expect("matrix");
    assert_eq!(outcome.rows.len(), 2);
    assert!(outcome.passed);
    assert!(outcome.baseline_source.contains("perf/history.jsonl"));
}

#[test]
fn execute_diff_matrix_propagates_report_load_failures() {
    let reports = InMemoryReportStore::new();
    let baselines = InMemoryBaselineSource::new();
    let err = execute_diff_matrix(
        &reports,
        &baselines,
        &[PathBuf::from("absent.json")],
        &thresholds(),
    )
    .expect_err("report missing");
    assert!(matches!(err, BenchError::ReportRead { .. }));
}

// -- the outcome document ---------------------------------------------

#[test]
fn matrix_outcome_pins_the_schema_tag_and_round_trips() {
    let baselines = InMemoryBaselineSource::new().with_baseline("fib", report("fib", 1_000, None));
    let reports = vec![(PathBuf::from("c.json"), report("fib", 1_000, None))];
    let outcome = diff_matrix(&reports, &baselines, &thresholds()).expect("matrix");
    assert_eq!(outcome.schema, BENCH_MATRIX_SCHEMA);
    let json = outcome.to_json_pretty().expect("serialises");
    assert!(json.contains(BENCH_MATRIX_SCHEMA), "self-describing JSON");
    let back: MatrixOutcome = serde_json::from_str(&json).expect("parses");
    assert_eq!(back, outcome, "deny_unknown_fields round trip");
}

#[test]
fn matrix_rendering_is_deterministic_and_names_the_budgets() {
    let baselines = InMemoryBaselineSource::new()
        .with_baseline("fib", report("fib", 1_000, None))
        .with_baseline("parse", report("parse", 2_000, None));
    let reports = vec![
        (PathBuf::from("a.json"), report("fib", 1_010, None)),
        (PathBuf::from("b.json"), report("parse", 2_010, None)),
    ];
    let outcome = diff_matrix(&reports, &baselines, &thresholds()).expect("matrix");
    let first = outcome.render_text();
    assert_eq!(
        first,
        outcome.render_text(),
        "deterministic for a given outcome"
    );
    assert!(first.contains(BENCH_MATRIX_SCHEMA));
    assert!(first.contains("+2.000%"), "per-benchmark budget rendered");
    assert!(first.contains("+0.500%"), "geo-mean budget rendered");
    assert!(first.contains("FAILED"), "geo drift is a failure");
}

#[test]
fn diff_thresholds_stay_untouched_by_the_matrix_budgets() {
    // The single-pair `diff` gate keeps its own threshold type; the matrix
    // budgets extend rather than mutate the M2.5 contract.
    let single = DiffThresholds::new(5.0);
    assert_eq!(single.max_delta_pct, 5.0);
    let matrix = MatrixThresholds::new(2.0, 0.5);
    assert_eq!(matrix.max_delta_pct, 2.0);
    assert_eq!(matrix.max_geo_delta_pct, 0.5);
}
