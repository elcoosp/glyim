//! CLI for the Readiness Ledger (plan M1.1).
//!
//! `glyim-ledger check [ROOT]` is the registry gate CI runs: it loads the
//! ledger under `ROOT`, validates it, prints every diagnostic sorted, and
//! exits 1 if any error-severity diagnostic exists. `glyim-ledger schema
//! [OUT]` exports the JSON-Schema document for both registry entry types.

use std::process::ExitCode;

use glyim_ledger::{Command, format_outcome, parse_args, write_schema};

fn main() -> ExitCode {
    let command = match parse_args(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(msg) => {
            eprintln!("{msg}");
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
        Command::Schema { out } => match out {
            Some(path) => match write_schema(&path) {
                Ok(summary) => {
                    println!("{summary}");
                    ExitCode::SUCCESS
                }
                Err(msg) => {
                    eprintln!("{msg}");
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
