//! The `glyim-pilot queue` CLI face (plan M1.11): print the ledger-ranked
//! task queue the orchestrator dispatches from.
//!
//! `queue show` is the human face of [`LedgerQueueSource`]: it fetches the
//! §3.9 queue over the project's registry (lane filter and limit are the
//! ledger's own), renders the pilot's status-style table, and `--json`
//! prints the `glyim.ledger-next/1` contract verbatim — the same document
//! the [`LedgerGate`](crate::gates::ledger::LedgerGate) consumes and the
//! ledger CLI emits, so the three faces cannot disagree about "what is
//! next".

use std::path::Path;

use glyim_ledger::{format_next_json, parse_lane};
use ledger_schema::Lane;

use crate::error::PilotError;
use crate::queue::{format_queue_table, LedgerQueueSource, TaskQueueSource};

#[derive(clap::Subcommand)]
/// `QueueCommands`.
pub enum QueueCommands {
    /// Show the ledger-ranked task queue (plan M1.11: the orchestrator
    /// dispatches from this queue).
    Show {
        /// Lane filter (L/F/T/M/B/Q); the ledger's own rank is preserved.
        #[arg(short = 'l', long)]
        lane: Option<String>,
        /// Maximum number of tasks to print (ranks stay non-renumbered).
        #[arg(short = 'n', long)]
        limit: Option<usize>,
        /// Print the `glyim.ledger-next/1` JSON contract instead of the
        /// table.
        #[arg(long)]
        json: bool,
    },
}

/// Parses the CLI lane spelling into a registry [`Lane`].
///
/// # Errors
///
/// An unknown spelling fails with [`PilotError::Queue`] naming the six
/// valid lanes.
pub fn parse_cli_lane(spelling: &str) -> Result<Lane, PilotError> {
    parse_lane(spelling).ok_or_else(|| {
        PilotError::Queue(format!(
            "unknown lane {spelling:?} — valid lanes are L, F, T, M, B, Q"
        ))
    })
}

/// `handle_queue_command`.
///
/// # Errors
///
/// Propagates [`PilotError::Queue`] from the queue source (a registry
/// load/validation failure) and lane-parse errors.
pub async fn handle_queue_command(
    cmd: QueueCommands,
    project_root: &Path,
) -> Result<(), PilotError> {
    match cmd {
        QueueCommands::Show { lane, limit, json } => {
            let lane = lane.map(|l| parse_cli_lane(&l)).transpose()?;
            let source = LedgerQueueSource::new(project_root);
            if json {
                let outcome = source.fetch_outcome(lane, limit)?;
                print!("{}", format_next_json(&outcome));
            } else {
                let tasks = source.fetch(lane, limit)?;
                println!("{}", format_queue_table(&tasks));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{handle_queue_command, parse_cli_lane, QueueCommands};
    use crate::error::PilotError;
    use ledger_schema::Lane;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("pilot crate lives under <repo>/tools/")
            .to_path_buf()
    }

    #[test]
    fn lane_spelling_parse_covers_all_six_lanes() {
        for (spelling, expected) in [
            ("L", Lane::L),
            ("F", Lane::F),
            ("T", Lane::T),
            ("M", Lane::M),
            ("B", Lane::B),
            ("Q", Lane::Q),
        ] {
            assert_eq!(parse_cli_lane(spelling).expect("valid"), expected);
        }
        let err = parse_cli_lane("X").expect_err("unknown lane fails");
        assert!(matches!(err, PilotError::Queue(_)));
        assert!(err.to_string().contains("L, F, T, M, B, Q"));
    }

    /// End-to-end on a sample item (plan M1.11 DoD): the CLI face fetches
    /// the real queue over this repo's registry and renders it.
    #[tokio::test]
    async fn queue_show_runs_end_to_end_over_the_repo_registry() {
        let cmd = QueueCommands::Show {
            lane: None,
            limit: Some(3),
            json: false,
        };
        handle_queue_command(cmd, &repo_root())
            .await
            .expect("the repo registry loads and ranks");
    }
}
