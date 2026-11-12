//! Binary entry point for `glyim-bench` (plan M2.2).
//!
//! Errors render through `miette` (code + help), matching the
//! `glyim-test` / `glyim-cli` seam discipline: the operator sees the remedy,
//! not just the failure.

use std::process::ExitCode;

use clap::Parser as _;

use glyim_bench::cli::{Cli, Command};
use glyim_bench::report::BenchReport;

fn main() -> ExitCode {
    let cli = Cli::parse();

    // The diff gate is a verdict, not an error: the outcome renders like a
    // report, but a regression flips the exit code (the `gate` shape).
    let mut diff_failed = false;
    let result = match cli.command {
        Command::Run {
            iterations,
            source,
            json,
            command,
        } => glyim_bench::cli::execute_run(iterations, source, json.as_ref(), &command).map(
            |report: BenchReport| {
                print!("{}", report.render_text());
            },
        ),
        Command::Gate {
            max_cov_pct,
            report,
        } => glyim_bench::cli::execute_gate(max_cov_pct, &report).map(|checked| {
            println!(
                "gate OK: {} CoV {:.3}% within budget {:.3}% ({})",
                checked.gate.metric,
                checked.gate.cov_pct,
                max_cov_pct,
                report.display(),
            );
        }),
        Command::Diff {
            max_delta_pct,
            json,
            baseline,
            current,
        } => glyim_bench::cli::execute_diff(max_delta_pct, &baseline, &current, json.as_ref()).map(
            |outcome| {
                diff_failed = !outcome.is_ok();
                print!("{}", outcome.render_text());
            },
        ),
    };

    match result {
        Ok(()) if !diff_failed => ExitCode::SUCCESS,
        Ok(()) => ExitCode::FAILURE,
        Err(err) => {
            eprint!("{:?}", miette::Report::new(err));
            ExitCode::FAILURE
        }
    }
}
