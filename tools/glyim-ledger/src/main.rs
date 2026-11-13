//! CLI for the Readiness Ledger (plan M1.1).
//!
//! `glyim-ledger check [ROOT]` is the registry gate CI runs: it loads the
//! ledger under `ROOT`, validates it, cross-checks the M1.3 inventory
//! extractors (plan §3.5), prints every diagnostic sorted, and exits 1 if
//! any error-severity diagnostic exists. `glyim-ledger gaps [ROOT]`
//! renders the M1.7 gap-registry witness report (witness debt is printed,
//! not failed). `glyim-ledger next [ROOT] [--lane L] [--n N]` prints the
//! plan §3.9 deterministic work queue as the `glyim.ledger-next/1` JSON
//! contract `glyim-pilot` consumes. `glyim-ledger schema [OUT]` exports
//! the JSON-Schema document for both registry entry types.
//! `glyim-ledger inventory [ROOT]` reports the source-derived inventory
//! and its claim/ignore status without full registry validation.
//! `glyim-ledger compute [ROOT] [--evidence PATH] [--format text|json]
//! [--snapshot PATH]` runs the plan M1.8 level-computation engine and
//! prints per-feature readiness levels computed from evidence and
//! `ledger/policy.toml`; `--snapshot PATH` also persists the
//! `glyim.ledger-snapshot/1` document of the run. `glyim-ledger status
//! [ROOT] [--evidence PATH] [--format md|json]` renders the computed
//! status as the marker-delimited Markdown block (plan M1.9 first slice)
//! or the `glyim.status/1` JSON contract. `glyim-ledger diff <BASE>
//! <CURRENT> [ROOT] [--format text|json]` is the plan M1.10
//! readiness-regression gate: it compares two snapshot documents and
//! fails on any computed-level drop that no unexpired waiver in
//! `ledger/waivers.toml` covers (constitution C9).

use std::process::ExitCode;

use glyim_ledger::{
    Command, LedgerCliError, SnapshotStore, format_outcome, parse_args, write_schema,
};

/// Print a typed CLI error with its `miette` help line (when one exists).
fn report(err: &LedgerCliError) {
    eprintln!("{err}");
    if let Some(help) = miette::Diagnostic::help(err) {
        eprintln!("  help: {help}");
    }
}

/// Serialize the built-in schema document, routing any failure through the
/// typed [`LedgerCliError`] seam so every failure path carries the same
/// `help:` line and exit contract (the former stdout branch reported an
/// ad-hoc `eprintln!` instead).
fn schema_stdout() -> Result<String, LedgerCliError> {
    serde_json::to_string_pretty(&ledger_schema::registry_schema())
        .map(|text| text + "\n")
        .map_err(|err| LedgerCliError::SchemaSerialize {
            detail: err.to_string(),
        })
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
        Command::Next { root, lane, limit } => {
            let outcome = glyim_ledger::run_next(&root);
            if outcome.is_ok() {
                let selected = glyim_ledger::filter(&outcome, lane, limit);
                print!("{}", glyim_ledger::format_next_json(&selected));
                ExitCode::SUCCESS
            } else {
                // Registry diagnostics are prose, not queue items: report
                // them through the text channel and fail like `check`.
                for diagnostic in &outcome.diagnostics {
                    eprintln!("{diagnostic}");
                }
                ExitCode::FAILURE
            }
        }
        Command::Explain { id, root } => {
            let outcome = glyim_ledger::run_explain(&root, &id);
            print!("{}", glyim_ledger::format_explain_report(&outcome));
            // A registry load failure fails like `check`; an unknown ID
            // prints the "no registry entry" report and also fails, so a
            // typo'd ID never exits green in a scripted pipeline.
            if outcome.is_ok() && outcome.entry.is_some() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Compute {
            root,
            evidence,
            format,
            snapshot,
        } => {
            let outcome = glyim_ledger::run_compute(&root, evidence.as_deref());
            // Persist the readiness snapshot when requested (`diff`'s input
            // face). A failed write is a CLI error, not a diagnostic: the
            // operator asked for a document that did not land.
            if let Some(snapshot_path) = snapshot {
                let snapshot = glyim_ledger::ComputeSnapshot::from_outcome(&outcome);
                if let Err(diagnostic) =
                    glyim_ledger::FileSnapshotStore.save(&snapshot_path, &snapshot)
                {
                    let err = LedgerCliError::SnapshotWrite {
                        path: snapshot_path.display().to_string(),
                        detail: diagnostic.message,
                    };
                    report(&err);
                    return ExitCode::FAILURE;
                }
            }
            match format {
                glyim_ledger::ComputeFormat::Text => {
                    print!("{}", glyim_ledger::format_compute_report(&outcome));
                }
                glyim_ledger::ComputeFormat::Json => {
                    print!("{}", glyim_ledger::format_compute_json(&outcome));
                }
            }
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Status {
            root,
            evidence,
            format,
        } => {
            let outcome = glyim_ledger::run_status(&root, evidence.as_deref());
            match format {
                glyim_ledger::StatusFormat::Markdown => {
                    print!("{}", glyim_ledger::format_status_markdown(&outcome.compute));
                }
                glyim_ledger::StatusFormat::Json => {
                    print!("{}", glyim_ledger::format_status_json(&outcome.compute));
                }
            }
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Diff {
            root,
            baseline,
            current,
            format,
        } => {
            let outcome = glyim_ledger::run_diff(&root, &baseline, &current);
            match format {
                glyim_ledger::DiffFormat::Text => {
                    print!("{}", glyim_ledger::format_diff_text(&outcome));
                }
                glyim_ledger::DiffFormat::Json => {
                    print!("{}", glyim_ledger::format_diff_json(&outcome));
                }
            }
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
            None => match schema_stdout() {
                Ok(text) => {
                    print!("{text}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            },
        },
    }
}
