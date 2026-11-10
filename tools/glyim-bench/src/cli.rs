//! CLI surface (`clap` derive), mirroring the `glyim-cli` house pattern of
//! keeping the parser in the library so it stays unit-testable.
//!
//! Two subcommands:
//!
//! - `glyim-bench run [--iterations N] [--source S] [--json OUT] -- CMD...`
//!   executes the command and prints the report (text to stdout, JSON to
//!   `--json`).
//! - `glyim-bench gate [--max-cov-pct P] REPORT` re-checks a written report
//!   against the stability budget and exits non-zero on failure — the shape
//!   a CI perf job consumes.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::error::BenchError;
use crate::report::BenchReport;
use crate::runner::{RunPlan, detect_source, run_iterated};
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
}

/// Parse a report file into a [`BenchReport`].
///
/// # Errors
///
/// [`BenchError::ReportRead`] / [`BenchError::ReportParse`] with `miette`
/// help text for unreadable or non-`glyim.bench/1` files.
pub fn load_report(path: &PathBuf) -> Result<BenchReport, BenchError> {
    let text = std::fs::read_to_string(path).map_err(|err| BenchError::ReportRead {
        path: path.display().to_string(),
        detail: err.to_string(),
    })?;
    serde_json::from_str(&text).map_err(|err| BenchError::ReportParse {
        detail: err.to_string(),
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
}
