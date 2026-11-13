//! Consumer walkthrough: enforce committed absolute budgets (the plan M2.6
//! seam) against persisted `glyim.bench/1` reports.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-bench --example budget_check
//! ```
//!
//! The example is the library counterpart of the CLI's
//! `budget --budgets FILE REPORT...` subcommand: a committed
//! `glyim.bench-budgets/1` TOML document is loaded through
//! [`FileBudgetStore`], two reports are loaded through [`FileReportStore`]
//! (one within its budgets, one over), and [`execute_budget`] produces the
//! `glyim.bench-budget/1` outcome whose gate a CI budget job maps to a
//! non-zero exit. Unlike the M2.5 diff gate (relative to a baseline
//! report), the budget gate is absolute — and a report whose program has
//! no budget entry fails outright.

use glyim_bench::budget::{FileBudgetStore, execute_budget};
use glyim_bench::diff::{FileReportStore, ReportStore};
use glyim_bench::error::BenchError;
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::summarize;

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
            max_cov_pct: glyim_bench::stats::DEFAULT_MAX_COV_PCT,
            passed: true,
        },
    )
}

fn main() -> Result<(), BenchError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let budgets_path = dir.path().join("budgets.toml");
    let within_path = dir.path().join("within.json");
    let over_path = dir.path().join("over.json");

    // The committed budgets document (plan §6 M2.6: `perf/budgets.toml`).
    std::fs::write(
        &budgets_path,
        r#"schema = "glyim.bench-budgets/1"

[[budget]]
program = "glyim --emit=run corpus/wide.g"
max_wall_mean_ns = 2_000_000
max_instructions_mean = 3_000_000

[[budget]]
program = "glyim --emit=run corpus/deep.g"
max_wall_mean_ns = 2_000_000
"#,
    )
    .expect("write budgets");

    let budgets_store = FileBudgetStore;
    let reports_store = FileReportStore;
    reports_store.save(
        &within_path,
        &report("glyim --emit=run corpus/wide.g", 1_900_000, 2_900_000),
    )?;
    reports_store.save(
        &over_path,
        &report("glyim --emit=run corpus/deep.g", 2_400_000, 900_000),
    )?;

    let outcome = execute_budget(
        &budgets_store,
        &reports_store,
        &budgets_path,
        &[within_path.clone(), over_path.clone()],
    )?;

    print!("{}", outcome.render_text());
    println!("---");
    println!("{}", outcome.to_json_pretty()?);
    println!("---");
    if outcome.is_ok() {
        println!("gate OK: every report within its committed budget");
        Ok(())
    } else {
        println!("gate FAILED: a mean exceeded its budget (CI would exit non-zero)");
        Ok(())
    }
}
