//! CLI surface (`clap` derive), mirroring the `glyim-cli` house pattern of
//! keeping the parser in the library so it stays unit-testable.
//!
//! Seven subcommands:
//!
//! - `glyim-bench run [--iterations N] [--source S] [--json OUT] -- CMD...`
//!   executes the command and prints the report (text to stdout, JSON to
//!   `--json`).
//! - `glyim-bench gate [--max-cov-pct P] REPORT` re-checks a written report
//!   against the stability budget and exits non-zero on failure — the shape
//!   a CI perf job consumes.
//! - `glyim-bench diff [--max-delta-pct P] [--json OUT] BASE CURRENT` diffs
//!   two reports and exits non-zero when a metric regressed beyond the
//!   budget — the M2.5 perf-history gate.
//! - `glyim-bench scale [--iterations N] [--sizes LIST] [--max-exponent E]
//!   [--json OUT] -- CMD...` measures the command at every size (one argv
//!   element must carry the `{size}` placeholder) and fits the log-log
//!   scaling exponent — the M2.4 analyzer; exits non-zero when the fitted
//!   exponent exceeds the budget.
//! - `glyim-bench budget [--budgets PATH] [--json OUT] REPORT...` checks
//!   written reports against the committed absolute budgets (plan M2.6) and
//!   exits non-zero when a metric exceeds its budget, is unmeasured, or the
//!   program has no budget entry.
//! - `glyim-bench history record REPORT --history PATH` appends a report to
//!   a JSONL perf history (plan §6 M2.5 `perf/history/*.jsonl`), `history
//!   show --history PATH` lists its records, and `history diff --history
//!   PATH CURRENT` diffs against the latest recorded baseline.
//! - `glyim-bench matrix [--history PATH] [--max-delta-pct P]
//!   [--max-geo-delta-pct G] [--json OUT] REPORT...` diffs every current
//!   report against the latest recorded baseline of its program and gates
//!   the per-benchmark deltas **and** their geo-mean (the plan §325 "fail
//!   >2% per benchmark or >0.5% geo-mean" criterion) — the aggregate gate
//!   that catches a uniform sub-budget drift no single row trips.
//! - `glyim-bench corpus write [--dir PATH] [--families LIST] [--seeds
//!   LIST] [--size N]` materializes the committed corpora directory (plan
//!   M2.3 remainder: "corpora committed with hashes") — one
//!   `<family>-<seed>-<size>.g` file per program plus the
//!   `glyim.corpus/1` `corpus.json` manifest; `corpus check [--dir PATH]`
//!   re-verifies the committed bytes against the manifest and exits
//!   non-zero on any missing or hand-edited program.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::diff::ReportStore;
use crate::error::BenchError;
use crate::history::HistoryStore;
use crate::report::BenchReport;
use crate::runner::{RunPlan, detect_source, run_iterated};
use crate::scaling::DEFAULT_MAX_SCALING_EXPONENT;
use crate::source::MeasurementSource;
use crate::stats::DEFAULT_MAX_COV_PCT;

/// `glyim-bench` command-line interface.
#[derive(Parser, Debug)]
#[command(
    name = "glyim-bench",
    version,
    about = "M2.2 measurement harness: wall time, instruction counts, child RSS, and the CoV stability gate",
    propagate_version = true
)]
pub struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run COMMAND for N iterations and report samples + stability gate.
    Run {
        /// Number of iterations (the M2.2 gate expects 10).
        #[arg(long, default_value_t = 10)]
        iterations: usize,
        /// Counter source; default auto-detects `perf`, falling back to wall.
        #[arg(long)]
        source: Option<MeasurementSource>,
        /// Also write the report as `glyim.bench/1` JSON to this path.
        #[arg(long)]
        json: Option<PathBuf>,
        /// The measured command, after `--`.
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// Re-check a written report against the stability budget.
    Gate {
        /// Allowed coefficient of variation, in percent.
        #[arg(long, default_value_t = DEFAULT_MAX_COV_PCT)]
        max_cov_pct: f64,
        /// The report file to check.
        report: PathBuf,
    },
    /// Diff a baseline report against a current one (M2.5 perf-history
    /// seam); exits non-zero when a metric regressed beyond the budget.
    Diff {
        /// Allowed upward mean delta, in percent.
        #[arg(long, default_value_t = DEFAULT_MAX_DELTA_PCT)]
        max_delta_pct: f64,
        /// Also write the diff as `glyim.bench-diff/1` JSON to this path.
        #[arg(long)]
        json: Option<PathBuf>,
        /// The baseline `glyim.bench/1` report.
        baseline: PathBuf,
        /// The current report to compare against the baseline.
        current: PathBuf,
    },
    /// Measure COMMAND at every input size and fit the scaling exponent
    /// (M2.4 analyzer); exits non-zero when the exponent exceeds the
    /// budget.
    Scale {
        /// Iterations per size (the M2.2 gate expects 10).
        #[arg(long, default_value_t = 10)]
        iterations: usize,
        /// Counter source; default auto-detects `perf`, falling back to wall.
        #[arg(long)]
        source: Option<MeasurementSource>,
        /// Comma-separated input sizes, e.g. `100,1000,10000`.
        #[arg(long)]
        sizes: String,
        /// Allowed scaling exponent (1.0 = linear).
        #[arg(long, default_value_t = DEFAULT_MAX_SCALING_EXPONENT)]
        max_exponent: f64,
        /// Also write the outcome as `glyim.bench-scaling/1` JSON to this
        /// path.
        #[arg(long)]
        json: Option<PathBuf>,
        /// The measured command, after `--`; one argument must carry the
        /// `{size}` placeholder.
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// Check written reports against the committed absolute budgets
    /// (M2.6 budgets seam); exits non-zero when a metric exceeds its
    /// budget, is unmeasured, or the program has no budget entry.
    Budget {
        /// The budgets file (`glyim.bench-budgets/1` TOML).
        #[arg(long, default_value = "perf/budgets.toml")]
        budgets: PathBuf,
        /// Also write the outcome as `glyim.bench-budget/1` JSON to this
        /// path.
        #[arg(long)]
        json: Option<PathBuf>,
        /// The `glyim.bench/1` reports to check (one per benchmark).
        #[arg(required = true)]
        reports: Vec<PathBuf>,
    },
    /// Record reports into the append-only perf history and select
    /// baselines from it (plan §6 M2.5 `perf/history/*.jsonl`).
    History {
        /// What to do with the history file.
        #[command(subcommand)]
        action: HistoryAction,
    },
    /// Diff a matrix of current reports against the latest recorded
    /// baseline of each program and gate the per-benchmark deltas **and**
    /// their geo-mean (plan §325: "fail >2% per benchmark or >0.5%
    /// geo-mean"); exits non-zero when any row regressed or a geo-mean
    /// exceeded its budget.
    Matrix {
        /// The history JSONL file the per-program baselines are selected
        /// from (latest recorded run per program).
        #[arg(long, default_value = "perf/history.jsonl")]
        history: PathBuf,
        /// Allowed upward mean delta per benchmark, in percent.
        #[arg(long, default_value_t = crate::matrix::DEFAULT_MATRIX_MAX_DELTA_PCT)]
        max_delta_pct: f64,
        /// Allowed upward geo-mean delta across the matrix, in percent.
        #[arg(long, default_value_t = crate::matrix::DEFAULT_MAX_GEO_DELTA_PCT)]
        max_geo_delta_pct: f64,
        /// Also write the outcome as `glyim.bench-matrix/1` JSON to this
        /// path.
        #[arg(long)]
        json: Option<PathBuf>,
        /// The current `glyim.bench/1` reports (one per benchmark; each
        /// program at most once).
        #[arg(required = true)]
        reports: Vec<PathBuf>,
    },
    /// Materialize (`write`) or re-verify (`check`) the committed corpora
    /// directory (plan M2.3 remainder: "corpora committed with hashes").
    Corpus {
        /// What to do with the corpus directory.
        #[command(subcommand)]
        action: CorpusAction,
    },
}

/// The `corpus` subcommand's actions.
#[derive(Subcommand, Debug)]
pub enum CorpusAction {
    /// Generate the plan's programs and commit them through the
    /// [`crate::corpus_dir::CorpusSink`] seam (one `.g` file per program
    /// plus the `glyim.corpus/1` `corpus.json` manifest).
    Write {
        /// The directory to materialize (created on demand).
        #[arg(long, default_value = "perf/corpus")]
        dir: PathBuf,
        /// Comma-separated family names (default: all seven).
        #[arg(long)]
        families: Option<String>,
        /// Comma-separated seeds (default: `0,1,2`).
        #[arg(long)]
        seeds: Option<String>,
        /// The programs' `size` parameter.
        #[arg(long, default_value_t = 64)]
        size: usize,
    },
    /// Re-verify a committed corpora directory: template identities via
    /// the manifest's own contract, then every committed `.g` file's
    /// bytes against its recorded identity; exits non-zero when any
    /// program is missing or drifted.
    Check {
        /// The directory to verify.
        #[arg(long, default_value = "perf/corpus")]
        dir: PathBuf,
    },
}

/// The `history` subcommand's actions.
#[derive(Subcommand, Debug)]
pub enum HistoryAction {
    /// Append a `glyim.bench/1` report to a JSONL history file (creating
    /// it on first append).
    Record {
        /// The history JSONL file to append to; the conventional layout is
        /// `perf/history/<program_slug>.jsonl`.
        #[arg(long)]
        history: PathBuf,
        /// The `glyim.bench/1` report to record.
        report: PathBuf,
    },
    /// List a history file's records, oldest first (file order is append
    /// order).
    Show {
        /// The history JSONL file to list.
        #[arg(long)]
        history: PathBuf,
        /// Only records of this program.
        #[arg(long)]
        program: Option<String>,
    },
    /// Diff a current report against the **latest** recorded baseline for
    /// its program (the plan M2.5 perf-compare over the committed history);
    /// exits non-zero when a metric regressed beyond the budget.
    Diff {
        /// The history JSONL file to select the baseline from.
        #[arg(long)]
        history: PathBuf,
        /// Allowed upward mean delta, in percent.
        #[arg(long, default_value_t = DEFAULT_MAX_DELTA_PCT)]
        max_delta_pct: f64,
        /// Also write the diff as `glyim.bench-diff/1` JSON to this path.
        #[arg(long)]
        json: Option<PathBuf>,
        /// The current report to compare against the history baseline.
        current: PathBuf,
    },
    /// Measure the **total drift** across the last N recorded runs of one
    /// program (the plan M2.5 complement: the accumulated multi-PR
    /// regression that latest-baseline diffs never see); exits non-zero
    /// when the window's drift exceeds the budget.
    Trend {
        /// The history JSONL file to read.
        #[arg(long)]
        history: PathBuf,
        /// The program to trend (its recorded runs form the window).
        #[arg(long)]
        program: String,
        /// How many of the program's recorded runs the trend spans (the
        /// last N in append order).
        #[arg(long, default_value_t = crate::trend::DEFAULT_TREND_WINDOW)]
        window: usize,
        /// Allowed total upward drift over the window, in percent.
        #[arg(long, default_value_t = crate::trend::DEFAULT_MAX_TREND_DRIFT_PCT)]
        max_drift_pct: f64,
        /// Also write the report as `glyim.bench-trend/1` JSON to this
        /// path.
        #[arg(long)]
        json: Option<PathBuf>,
    },
}

/// Default regression budget for `diff`, in percent: a mean wall or
/// instruction delta above this fails the gate. Deliberately looser than
/// the M2.2 stability budget (a stable *noisy* run can still be slower);
/// CI perf jobs pass an explicit `--max-delta-pct`.
pub const DEFAULT_MAX_DELTA_PCT: f64 = 5.0;

/// Parse a report file into a [`BenchReport`].
///
/// # Errors
///
/// [`BenchError::ReportRead`] / [`BenchError::ReportParse`] with `miette`
/// help text for unreadable or non-`glyim.bench/1` files; the parse detail
/// names the offending path so a multi-report gate (`budget`, `matrix`)
/// points at the right file.
pub fn load_report(path: &PathBuf) -> Result<BenchReport, BenchError> {
    let text = std::fs::read_to_string(path).map_err(|err| BenchError::ReportRead {
        path: path.display().to_string(),
        detail: err.to_string(),
    })?;
    serde_json::from_str(&text).map_err(|err| BenchError::ReportParse {
        detail: format!("{}: {err}", path.display()),
    })
}

/// Execute [`Command::Run`]: returns the report (already printed).
///
/// # Errors
///
/// Any runner error, or a report-write failure on the `--json` path.
pub fn execute_run(
    iterations: usize,
    source: Option<MeasurementSource>,
    json: Option<&PathBuf>,
    command: &[String],
) -> Result<BenchReport, BenchError> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| BenchError::InvalidPlan {
            detail: "no command given after `--`; usage: glyim-bench run -- <program> [args..]"
                .to_string(),
        })?;
    let source = source.unwrap_or_else(detect_source);
    let mut plan = RunPlan::new(program)
        .with_iterations(iterations)
        .with_source(source);
    for arg in args {
        plan = plan.push_arg(arg);
    }
    let report = run_iterated(&plan)?;
    if let Some(path) = json {
        let text = report.to_json_pretty()?;
        std::fs::write(path, text).map_err(|err| BenchError::ReportWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
    }
    Ok(report)
}

/// Execute [`Command::Gate`]: recompute the gate from the report's samples.
///
/// # Errors
///
/// Report read/parse failures, or [`BenchError::GateFailed`] when the
/// recomputed coefficient of variation exceeds the budget.
pub fn execute_gate(max_cov_pct: f64, report_path: &PathBuf) -> Result<BenchReport, BenchError> {
    let report = load_report(report_path)?;
    let samples = report.gate_samples();
    let cov_pct = crate::stats::coefficient_of_variation(&samples).ok_or_else(|| {
        BenchError::ReportParse {
            detail: "report has no samples to gate on".to_string(),
        }
    })?;
    if !crate::stats::gate_passes(cov_pct, max_cov_pct) {
        return Err(BenchError::GateFailed {
            metric: report.gate.metric.clone(),
            cov_pct,
            max_cov_pct,
        });
    }
    Ok(report)
}

/// Execute [`Command::Diff`]: load both reports through the M2.5
/// [`ReportStore`](crate::diff::ReportStore) seam, diff, and optionally
/// persist the `glyim.bench-diff/1` outcome on `--json`.
///
/// The caller (main) prints the outcome and maps `!outcome.is_ok()` to a
/// non-zero exit — the shape a CI perf job consumes.
///
/// # Errors
///
/// Report read/parse failures for either side, or
/// [`BenchError::InvalidPlan`] when the two reports measure different
/// programs.
pub fn execute_diff(
    max_delta_pct: f64,
    baseline_path: &Path,
    current_path: &Path,
    json: Option<&Path>,
) -> Result<crate::diff::BenchDiffOutcome, BenchError> {
    let store = crate::diff::FileReportStore;
    let outcome = crate::diff::execute_diff(
        &store,
        baseline_path,
        current_path,
        &crate::diff::DiffThresholds::new(max_delta_pct),
    )?;
    if let Some(path) = json {
        let text = outcome.to_json_pretty()?;
        std::fs::write(path, text).map_err(|err| BenchError::ReportWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
    }
    Ok(outcome)
}

/// Execute [`HistoryAction::Diff`]: select the baseline as the **latest**
/// record for the current report's program from the JSONL history, run the
/// unchanged M2.5 diff engine, and optionally persist the
/// `glyim.bench-diff/1` outcome on `--json` (the plan M2.5 perf-compare
/// shape; the outcome's `baseline_path` names the history file with the
/// `#latest` selector).
///
/// # Errors
///
/// Report/history read/parse failures, or [`BenchError::InvalidPlan`]
/// when no recorded history measures the current report's program.
pub fn execute_history_diff(
    history_path: &Path,
    max_delta_pct: f64,
    current_path: &Path,
    json: Option<&Path>,
) -> Result<crate::diff::BenchDiffOutcome, BenchError> {
    let reports = crate::diff::FileReportStore;
    let outcome = crate::history::execute_diff_from_history(
        &reports,
        &crate::history::JsonlHistoryStore,
        history_path,
        current_path,
        &crate::diff::DiffThresholds::new(max_delta_pct),
    )?;
    if let Some(path) = json {
        let text = outcome.to_json_pretty()?;
        std::fs::write(path, text).map_err(|err| BenchError::ReportWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
    }
    Ok(outcome)
}

/// Execute [`HistoryAction::Record`]: load the report through the M2.5
/// [`ReportStore`](crate::diff::ReportStore) seam and append it to the
/// JSONL history. Returns the recorded report and the history's new length.
///
/// # Errors
///
/// Report read/parse failures, or history append failures.
pub fn execute_history_record(
    history_path: &Path,
    report_path: &Path,
) -> Result<(BenchReport, usize), BenchError> {
    let reports = crate::diff::FileReportStore;
    let report = reports.load(report_path)?;
    let history = crate::history::JsonlHistoryStore;
    history.append(
        history_path,
        &crate::history::HistoryRecord::new(report.clone()),
    )?;
    let len = history.load_all(history_path)?.len();
    tracing::debug!(report = %report_path.display(), history = %history_path.display(), "report recorded");
    Ok((report, len))
}

/// Execute [`HistoryAction::Show`]: list a history file's records (oldest
/// first), optionally filtered to one program.
///
/// # Errors
///
/// History read/parse failures.
pub fn execute_history_show(
    history_path: &Path,
    program: Option<&str>,
) -> Result<Vec<crate::history::HistoryRecord>, BenchError> {
    let history = crate::history::JsonlHistoryStore;
    let records = history.load_all(history_path)?;
    Ok(records
        .into_iter()
        .filter(|record| program.is_none_or(|name| record.report.program == name))
        .collect())
}

/// Execute [`HistoryAction::Trend`]: read the JSONL history through the
/// [`HistoryStore`](crate::history::HistoryStore) seam and measure the
/// total drift across the program's last `window` recorded runs, optionally
/// persisting the `glyim.bench-trend/1` report on `--json`.
///
/// The caller (main) prints the report and maps `!report.is_ok()` to a
/// non-zero exit — the shape a CI drift-ratchet job consumes.
///
/// # Errors
///
/// History read/parse failures, [`BenchError::BaselineUnavailable`] when
/// the program was never recorded, or [`BenchError::TrendPlan`] when the
/// window holds fewer than two runs.
pub fn execute_history_trend(
    history_path: &Path,
    program: &str,
    window: usize,
    max_drift_pct: f64,
    json: Option<&Path>,
) -> Result<crate::trend::TrendReport, BenchError> {
    let history = crate::history::JsonlHistoryStore;
    let trend = crate::trend::execute_trend_from_history(
        &history,
        history_path,
        program,
        &crate::trend::TrendThresholds::new(window, max_drift_pct),
    )?;
    if let Some(path) = json {
        let text = trend.to_json_pretty()?;
        std::fs::write(path, text).map_err(|err| BenchError::ReportWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
    }
    Ok(trend)
}

/// Execute [`CorpusAction::Write`]: materialize the plan's programs and
/// manifest into `dir` through the [`crate::corpus_dir::CorpusSink`] seam.
///
/// # Errors
///
/// Plan parse failures ([`BenchError::CorpusPlan`]) and sink write
/// failures.
pub fn execute_corpus_write(
    dir: &Path,
    families: Option<&str>,
    seeds: Option<&str>,
    size: usize,
) -> Result<crate::corpus::CorpusManifest, BenchError> {
    let plan = crate::corpus_dir::CorpusDirPlan::from_flags(families, seeds, size)?;
    let mut sink = crate::corpus_dir::DirCorpusSink::new(dir);
    let manifest = crate::corpus_dir::write_corpus_dir(&mut sink, &plan)?;
    tracing::debug!(dir = %dir.display(), programs = manifest.programs.len(), "corpus written");
    Ok(manifest)
}

/// Execute [`CorpusAction::Check`]: re-verify the committed bytes of a
/// corpus directory against its `glyim.corpus/1` manifest.
///
/// # Errors
///
/// Manifest read/parse failures ([`BenchError::CorpusDirRead`]) and
/// template-drift failures ([`BenchError::CorpusIdentityMismatch`]);
/// missing/mismatched program files are report content on the returned
/// [`crate::corpus_dir::CorpusDirCheck`] (the caller flips the exit code).
pub fn execute_corpus_check(dir: &Path) -> Result<crate::corpus_dir::CorpusDirCheck, BenchError> {
    crate::corpus_dir::check_corpus_dir(dir)
}

/// Parses the `--sizes` CSV into a sorted, distinct size list.
///
/// # Errors
///
/// [`BenchError::InvalidPlan`] for an empty list, a non-integer element, a
/// zero size, or a duplicate.
pub fn parse_sizes(sizes: &str) -> Result<Vec<u64>, BenchError> {
    let parsed: Vec<u64> = sizes
        .split(',')
        .map(|element| {
            element
                .trim()
                .parse::<u64>()
                .map_err(|_| BenchError::InvalidPlan {
                    detail: format!("`--sizes` element {element:?} is not a positive integer"),
                })
        })
        .collect::<Result<_, _>>()?;
    let mut sorted = parsed;
    sorted.sort_unstable();
    if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(BenchError::InvalidPlan {
            detail: "`--sizes` elements must be distinct".to_string(),
        });
    }
    if sorted.is_empty() {
        return Err(BenchError::InvalidPlan {
            detail: "`--sizes` needs at least one size".to_string(),
        });
    }
    Ok(sorted)
}

/// Execute [`Command::Scale`]: measure the command at every size through
/// the M2.4 [`run_scaling`](crate::scaling::run_scaling) wiring and
/// optionally persist the `glyim.bench-scaling/1` outcome on `--json`.
///
/// The caller (main) prints the outcome and maps `!outcome.is_ok()` to a
/// non-zero exit — the shape a CI perf job consumes.
///
/// # Errors
///
/// Any [`crate::scaling::run_scaling`] error, or a report-write failure on
/// the `--json` path.
pub fn execute_scale(
    iterations: usize,
    source: Option<MeasurementSource>,
    sizes: &str,
    max_exponent: f64,
    json: Option<&PathBuf>,
    command: &[String],
) -> Result<crate::scaling::ScalingOutcome, BenchError> {
    let plan = crate::scaling::ScalingPlan::new(iterations, parse_sizes(sizes)?, command.to_vec())
        .with_max_exponent(max_exponent)
        .with_source(source.unwrap_or_else(detect_source));
    let outcome = crate::scaling::run_scaling(&plan)?;
    if let Some(path) = json {
        let text = outcome.to_json_pretty()?;
        std::fs::write(path, text).map_err(|err| BenchError::ReportWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
    }
    Ok(outcome)
}

/// Execute [`Command::Budget`]: load the budgets through the M2.6
/// [`BudgetStore`](crate::budget::BudgetStore) seam and every report
/// through the M2.5 [`ReportStore`](crate::diff::ReportStore) seam, and
/// optionally persist the `glyim.bench-budget/1` outcome on `--json`.
///
/// The caller (main) prints the outcome and maps `!outcome.is_ok()` to a
/// non-zero exit — the shape the CI budget gate consumes.
///
/// # Errors
///
/// Budgets-file read/parse failures, report read/parse failures, or a
/// report-write failure on the `--json` path.
pub fn execute_budget(
    budgets_path: &Path,
    report_paths: &[PathBuf],
    json: Option<&Path>,
) -> Result<crate::budget::BudgetOutcome, BenchError> {
    let budgets_store = crate::budget::FileBudgetStore;
    let report_store = crate::diff::FileReportStore;
    let outcome =
        crate::budget::execute_budget(&budgets_store, &report_store, budgets_path, report_paths)?;
    if let Some(path) = json {
        let text = outcome.to_json_pretty()?;
        std::fs::write(path, text).map_err(|err| BenchError::ReportWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
    }
    Ok(outcome)
}

/// Execute [`Command::Matrix`]: load every current report through the M2.5
/// [`ReportStore`](crate::diff::ReportStore) seam, select each program's
/// baseline from the JSONL history through the M2.5-remainder
/// [`BaselineSource`](crate::matrix::BaselineSource) seam, and optionally
/// persist the `glyim.bench-matrix/1` outcome on `--json`.
///
/// The caller (main) prints the outcome and maps `!outcome.is_ok()` to a
/// non-zero exit — the shape the CI perf-compare job consumes.
///
/// # Errors
///
/// Report/history read/parse failures,
/// [`BenchError::BaselineUnavailable`] when a program was never recorded,
/// or a report-write failure on the `--json` path.
pub fn execute_matrix(
    history_path: &Path,
    max_delta_pct: f64,
    max_geo_delta_pct: f64,
    report_paths: &[PathBuf],
    json: Option<&Path>,
) -> Result<crate::matrix::MatrixOutcome, BenchError> {
    let report_store = crate::diff::FileReportStore;
    let history = crate::history::JsonlHistoryStore;
    let baselines = crate::matrix::HistoryBaselineSource::new(&history, history_path);
    let outcome = crate::matrix::execute_diff_matrix(
        &report_store,
        &baselines,
        report_paths,
        &crate::matrix::MatrixThresholds::new(max_delta_pct, max_geo_delta_pct),
    )?;
    if let Some(path) = json {
        let text = outcome.to_json_pretty()?;
        std::fs::write(path, text).map_err(|err| BenchError::ReportWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use crate::source::MeasurementSource;
    use clap::Parser as _;

    fn parse(args: &[&str]) -> Command {
        let cli = Cli::parse_from(args);
        cli.command
    }

    #[test]
    fn run_defaults_match_the_m2_2_gate_shape() {
        let Command::Run {
            iterations,
            source,
            json,
            command,
        } = parse(&["glyim-bench", "run", "--", "true"])
        else {
            panic!("run subcommand expected");
        };
        assert_eq!(iterations, 10);
        assert!(source.is_none(), "auto-detect by default");
        assert!(json.is_none());
        assert_eq!(command, vec!["true".to_string()]);
    }

    #[test]
    fn run_after_double_dash_keeps_flags() {
        let Command::Run { command, .. } =
            parse(&["glyim-bench", "run", "--", "glyim", "--emit=obj", "fib.g"])
        else {
            panic!("run subcommand expected");
        };
        // `--emit=obj` must land in the child's args, not be consumed by the
        // bench CLI (`last = true`).
        assert_eq!(command.len(), 3);
        assert_eq!(command[1], "--emit=obj");
    }

    #[test]
    fn run_accepts_explicit_source() {
        let Command::Run { source, .. } =
            parse(&["glyim-bench", "run", "--source", "callgrind", "--", "true"])
        else {
            panic!("run subcommand expected");
        };
        assert_eq!(source, Some(MeasurementSource::Callgrind));
    }

    #[test]
    fn gate_parses_report_positional() {
        let Command::Gate {
            max_cov_pct,
            report,
        } = parse(&["glyim-bench", "gate", "perf/report.json"])
        else {
            panic!("gate subcommand expected");
        };
        assert_eq!(max_cov_pct, 0.1);
        assert_eq!(report.display().to_string(), "perf/report.json");
    }

    #[test]
    fn scale_defaults_match_the_m2_4_analyzer_shape() {
        let Command::Scale {
            iterations,
            source,
            sizes,
            max_exponent,
            json,
            command,
        } = parse(&[
            "glyim-bench",
            "scale",
            "--sizes",
            "100,1000",
            "--",
            "echo",
            "{size}",
        ])
        else {
            panic!("scale subcommand expected");
        };
        assert_eq!(iterations, 10);
        assert!(source.is_none(), "auto-detect by default");
        assert_eq!(sizes, "100,1000");
        assert!((max_exponent - 1.0).abs() < f64::EPSILON, "linear budget");
        assert!(json.is_none());
        assert_eq!(command, vec!["echo".to_string(), "{size}".to_string()]);
    }

    #[test]
    fn parse_sizes_sorts_distinct_sizes_and_rejects_garbage() {
        let sizes = super::parse_sizes("1000, 100, 10000").expect("parses");
        assert_eq!(sizes, [100, 1_000, 10_000], "sorted ascending");
        assert!(super::parse_sizes("").is_err());
        assert!(super::parse_sizes("100,abc").is_err());
        assert!(super::parse_sizes("100,100").is_err(), "duplicates");
    }

    #[test]
    fn budget_defaults_match_the_m2_6_gate_shape() {
        use std::path::PathBuf;
        let Command::Budget {
            budgets,
            json,
            reports,
        } = parse(&["glyim-bench", "budget", "perf/report.json"])
        else {
            panic!("budget subcommand expected");
        };
        assert_eq!(budgets.display().to_string(), "perf/budgets.toml");
        assert!(json.is_none());
        assert_eq!(reports, vec![PathBuf::from("perf/report.json")]);
    }

    #[test]
    fn budget_requires_at_least_one_report() {
        assert!(
            Cli::try_parse_from(["glyim-bench", "budget"]).is_err(),
            "no reports given"
        );
    }

    #[test]
    fn history_record_show_and_diff_parse_their_flags() {
        let Command::History {
            action: super::HistoryAction::Record { history, report },
        } = parse(&[
            "glyim-bench",
            "history",
            "record",
            "--history",
            "perf/history/fib.jsonl",
            "r.json",
        ])
        else {
            panic!("history record expected");
        };
        assert_eq!(history.display().to_string(), "perf/history/fib.jsonl");
        assert_eq!(report.display().to_string(), "r.json");

        let Command::History {
            action: super::HistoryAction::Show { history, program },
        } = parse(&[
            "glyim-bench",
            "history",
            "show",
            "--history",
            "perf/history/fib.jsonl",
            "--program",
            "fib",
        ])
        else {
            panic!("history show expected");
        };
        assert_eq!(history.display().to_string(), "perf/history/fib.jsonl");
        assert_eq!(program.as_deref(), Some("fib"));

        let Command::History {
            action:
                super::HistoryAction::Diff {
                    history,
                    max_delta_pct,
                    current,
                    json,
                },
        } = parse(&[
            "glyim-bench",
            "history",
            "diff",
            "--history",
            "perf/history/fib.jsonl",
            "cur.json",
        ])
        else {
            panic!("history diff expected");
        };
        assert_eq!(history.display().to_string(), "perf/history/fib.jsonl");
        assert_eq!(max_delta_pct, super::DEFAULT_MAX_DELTA_PCT);
        assert_eq!(current.display().to_string(), "cur.json");
        assert!(json.is_none());
    }

    #[test]
    fn diff_keeps_the_explicit_baseline_positional_shape() {
        let Command::Diff {
            max_delta_pct,
            json,
            baseline,
            current,
        } = parse(&["glyim-bench", "diff", "base.json", "cur.json"])
        else {
            panic!("diff expected");
        };
        assert_eq!(max_delta_pct, 5.0);
        assert!(json.is_none());
        assert_eq!(baseline.display().to_string(), "base.json");
        assert_eq!(current.display().to_string(), "cur.json");
    }

    #[test]
    fn matrix_defaults_match_the_plan_budgets() {
        use std::path::PathBuf;
        let Command::Matrix {
            history,
            max_delta_pct,
            max_geo_delta_pct,
            json,
            reports,
        } = parse(&["glyim-bench", "matrix", "a.json", "b.json"])
        else {
            panic!("matrix subcommand expected");
        };
        assert_eq!(history.display().to_string(), "perf/history.jsonl");
        assert_eq!(
            max_delta_pct,
            super::super::matrix::DEFAULT_MATRIX_MAX_DELTA_PCT,
            "the plan's per-benchmark fail criterion"
        );
        assert_eq!(
            max_geo_delta_pct,
            super::super::matrix::DEFAULT_MAX_GEO_DELTA_PCT,
            "the plan's geo-mean fail criterion"
        );
        assert!(json.is_none());
        assert_eq!(
            reports,
            vec![PathBuf::from("a.json"), PathBuf::from("b.json")]
        );
    }

    #[test]
    fn matrix_requires_at_least_one_report_and_parses_budgets() {
        assert!(
            Cli::try_parse_from(["glyim-bench", "matrix"]).is_err(),
            "no reports given"
        );
        let Command::Matrix {
            history,
            max_delta_pct,
            max_geo_delta_pct,
            json,
            ..
        } = parse(&[
            "glyim-bench",
            "matrix",
            "--history",
            "perf/all.jsonl",
            "--max-delta-pct",
            "1.0",
            "--max-geo-delta-pct",
            "0.25",
            "--json",
            "out.json",
            "a.json",
        ])
        else {
            panic!("matrix subcommand expected");
        };
        assert_eq!(history.display().to_string(), "perf/all.jsonl");
        assert_eq!(max_delta_pct, 1.0);
        assert_eq!(max_geo_delta_pct, 0.25);
        assert_eq!(json.unwrap().display().to_string(), "out.json");
    }
}
