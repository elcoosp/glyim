//! Consumer walkthrough: the multi-report diff matrix and the geo-mean
//! aggregate gate (plan §6 M2.5 remainder, §325 "fail >2% per benchmark or
//! >0.5% geo-mean").
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-bench --example matrix_gate
//! ```
//!
//! The example reproduces the regression class the aggregate clause exists
//! for: a change that slows **every** benchmark by exactly +1% passes each
//! per-benchmark budget (+1% ≤ +2%) and is caught only by the matrix gate's
//! geo-mean budget (+1% > +0.5%). Three programs are recorded into a JSONL
//! perf history, the current reports are diffed through
//! [`execute_diff_matrix`] with the [`HistoryBaselineSource`] seam (latest
//! recorded run per program), and the verdict is what a CI perf-compare job
//! maps to a non-zero exit.

use glyim_bench::diff::{FileReportStore, ReportStore};
use glyim_bench::error::BenchError;
use glyim_bench::history::{HistoryRecord, HistoryStore, JsonlHistoryStore};
use glyim_bench::matrix::{MatrixThresholds, execute_diff_matrix};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::{DEFAULT_MAX_COV_PCT, summarize};

/// A report with constant samples so the means are exact.
fn report(program: &str, mean_ns: u64, instructions: u64) -> BenchReport {
    let samples = vec![
        BenchSample {
            wall_ns: mean_ns,
            instructions: Some(instructions),
            cycles: None,
            rss_kb: None,
            alloc_count: None,
            alloc_bytes: None,
        };
        10
    ];
    BenchReport::new(
        program,
        Vec::new(),
        MeasurementSource::PerfStat,
        10,
        samples,
        Some(summarize(&[instructions; 10]).expect("non-empty")),
        summarize(&[mean_ns; 10]).expect("non-empty"),
        GateOutcome {
            metric: "instructions".to_string(),
            cov_pct: 0.0,
            max_cov_pct: DEFAULT_MAX_COV_PCT,
            passed: true,
        },
    )
}

fn main() -> Result<(), BenchError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let history_path = dir.path().join("perf/history.jsonl");
    std::fs::create_dir_all(history_path.parent().expect("parent")).expect("mkdir");

    let programs = [
        ("glyim --emit=obj fib.g", 1_000_000, 2_000_000),
        ("glyim --emit=obj parse.g", 2_000_000, 4_000_000),
        ("glyim --emit=obj lower.g", 4_000_000, 8_000_000),
    ];

    // Commit the baselines on `main`, then write the current reports with
    // a uniform +1% drift on both metrics (the exact uniform-drift shape:
    // every per-benchmark delta is +1%).
    let history = JsonlHistoryStore;
    let reports = FileReportStore;
    let mut current_paths = Vec::new();
    for (index, (program, wall, instructions)) in programs.iter().enumerate() {
        history.append(
            &history_path,
            &HistoryRecord::new(report(program, *wall, *instructions)),
        )?;
        let current = report(
            program,
            wall + wall / 100,
            instructions + instructions / 100,
        );
        let path = dir.path().join(format!("current-{index}.json"));
        reports.save(&path, &current)?;
        current_paths.push(path);
    }

    // The wiring: every current report against the latest recorded run of
    // its program, then both geo-means.
    let outcome = execute_diff_matrix(
        &reports,
        &glyim_bench::matrix::HistoryBaselineSource::new(&history, &history_path),
        &current_paths,
        &MatrixThresholds::new(2.0, 0.5),
    )?;

    print!("{}", outcome.render_text());
    println!("---");
    if outcome.is_ok() {
        println!("gate OK: no row regressed and both geo-means are within budget");
        Ok(())
    } else {
        println!(
            "gate FAILED: the uniform +1% drift stays within every per-benchmark \
             budget (+2%) but the wall geo-mean (+1%) exceeds the +0.5% aggregate \
             budget — this is the regression class the plan's geo-mean clause \
             exists to catch (CI would exit non-zero)"
        );
        Ok(())
    }
}
