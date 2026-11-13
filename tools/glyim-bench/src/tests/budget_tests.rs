//! Budget-gate tests (M2.6 seam): the `glyim.bench-budgets/1` file
//! contract, the absolute-budget engine verdict ladder, the `BudgetStore`
//! seam, the outcome renderers, and the end-to-end `execute_budget` wiring.

use crate::budget::{
    BENCH_BUDGET_SCHEMA, BENCH_BUDGETS_SCHEMA, BudgetEntry, BudgetFile, BudgetStore, BudgetVerdict,
    FileBudgetStore, InMemoryBudgetStore, check_budgets, execute_budget,
};
use crate::diff::ReportStore;
use crate::error::BenchError;
use crate::report::{BenchReport, BenchSample, GateOutcome};
use crate::source::MeasurementSource;
use crate::stats::{DEFAULT_MAX_COV_PCT, summarize};
use std::path::{Path, PathBuf};

/// The canonical `glyim.bench-budgets/1` document used across the tests.
fn budgets_toml() -> &'static str {
    r#"schema = "glyim.bench-budgets/1"

[[budget]]
program = "prog"
max_wall_mean_ns = 1_500
max_instructions_mean = 2_500

[[budget]]
program = "other"
max_wall_mean_ns = 100
"#
}

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

/// One `(path, report)` pair as the engine consumes them.
fn entry(program: &str, mean_ns: u64, instructions: Option<u64>) -> (PathBuf, BenchReport) {
    (
        PathBuf::from(format!("{program}.json")),
        report(program, mean_ns, instructions),
    )
}

// -- budgets file contract -------------------------------------------

#[test]
fn budgets_file_parses_the_toml_contract() {
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid budgets file");
    assert_eq!(file.schema, BENCH_BUDGETS_SCHEMA);
    assert_eq!(file.entries.len(), 2);
    let prog = &file.entries[0];
    assert_eq!(prog.program, "prog");
    assert_eq!(prog.max_wall_mean_ns, Some(1_500));
    assert_eq!(prog.max_instructions_mean, Some(2_500));
    let other = &file.entries[1];
    assert_eq!(other.max_instructions_mean, None);
}

#[test]
fn budgets_file_rejects_unknown_fields() {
    let bad = r#"schema = "glyim.bench-budgets/1"
[[budget]]
program = "prog"
max_wall_mean_ns = 100
sneaky_extra = true
"#;
    let err = BudgetFile::from_toml_str(bad).expect_err("unknown field");
    assert!(matches!(err, BenchError::BudgetsParse { .. }));
    let help = miette::Diagnostic::help(&err).expect("help");
    assert!(help.to_string().contains("glyim.bench-budgets/1"));
}

#[test]
fn budgets_file_rejects_a_wrong_schema_tag() {
    let bad = budgets_toml().replace("glyim.bench-budgets/1", "glyim.bench/1");
    let err = BudgetFile::from_toml_str(&bad).expect_err("schema mismatch");
    assert!(matches!(err, BenchError::BudgetsParse { .. }));
    assert!(err.to_string().contains(BENCH_BUDGETS_SCHEMA));
}

#[test]
fn budgets_file_rejects_duplicate_programs() {
    let bad = r#"schema = "glyim.bench-budgets/1"

[[budget]]
program = "prog"
max_wall_mean_ns = 100

[[budget]]
program = "prog"
max_wall_mean_ns = 200
"#;
    let err = BudgetFile::from_toml_str(bad).expect_err("duplicate program");
    assert!(matches!(err, BenchError::BudgetsParse { .. }));
    assert!(
        err.to_string().contains("prog"),
        "names the duplicated program"
    );
}

#[test]
fn budgets_file_rejects_a_row_without_any_bound() {
    let bad = r#"schema = "glyim.bench-budgets/1"

[[budget]]
program = "prog"
"#;
    let err = BudgetFile::from_toml_str(bad).expect_err("no bound");
    assert!(matches!(err, BenchError::BudgetsParse { .. }));
}

// -- engine verdict ladder -------------------------------------------

#[test]
fn within_budget_metrics_pass() {
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid");
    let outcome = check_budgets(&[entry("prog", 1_000, Some(2_000))], &file);
    assert!(outcome.is_ok());
    let check = &outcome.checks[0];
    assert!(check.budgeted);
    assert_eq!(check.rows.len(), 2, "one row per bound on the entry");
    assert_eq!(check.rows[0].verdict, BudgetVerdict::WithinBudget);
    assert_eq!(check.rows[1].verdict, BudgetVerdict::WithinBudget);
    assert_eq!(check.rows[0].metric, "wall_ns");
    assert_eq!(check.rows[1].metric, "instructions");
}

#[test]
fn budget_boundary_is_inclusive() {
    // Same `<=` convention as the M2.2 stability gate (stats.rs).
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid");
    let outcome = check_budgets(&[entry("prog", 1_500, Some(2_500))], &file);
    assert!(outcome.is_ok(), "actual == budget is within budget");
    assert_eq!(
        outcome.checks[0].rows[0].verdict,
        BudgetVerdict::WithinBudget
    );
}

#[test]
fn exceeded_wall_mean_fails_the_gate() {
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid");
    let outcome = check_budgets(&[entry("prog", 1_501, None)], &file);
    assert!(!outcome.is_ok());
    assert_eq!(outcome.checks[0].rows[0].verdict, BudgetVerdict::Exceeded);
}

#[test]
fn exceeded_instructions_mean_fails_the_gate() {
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid");
    let outcome = check_budgets(&[entry("prog", 1_000, Some(2_501))], &file);
    assert!(!outcome.is_ok());
    let instructions = &outcome.checks[0].rows[1];
    assert_eq!(instructions.verdict, BudgetVerdict::Exceeded);
}

#[test]
fn unmeasured_instruction_budget_fails() {
    // A wall-only report cannot silently satisfy an instruction budget.
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid");
    let outcome = check_budgets(&[entry("prog", 1_000, None)], &file);
    assert!(!outcome.is_ok());
    assert_eq!(outcome.checks[0].rows[1].verdict, BudgetVerdict::Unmeasured);
}

#[test]
fn unbudgeted_program_fails_the_gate() {
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid");
    let outcome = check_budgets(&[entry("unknown", 1, None)], &file);
    assert!(!outcome.is_ok());
    assert!(!outcome.checks[0].budgeted, "no entry covers `unknown`");
    assert!(outcome.checks[0].rows.is_empty());
}

// -- renderers and determinism ---------------------------------------

#[test]
fn rendering_is_byte_deterministic() {
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid");
    let reports = vec![entry("prog", 2_000, Some(9_000)), entry("unknown", 5, None)];
    let first = check_budgets(&reports, &file);
    let second = check_budgets(&reports, &file);
    assert_eq!(first, second);
    assert_eq!(first.render_text(), second.render_text());
    assert_eq!(
        first.to_json_pretty().unwrap(),
        second.to_json_pretty().unwrap()
    );
    assert!(first.render_text().contains("glyim-bench budget"));
    assert!(first.render_text().contains("EXCEEDED"));
}

#[test]
fn outcome_json_round_trips_and_rejects_unknown_fields() {
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid");
    let outcome = check_budgets(&[entry("prog", 2_000, Some(9_000))], &file);
    let json = outcome.to_json_pretty().expect("serialises");
    assert!(json.contains(BENCH_BUDGET_SCHEMA), "schema tag in JSON");
    let back: crate::budget::BudgetOutcome = serde_json::from_str(&json).expect("deserialises");
    assert_eq!(back, outcome);
    let sneaky = json.replace(
        &format!("\"schema\": \"{BENCH_BUDGET_SCHEMA}\""),
        &format!("\"schema\": \"{BENCH_BUDGET_SCHEMA}\", \"sneaky\": true"),
    );
    assert!(serde_json::from_str::<crate::budget::BudgetOutcome>(&sneaky).is_err());
}

// -- the BudgetStore seam --------------------------------------------

#[test]
fn in_memory_budget_store_round_trips() {
    let file = BudgetFile::from_toml_str(budgets_toml()).expect("valid");
    let store = InMemoryBudgetStore::new();
    store
        .save(Path::new("mem/budgets.toml"), &file)
        .expect("in-memory save");
    let loaded = store.load(Path::new("mem/budgets.toml")).expect("loaded");
    assert_eq!(loaded, file);
    assert!(store.load(Path::new("mem/absent.toml")).is_err());
}

#[test]
fn file_budget_store_round_trips_and_reports_missing_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("budgets.toml");
    std::fs::write(&path, budgets_toml()).expect("write");
    let store = FileBudgetStore;
    let loaded = store.load(&path).expect("loaded");
    assert_eq!(loaded.entries.len(), 2);
    let missing = dir.path().join("absent.toml");
    match store.load(&missing) {
        Err(BenchError::BudgetsRead { path, .. }) => {
            assert_eq!(path, missing.display().to_string());
        }
        other => panic!("expected BudgetsRead, got {other:?}"),
    }
}

// -- execute_budget wiring -------------------------------------------

#[test]
fn execute_budget_gates_reports_end_to_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    let budgets_path = dir.path().join("budgets.toml");
    std::fs::write(&budgets_path, budgets_toml()).expect("write budgets");
    let reports_store = crate::diff::FileReportStore;
    let within = dir.path().join("within.json");
    let exceeded = dir.path().join("exceeded.json");
    reports_store
        .save(&within, &report("prog", 1_000, Some(2_000)))
        .expect("save within");
    reports_store
        .save(&exceeded, &report("other", 999, None))
        .expect("save exceeded");

    let budgets_store = FileBudgetStore;
    let outcome = execute_budget(
        &budgets_store,
        &reports_store,
        &budgets_path,
        &[within.clone(), exceeded.clone()],
    )
    .expect("loads");
    assert!(!outcome.is_ok(), "other's wall mean 999 > 100");
    assert_eq!(outcome.checks[0].program, "prog");
    assert_eq!(outcome.checks[1].program, "other");

    let within_only = execute_budget(
        &budgets_store,
        &reports_store,
        &budgets_path,
        std::slice::from_ref(&within),
    )
    .expect("loads");
    assert!(within_only.is_ok());
}

#[test]
fn execute_budget_fails_when_the_budgets_file_is_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let budgets = dir.path().join("absent.toml");
    let reports_store = crate::diff::FileReportStore;
    let report_path = dir.path().join("r.json");
    reports_store
        .save(&report_path, &report("prog", 1, None))
        .expect("save");
    let result = execute_budget(
        &FileBudgetStore,
        &reports_store,
        &budgets,
        std::slice::from_ref(&report_path),
    );
    assert!(matches!(result, Err(BenchError::BudgetsRead { .. })));
}

#[test]
fn budget_entries_construct_directly_for_fixtures() {
    // The public struct lets tests and examples build budgets without TOML.
    let budget_entry = BudgetEntry {
        program: "prog".to_string(),
        max_wall_mean_ns: Some(1_500),
        max_instructions_mean: None,
    };
    let file = BudgetFile {
        schema: BENCH_BUDGETS_SCHEMA.to_string(),
        entries: vec![budget_entry],
    };
    assert!(check_budgets(&[entry("prog", 1_000, None)], &file).is_ok());
}
