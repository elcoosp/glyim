//! M1.11 consumer walkthrough: the orchestrator task queue.
//!
//! The plan §6 M1.11 second half — "`ledger next` feeds the orchestrator
//! task queue; pilot run consumes queue end-to-end on a sample item" — as
//! a runnable: fetch the ledger-ranked queue over this repository's
//! registry, show the table the `pilot queue show` face prints, and
//! compose the exact prompt `session start --from-queue` dispatches for
//! the head item.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-pilot --example queue_walkthrough
//! ```
//!
//! An optional first argument overrides the project root (the directory
//! containing `ledger/`). The walkthrough is read-only: it never mutates
//! the registry and never starts a session — it prints what *would* be
//! dispatched, so a reviewer can verify the queue→prompt seam against the
//! same `glyim.ledger-next/1` document `ledger next` emits.

use std::path::PathBuf;

use glyim_pilot::queue::{head_task, LedgerQueueSource, QueuedTask, TaskQueueSource};

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(std::path::Path::parent)
                .expect("pilot crate lives under <repo>/tools/")
                .to_path_buf()
        });

    let source = LedgerQueueSource::new(&root);

    // 1. The ranked queue (the `queue show` table face).
    let tasks: Vec<QueuedTask> = source
        .fetch(None, Some(5))
        .expect("the repository registry loads and ranks");
    println!(
        "Ledger-ranked task queue (top {} of the registry):\n",
        tasks.len()
    );
    println!("{}", render_queue(&tasks));

    // 2. The dispatch prompt for the head item (the
    //    `session start --from-queue` seam).
    let head = head_task(&source, None)
        .expect("queue fetch")
        .expect("the repository queue is non-empty");
    println!("\nWould dispatch as the head task (rank {}):\n", head.rank);
    println!("{}", head.prompt);

    // 3. The JSON face — the `glyim.ledger-next/1` contract, the same
    //    document `ledger next` prints and the LedgerGate consumes.
    let outcome = source
        .fetch_outcome(None, Some(1))
        .expect("queue outcome fetch");
    println!("\nJSON contract face (glyim.ledger-next/1, head item):");
    println!("{}", glyim_ledger::format_next_json(&outcome));
}

/// Renders the queue as a fixed-width ranked listing (the same rows the
/// `queue show` table carries, in plain text an example can print).
fn render_queue(tasks: &[QueuedTask]) -> String {
    tasks
        .iter()
        .map(|task| {
            format!(
                "  {:>4}  {:<10} {:<24} {}",
                task.rank, task.id, task.kind_detail, task.title
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
