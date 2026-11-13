//! Budget-gate bench (plan M2.6).
//!
//! Dependency-free (`harness = false`), mirroring the `diff_engine` /
//! `scaling_engine` benches: measures the pure budget check plus both
//! renderers over a synthetic 8-program report set against an 8-entry
//! budgets file, so CI logs can track the gate cost as the benchmark
//! matrix grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-bench --bench budget_engine
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use glyim_bench::budget::{BENCH_BUDGET_SCHEMA, BENCH_BUDGETS_SCHEMA};
use glyim_bench::budget::{BudgetEntry, BudgetFile, check_budgets};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::{DEFAULT_MAX_COV_PCT, summarize};

const PROGRAMS: usize = 8;
const SAMPLES: usize = 128;

/// One `glyim.bench/1` report over a constant sample set (mean equals the
/// per-iteration wall so the budget boundary is exact).
fn report(program: &str, wall_mean: u64, instructions_mean: u64) -> BenchReport {
    let samples = vec![
        BenchSample {
            wall_ns: wall_mean,
            instructions: Some(instructions_mean),
            cycles: None,
            rss_kb: None,
            alloc_count: None,
            alloc_bytes: None,
        };
        SAMPLES
    ];
    let walls: Vec<u64> = vec![wall_mean; SAMPLES];
    let instructions: Vec<u64> = vec![instructions_mean; SAMPLES];
    BenchReport::new(
        program,
        vec!["--emit=obj".to_string(), "fib.g".to_string()],
        MeasurementSource::PerfStat,
        SAMPLES,
        samples,
        summarize(&instructions),
        summarize(&walls).expect("non-empty"),
        GateOutcome {
            metric: "instructions".to_string(),
            cov_pct: 0.0,
            max_cov_pct: DEFAULT_MAX_COV_PCT,
            passed: true,
        },
    )
}

/// The synthetic fixtures: `PROGRAMS` reports (one deliberately over
/// budget) and a matching budgets file.
fn fixtures() -> (Vec<(PathBuf, BenchReport)>, BudgetFile) {
    let reports: Vec<(PathBuf, BenchReport)> = (0..PROGRAMS)
        .map(|i| {
            let program = format!("bench_{i}");
            // Program 0 exceeds its instruction budget; everything else is
            // exactly at the (inclusive) boundary.
            let over = i == 0;
            (
                PathBuf::from(format!("{program}.json")),
                report(
                    &program,
                    1_000_000 + i as u64 * 1_000,
                    if over { 999_999 } else { 100_000 },
                ),
            )
        })
        .collect();
    let budgets = BudgetFile {
        schema: BENCH_BUDGETS_SCHEMA.to_string(),
        entries: (0..PROGRAMS)
            .map(|i| BudgetEntry {
                program: format!("bench_{i}"),
                max_wall_mean_ns: Some(1_000_000 + i as u64 * 1_000),
                max_instructions_mean: Some(100_000),
            })
            .collect(),
    };
    (reports, budgets)
}

fn main() {
    let (reports, budgets) = fixtures();

    let started = Instant::now();
    let outcome = black_box(check_budgets(black_box(&reports), black_box(&budgets)));
    let check_elapsed = started.elapsed();

    let started = Instant::now();
    let text = black_box(outcome.render_text());
    let text_elapsed = started.elapsed();

    let started = Instant::now();
    let json = black_box(outcome.to_json_pretty().expect("serializes"));
    let json_elapsed = started.elapsed();

    // Contract asserts: the over-budget program is caught, the boundary
    // rows pass, and renders are deterministic (plan C6).
    assert_eq!(outcome.schema, BENCH_BUDGET_SCHEMA);
    assert!(!outcome.is_ok(), "bench_0 exceeded its instruction budget");
    let exceeded = &outcome.checks[0];
    assert_eq!(exceeded.program, "bench_0");
    assert_eq!(exceeded.rows.len(), 2);
    let boundary = &outcome.checks[1];
    assert!(
        boundary
            .rows
            .iter()
            .all(|row| row.verdict == glyim_bench::budget::BudgetVerdict::WithinBudget)
    );
    assert_eq!(outcome.checks.len(), PROGRAMS);
    assert_eq!(text, outcome.render_text());
    assert_eq!(outcome.to_json_pretty().unwrap(), json);

    println!("budget bench: {PROGRAMS} programs x {SAMPLES} samples vs {PROGRAMS} budget rows");
    println!("  check: {check_elapsed:?}");
    println!("  text:  {text_elapsed:?}");
    println!("  json:  {json_elapsed:?}");
}
