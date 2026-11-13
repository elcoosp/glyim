//! Perf-history store tests (plan M2.5 remainder): the JSONL
//! `glyim.bench-history/1` records, the `HistoryStore` seam, the
//! latest-baseline selection, and the diff-from-history wiring.

use crate::diff::FileReportStore;
use crate::diff::{DiffThresholds, DiffVerdict, ReportStore};
use crate::error::BenchError;
use crate::history::{
    HISTORY_SCHEMA, HistoryRecord, HistoryStore, InMemoryHistoryStore, JsonlHistoryStore,
    execute_diff_from_history, latest_baseline,
};
use crate::report::{BenchReport, BenchSample, GateOutcome};
use crate::source::MeasurementSource;
use crate::stats::{DEFAULT_MAX_COV_PCT, summarize};
use std::path::{Path, PathBuf};

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

fn record(program: &str, mean_ns: u64, instructions: Option<u64>) -> HistoryRecord {
    HistoryRecord::new(report(program, mean_ns, instructions))
}

// -- the record contract ---------------------------------------------

#[test]
fn history_record_pins_the_schema_tag() {
    let record = record("prog", 1_000, None);
    assert_eq!(record.schema, HISTORY_SCHEMA);
    let line = record.to_jsonl_line().expect("serialises");
    assert!(line.contains(HISTORY_SCHEMA), "self-describing line");
    assert!(!line.contains('\n'), "one JSON object per line");
}

#[test]
fn history_record_round_trips_through_a_line() {
    let record = record("prog", 1_234, Some(5_678));
    let line = record.to_jsonl_line().expect("serialises");
    let back = HistoryRecord::from_jsonl_line(&line).expect("parses");
    assert_eq!(back, record);
}

#[test]
fn history_line_rejects_unknown_fields_and_wrong_schema() {
    let mut record = record("prog", 1_000, None);
    record.schema = "glyim.bench/1".to_string();
    let line = record.to_jsonl_line().expect("serialises");
    let err = HistoryRecord::from_jsonl_line(&line).expect_err("wrong schema tag");
    assert!(matches!(err, BenchError::HistoryParse { .. }));
    assert!(err.to_string().contains(HISTORY_SCHEMA));

    let sneaky = format!("{},\"sneaky\":true}}", line.trim_end_matches('}'));
    assert!(HistoryRecord::from_jsonl_line(&sneaky).is_err());
}

// -- the HistoryStore seam -------------------------------------------

#[test]
fn in_memory_history_store_appends_in_order() {
    let store = InMemoryHistoryStore::new();
    let path = Path::new("mem/history.jsonl");
    store
        .append(path, &record("prog", 1_000, None))
        .expect("append first");
    store
        .append(path, &record("prog", 1_100, None))
        .expect("append second");
    let records = store.load_all(path).expect("loaded");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].report.wall.mean, 1_000.0);
    assert_eq!(records[1].report.wall.mean, 1_100.0);
    assert!(
        store
            .load_all(Path::new("mem/absent.jsonl"))
            .expect("absent")
            .is_empty()
    );
}

#[test]
fn jsonl_history_store_round_trips_and_tolerates_blank_lines() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("history.jsonl");
    let store = JsonlHistoryStore;
    store
        .append(&path, &record("prog", 1_000, None))
        .expect("append");
    store
        .append(&path, &record("prog", 1_100, None))
        .expect("append");
    // A trailing blank line (editors do this) must not break the load.
    let existing = std::fs::read_to_string(&path).expect("read");
    std::fs::write(&path, format!("{existing}\n")).expect("append blank line");
    let records = store.load_all(&path).expect("loaded");
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].report.wall.mean, 1_100.0);
    assert!(matches!(
        store.load_all(&dir.path().join("nope.jsonl")),
        Ok(empty) if empty.is_empty()
    ));
}

#[test]
fn jsonl_history_store_names_a_broken_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("history.jsonl");
    std::fs::write(&path, "{not json}\n").expect("write junk");
    match JsonlHistoryStore.load_all(&path) {
        Err(BenchError::HistoryParse { detail }) => {
            assert!(detail.contains("line 1"), "names the offending line");
        }
        other => panic!("expected HistoryParse, got {other:?}"),
    }
}

// -- latest-baseline selection ---------------------------------------

#[test]
fn latest_baseline_selects_the_last_matching_record() {
    let records = vec![
        record("prog", 1_000, None),
        record("other", 5_000, None),
        record("prog", 1_200, None),
    ];
    let baseline = latest_baseline(&records, "prog").expect("history exists");
    assert_eq!(baseline.program, "prog");
    assert_eq!(baseline.wall.mean, 1_200.0, "file order is append order");
}

#[test]
fn latest_baseline_errors_on_an_empty_history() {
    let err = latest_baseline(&[], "prog").expect_err("no history");
    assert!(matches!(err, BenchError::BaselineUnavailable { .. }));
    assert!(err.to_string().contains("prog"));
}

// -- diff-from-history wiring ----------------------------------------

#[test]
fn execute_diff_from_history_diffs_against_the_latest_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    let history_path = dir.path().join("history.jsonl");
    let store = JsonlHistoryStore;
    store
        .append(&history_path, &record("prog", 1_000, Some(2_000)))
        .expect("append older");
    store
        .append(&history_path, &record("prog", 1_500, Some(2_100)))
        .expect("append latest");

    let current_path = dir.path().join("current.json");
    let reports = FileReportStore;
    reports
        .save(&current_path, &report("prog", 1_600, Some(2_050)))
        .expect("save current");

    let outcome = execute_diff_from_history(
        &reports,
        &store,
        &history_path,
        &current_path,
        &DiffThresholds::new(5.0),
    )
    .expect("diffs");
    // Baseline is the *latest* history record (1_500), not the first.
    assert_eq!(
        outcome.baseline_path,
        format!("{}#latest", history_path.display())
    );
    assert_eq!(outcome.diffs[0].wall.baseline, 1_500.0);
    assert_eq!(outcome.diffs[0].wall.verdict, DiffVerdict::Regressed);
    assert!(!outcome.is_ok());
}

#[test]
fn execute_diff_from_history_fails_when_the_program_was_never_recorded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let history_path = dir.path().join("history.jsonl");
    let store = JsonlHistoryStore;
    store
        .append(&history_path, &record("prog", 1_000, None))
        .expect("append");

    let current_path = dir.path().join("current.json");
    let reports = FileReportStore;
    reports
        .save(&current_path, &report("unknown", 1_000, None))
        .expect("save current");

    let err = execute_diff_from_history(
        &reports,
        &store,
        &history_path,
        &current_path,
        &DiffThresholds::new(5.0),
    )
    .expect_err("no baseline for `unknown`");
    assert!(matches!(err, BenchError::BaselineUnavailable { .. }));
}

#[test]
fn history_and_report_stores_stay_independent() {
    // The history seam wraps reports; it must not leak into the report
    // store's contract (a saved history file is not a report file).
    let dir = tempfile::tempdir().expect("tempdir");
    let history_path = dir.path().join("history.jsonl");
    JsonlHistoryStore
        .append(&history_path, &record("prog", 1_000, None))
        .expect("append");
    assert!(
        FileReportStore.load(&history_path).is_err(),
        "a JSONL history is not a glyim.bench/1 document"
    );
}

#[test]
fn history_paths_are_just_paths() {
    // The store is path-agnostic: the CI layout (one file per benchmark
    // under perf/history/) is a caller decision, not a store rule.
    let dir = tempfile::tempdir().expect("tempdir");
    let nested = dir.path().join("perf").join("history").join("fib.jsonl");
    std::fs::create_dir_all(nested.parent().expect("parent")).expect("mkdir");
    JsonlHistoryStore
        .append(&nested, &record("fib", 1_000, None))
        .expect("append creates no directories");
    assert_eq!(
        JsonlHistoryStore.load_all(&nested).expect("loaded").len(),
        1
    );
}

#[test]
fn pathbuf_helpers_exist_for_callers() {
    // The CLI derives history filenames from program names; the helper is
    // deterministic and filesystem-safe.
    let slug = crate::history::program_slug("glyim --emit=obj fib.g");
    assert_eq!(slug, "glyim-emit-obj-fib-g");
    assert_eq!(crate::history::program_slug("a  b"), "a-b");
    let _ = PathBuf::from(slug);
}
