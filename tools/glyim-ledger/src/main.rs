//! CLI for the Readiness Ledger (plan M1.1).
//!
//! `glyim-ledger check [ROOT]` is the registry gate CI runs: it loads the
//! ledger under `ROOT`, validates it, cross-checks the M1.3 inventory
//! extractors (plan §3.5), prints every diagnostic sorted, and exits 1 if
//! any error-severity diagnostic exists. `glyim-ledger gaps [ROOT]`
//! renders the M1.7 gap-registry witness report (witness debt is printed,
//! not failed). `glyim-ledger schema [OUT]` exports the JSON-Schema
//! document for both registry entry types. `glyim-ledger inventory
//! [ROOT]` reports the source-derived inventory and its claim/ignore
//! status without full registry validation.

use std::process::ExitCode;

use glyim_ledger::{Command, LedgerCliError, format_outcome, parse_args, write_schema};

/// Print a typed CLI error with its `miette` help line (when one exists).
fn report(err: &LedgerCliError) {
    eprintln!("{err}");
    if let Some(help) = miette::Diagnostic::help(err) {
        eprintln!("  help: {help}");
    }
}

fn main() -> ExitCode {
    let command = match parse_args(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(err) => {
            report(&err);
            return ExitCode::from(2);
        }
    };

    match command {
        Command::Check { root } => {
            let outcome = glyim_ledger::run_check(&root);
            print!("{}", format_outcome(&outcome));
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Gaps { root } => {
            let outcome = glyim_ledger::run_gaps(&root);
            print!("{}", glyim_ledger::format_gap_report(&outcome));
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Inventory { root } => {
            let outcome = glyim_ledger::run_inventory(&root);
            print!("{}", glyim_ledger::format_inventory(&outcome));
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Schema { out } => match out {
            Some(path) => match write_schema(&path) {
                Ok(summary) => {
                    println!("{summary}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            },
            None => match serde_json::to_string_pretty(&ledger_schema::registry_schema()) {
                Ok(text) => {
                    println!("{text}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("cannot serialize schema: {err}");
                    ExitCode::FAILURE
                }
            },
        },
    }
}
