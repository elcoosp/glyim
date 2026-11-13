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

    // The diff/scale/budget/matrix/trend gates are verdicts, not errors:
    // the outcome renders like a report, but a regression (or a
    // superlinear fit, or a breached budget, or an accumulated drift)
    // flips the exit code (the `gate` shape).
    let mut diff_failed = false;
    let mut scale_failed = false;
    let mut budget_failed = false;
    let mut matrix_failed = false;
    let mut trend_failed = false;
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
        } => glyim_bench::cli::execute_diff(max_delta_pct, &baseline, &current, json.as_deref())
            .map(|outcome| {
                diff_failed = !outcome.is_ok();
                print!("{}", outcome.render_text());
            }),
        Command::Scale {
            iterations,
            source,
            sizes,
            max_exponent,
            json,
            command,
        } => glyim_bench::cli::execute_scale(
            iterations,
            source,
            &sizes,
            max_exponent,
            json.as_ref(),
            &command,
        )
        .map(|outcome| {
            scale_failed = !outcome.is_ok();
            print!("{}", outcome.render_text());
        }),
        Command::Budget {
            budgets,
            json,
            reports,
        } => glyim_bench::cli::execute_budget(&budgets, &reports, json.as_deref()).map(|outcome| {
            budget_failed = !outcome.is_ok();
            print!("{}", outcome.render_text());
        }),
        Command::Matrix {
            history,
            max_delta_pct,
            max_geo_delta_pct,
            json,
            reports,
        } => glyim_bench::cli::execute_matrix(
            &history,
            max_delta_pct,
            max_geo_delta_pct,
            &reports,
            json.as_deref(),
        )
        .map(|outcome| {
            matrix_failed = !outcome.is_ok();
            print!("{}", outcome.render_text());
        }),
        Command::History { action } => match action {
            glyim_bench::cli::HistoryAction::Record { history, report } => {
                glyim_bench::cli::execute_history_record(&history, &report).map(
                    |(recorded, len)| {
                        println!(
                            "recorded {} into {} ({} record{})",
                            recorded.program,
                            history.display(),
                            len,
                            if len == 1 { "" } else { "s" },
                        );
                    },
                )
            }
            glyim_bench::cli::HistoryAction::Show { history, program } => {
                glyim_bench::cli::execute_history_show(&history, program.as_deref()).map(
                    |records| {
                        println!("glyim-bench history ({})", history.display());
                        for (index, record) in records.iter().enumerate() {
                            let instructions = record.report.instructions.as_ref().map_or_else(
                                || "-".to_string(),
                                |stats| format!("{:.0}", stats.mean),
                            );
                            println!(
                                "  {:<4} {:<40} wall mean {:>12.0} ns  instructions {:>14}",
                                index, record.report.program, record.report.wall.mean, instructions,
                            );
                        }
                        println!("  {} record(s)", records.len());
                    },
                )
            }
            glyim_bench::cli::HistoryAction::Diff {
                history,
                max_delta_pct,
                json,
                current,
            } => glyim_bench::cli::execute_history_diff(
                &history,
                max_delta_pct,
                &current,
                json.as_deref(),
            )
            .map(|outcome| {
                diff_failed = !outcome.is_ok();
                print!("{}", outcome.render_text());
            }),
            glyim_bench::cli::HistoryAction::Trend {
                history,
                program,
                window,
                max_drift_pct,
                json,
            } => glyim_bench::cli::execute_history_trend(
                &history,
                &program,
                window,
                max_drift_pct,
                json.as_deref(),
            )
            .map(|trend| {
                trend_failed = !trend.is_ok();
                print!("{}", trend.render_text());
            }),
        },
    };

    match result {
        Ok(())
            if !diff_failed
                && !scale_failed
                && !budget_failed
                && !matrix_failed
                && !trend_failed =>
        {
            ExitCode::SUCCESS
        }
        Ok(()) => ExitCode::FAILURE,
        Err(err) => {
            eprint!("{:?}", miette::Report::new(err));
            ExitCode::FAILURE
        }
    }
}
