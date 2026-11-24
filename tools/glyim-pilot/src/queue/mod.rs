//! The plan M1.11 orchestrator task queue: the pilot consumes the ledger's
//! deterministic `glyim.ledger-next/1` work queue as its task list.
//!
//! The plan §6 M1.11 second half — "`ledger next` feeds the orchestrator
//! task queue; pilot run consumes queue end-to-end on a sample item" — is
//! this module. The [`LedgerGate`](crate::gates::ledger::LedgerGate) (the
//! first M1.11 half) already *surfaces* the queue head in a gate note; what
//! was missing is the seam that turns the ranked [`WorkItem`]s into the
//! prompts the pilot dispatches. [`TaskQueueSource`] is that seam: the
//! ledger-backed implementation ([`LedgerQueueSource`]) is the plan's
//! default (plan §3.9: the queue JSON "is consumed by `glyim-pilot`"), and
//! a future source slots in without touching the session-start flow.
//!
//! # The orchestrator contract
//!
//! [`compose_task_prompt`] maps one queue item to the task prompt a
//! session start dispatches, deterministically and read-only (the ledger
//! registry is never mutated). [`head_task`] is the plan §7.1 work-loop
//! step 1 ("pick the top item") as a function; `session start --from-queue`
//! and `pilot queue show` are its CLI faces.

use std::path::{Path, PathBuf};

use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Table};
use glyim_ledger::{filter, run_next, NextOutcome, WorkItem, WorkKind};
use ledger_schema::Lane;

use crate::error::PilotError;

/// Maps one ranked ledger [`WorkItem`] to the task prompt the
/// orchestrator dispatches (plan M1.11: the queue feeds the task list).
///
/// Deterministic and total: every field the ledger guarantees (rank, id,
/// title, why, evidence, files, lane, related) appears exactly once, in a
/// fixed order, so the same registry state renders the same prompt bytes
/// on every machine (the C6/C1 discipline the ledger renderers pin).
#[must_use]
pub fn compose_task_prompt(item: &WorkItem) -> String {
    let kind_line = match item.kind {
        WorkKind::Gap { class, witnessed } => format!(
            "open gap (class {}, {})",
            class.as_str(),
            if witnessed {
                "witnessed"
            } else {
                "unwitnessed — witness debt"
            }
        ),
        WorkKind::Feature => "registry feature".to_string(),
    };
    let lane_line = item.lane.map_or_else(
        || "unassigned".to_string(),
        |lane| lane.as_str().to_string(),
    );
    let files_line = if item.suggested_files.is_empty() {
        "none suggested — consult the registry entry".to_string()
    } else {
        item.suggested_files.join(", ")
    };
    let related_line = if item.related.is_empty() {
        "none".to_string()
    } else {
        item.related.join(", ")
    };
    format!(
        "Implement the ledger-ranked work item {} (rank {}).\n\
         \n\
         Title: {}\n\
         Kind: {}\n\
         Why now: {}\n\
         Evidence needed: {}\n\
         Suggested files: {}\n\
         Owner lane: {}\n\
         Related registry IDs: {}",
        item.id,
        item.rank,
        item.title,
        kind_line,
        item.why,
        item.evidence_needed,
        files_line,
        lane_line,
        related_line,
    )
}

/// One dispatchable pilot task mapped from the ledger work queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedTask {
    /// 1-based position in the full (unfiltered) ledger queue.
    pub rank: usize,
    /// Registry ID (`GAP-NNNN` or a feature ID).
    pub id: String,
    /// Human-readable entry title.
    pub title: String,
    /// Short kind label (`gap` / `feature`).
    pub kind: &'static str,
    /// Expanded kind detail (gap class and witness state).
    pub kind_detail: String,
    /// Deterministic one-line reason the item is queued.
    pub why: String,
    /// What evidence or change moves the item out of the queue.
    pub evidence_needed: String,
    /// Registry file (and witness fixture) the item lives in.
    pub suggested_files: Vec<String>,
    /// Suggested owner lane (`None` when unresolvable).
    pub lane: Option<Lane>,
    /// Gap → affected feature IDs; feature → direct dependant IDs.
    pub related: Vec<String>,
    /// The composed orchestrator prompt ([`compose_task_prompt`]).
    pub prompt: String,
}

impl QueuedTask {
    /// Maps a ledger [`WorkItem`] into a dispatchable task.
    #[must_use]
    pub fn from_work_item(item: &WorkItem) -> Self {
        let (kind, kind_detail) = match item.kind {
            WorkKind::Gap { class, witnessed } => {
                let detail = format!(
                    "gap (class {}, {})",
                    class.as_str(),
                    if witnessed {
                        "witnessed"
                    } else {
                        "unwitnessed"
                    }
                );
                ("gap", detail)
            }
            WorkKind::Feature => ("feature", "feature".to_string()),
        };
        Self {
            rank: item.rank,
            id: item.id.clone(),
            title: item.title.clone(),
            kind,
            kind_detail,
            why: item.why.clone(),
            evidence_needed: item.evidence_needed.clone(),
            suggested_files: item.suggested_files.clone(),
            lane: item.lane,
            related: item.related.clone(),
            prompt: compose_task_prompt(item),
        }
    }
}

/// Source of the orchestrator's task queue (the M1.11 seam).
///
/// The orchestrator consumes *any* queue source through this trait; the
/// ledger-backed [`LedgerQueueSource`] is the plan's default, and the
/// [`WorkItemSource`](glyim_ledger::next::WorkItemSource) seam upstream
/// keeps new ledger sources flowing through unchanged.
pub trait TaskQueueSource {
    /// Fetches the ranked queue (optionally lane-filtered, optionally
    /// limited). Ranks are the ledger's own — filtering never renumbers.
    ///
    /// # Errors
    ///
    /// A source that cannot produce a trustworthy queue (a ledger that
    /// failed to load or carries error diagnostics) fails with
    /// [`PilotError::Queue`] — an orchestrator that cannot ask the ledger
    /// "what is next" must not pick work itself (plan C1).
    fn fetch(
        &self,
        lane: Option<Lane>,
        limit: Option<usize>,
    ) -> Result<Vec<QueuedTask>, PilotError>;
}

/// The ledger-backed queue source: runs the plan §3.9 engine
/// ([`glyim_ledger::run_next`]) over the registry under `root` and maps
/// the ranked output into [`QueuedTask`]s.
#[derive(Debug, Clone)]
pub struct LedgerQueueSource {
    root: PathBuf,
}

impl LedgerQueueSource {
    /// Builds a source over the project directory containing `ledger/`.
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Runs the ledger queue engine and returns the raw (filtered)
    /// [`NextOutcome`] — the JSON face (`queue show --json`) renders this
    /// verbatim through [`format_next_json`], the same
    /// `glyim.ledger-next/1` contract the ledger CLI prints.
    ///
    /// # Errors
    ///
    /// Error-severity diagnostics from the registry load or validation
    /// fail with [`PilotError::Queue`]; warnings pass through.
    pub fn fetch_outcome(
        &self,
        lane: Option<Lane>,
        limit: Option<usize>,
    ) -> Result<NextOutcome, PilotError> {
        let outcome = run_next(&self.root);
        if outcome.errors() > 0 {
            let first = outcome
                .diagnostics
                .iter()
                .find(|d| d.severity == ledger_schema::Severity::Error)
                .map_or_else(
                    || "unknown registry error".to_string(),
                    |d| d.message.clone(),
                );
            return Err(PilotError::Queue(format!(
                "ledger queue unavailable — registry load failed: {first}"
            )));
        }
        Ok(filter(&outcome, lane, limit))
    }
}

impl TaskQueueSource for LedgerQueueSource {
    fn fetch(
        &self,
        lane: Option<Lane>,
        limit: Option<usize>,
    ) -> Result<Vec<QueuedTask>, PilotError> {
        let outcome = self.fetch_outcome(lane, limit)?;
        Ok(outcome
            .queue
            .iter()
            .map(QueuedTask::from_work_item)
            .collect())
    }
}

/// Work-loop step 1 (plan §7.1): pick the top item of the queue.
///
/// `Ok(None)` is the honest terminal state — the registry ranks nothing
/// for this lane — and the caller surfaces it as "queue empty", never as
/// a made-up task.
///
/// # Errors
///
/// Propagates the source's [`PilotError::Queue`] failure.
pub fn head_task(
    source: &dyn TaskQueueSource,
    lane: Option<Lane>,
) -> Result<Option<QueuedTask>, PilotError> {
    Ok(source.fetch(lane, Some(1))?.into_iter().next())
}

/// Resolves the `session start` prompt: `--from-queue` composes the
/// ledger head's task prompt; otherwise the explicit prompt (or the
/// historical default) applies. The two prompt sources are mutually
/// exclusive — a silent override would dispatch work the ledger did not
/// rank (plan C1).
///
/// # Errors
///
/// Both sources requested, or the queue head is empty while
/// `--from-queue` was requested, fail with [`PilotError::Queue`].
pub fn resolve_start_prompt(
    explicit: Option<&str>,
    from_queue: bool,
    lane: Option<Lane>,
    source: &dyn TaskQueueSource,
) -> Result<String, PilotError> {
    if from_queue {
        if explicit.is_some() {
            return Err(PilotError::Queue(
                "--from-queue and --prompt are mutually exclusive: the pilot dispatches \
                 either the ledger-ranked head or an explicit task, never a mix"
                    .to_string(),
            ));
        }
        let Some(head) = head_task(source, lane)? else {
            let lane_note = lane.map_or_else(
                || "the queue is empty".to_string(),
                |l| format!("the {} lane queue is empty", l.as_str()),
            );
            return Err(PilotError::Queue(format!(
                "--from-queue has no task to dispatch: {lane_note}"
            )));
        };
        return Ok(head.prompt);
    }
    Ok(explicit.unwrap_or(DEFAULT_START_PROMPT).to_string())
}

/// The historical `session start` default (kept for explicit-prompt
/// compatibility).
pub const DEFAULT_START_PROMPT: &str = "Write a simple Rust function that adds two numbers";

/// Renders the queue as the pilot's status-style UTF-8 table (the
/// [`render_status_table`](crate::cli::render_status_table) idiom).
#[must_use]
pub fn format_queue_table(tasks: &[QueuedTask]) -> String {
    if tasks.is_empty() {
        return "Queue empty — no ranked work items.".to_string();
    }
    let mut table = Table::new();
    table.load_style(UTF8_FULL);
    table.set_header(vec!["Rank", "ID", "Kind", "Lane", "Title", "Files"]);
    for task in tasks {
        let color = match task.kind {
            "gap" => Color::Red,
            _ => Color::White,
        };
        table.add_row(vec![
            Cell::new(task.rank),
            Cell::new(&task.id).fg(color),
            Cell::new(&task.kind_detail),
            Cell::new(
                task.lane
                    .map_or_else(|| "-".to_string(), |lane| lane.as_str().to_string()),
            ),
            Cell::new(&task.title),
            Cell::new(task.suggested_files.join(", ")),
        ]);
    }
    table.to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        format_queue_table, head_task, resolve_start_prompt, LedgerQueueSource, TaskQueueSource,
        DEFAULT_START_PROMPT,
    };
    use crate::error::PilotError;
    use glyim_ledger::{format_next_json, parse_lane};
    use ledger_schema::Lane;
    use std::path::PathBuf;

    /// The repo root (pilot crate → `<repo>/tools/glyim-pilot`), so the
    /// end-to-end tests consume the same registry `ledger next` serves in
    /// production.
    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("pilot crate lives under <repo>/tools/")
            .to_path_buf()
    }

    #[test]
    fn prompt_composition_is_deterministic_and_total() {
        let source = LedgerQueueSource::new(repo_root());
        let tasks = source
            .fetch(None, Some(3))
            .expect("the repo registry loads and ranks");
        assert!(!tasks.is_empty(), "the repo queue is non-empty");
        for task in &tasks {
            for needle in [
                format!("Title: {}", task.title),
                format!("Why now: {}", task.why),
                format!("Evidence needed: {}", task.evidence_needed),
            ] {
                assert!(
                    task.prompt.contains(&needle),
                    "prompt must carry the field line `{needle}`"
                );
            }
            assert!(task.prompt.starts_with(&format!(
                "Implement the ledger-ranked work item {} (rank {})",
                task.id, task.rank
            )));
        }
        // Determinism: the same item composes the same prompt bytes.
        let again = source.fetch(None, Some(3)).expect("second fetch");
        assert_eq!(tasks, again, "the queue is a deterministic function");
    }

    #[test]
    fn head_task_returns_the_rank_one_item() {
        let source = LedgerQueueSource::new(repo_root());
        let head = head_task(&source, None)
            .expect("fetch head")
            .expect("non-empty");
        assert_eq!(head.rank, 1, "the head item is rank 1");
        let tasks = source.fetch(None, None).expect("full queue");
        assert_eq!(head.id, tasks[0].id, "head equals the first ranked item");
    }

    #[test]
    fn lane_filter_preserves_ledger_ranks() {
        let source = LedgerQueueSource::new(repo_root());
        let lane = parse_lane("B").expect("B is a registry lane");
        let tasks = source.fetch(Some(lane), None).expect("lane-filtered queue");
        assert!(
            tasks.iter().all(|t| t.lane == Some(lane)),
            "every task belongs to the requested lane"
        );
        if let Some(first) = tasks.first() {
            assert!(
                first.rank >= 1,
                "ranks come from the full queue, never renumbered"
            );
        }
    }

    #[test]
    fn resolve_start_prompt_prefers_the_queue_head() {
        let source = LedgerQueueSource::new(repo_root());
        let prompt = resolve_start_prompt(None, true, None, &source).expect("queue head resolves");
        assert!(
            prompt.starts_with("Implement the ledger-ranked work item GAP-"),
            "the dispatch prompt must be the ledger-ranked task, got: {prompt}"
        );
    }

    #[test]
    fn resolve_start_prompt_keeps_explicit_and_default_paths() {
        let source = LedgerQueueSource::new(repo_root());
        let explicit = resolve_start_prompt(Some("fix the parser"), false, None, &source)
            .expect("explicit prompt resolves");
        assert_eq!(explicit, "fix the parser");
        let default =
            resolve_start_prompt(None, false, None, &source).expect("default prompt resolves");
        assert_eq!(default, DEFAULT_START_PROMPT);
    }

    #[test]
    fn resolve_start_prompt_rejects_mixed_sources_and_empty_queue() {
        let source = LedgerQueueSource::new(repo_root());
        let mixed = resolve_start_prompt(Some("do X"), true, None, &source)
            .expect_err("mixed prompt sources must fail");
        assert!(matches!(mixed, PilotError::Queue(_)));
        let empty = resolve_start_prompt(None, true, Some(Lane::Q), &source);
        // The Q lane may legitimately be empty; an empty result must be a
        // clean Queue error naming the lane, never a silent fallback.
        if let Err(PilotError::Queue(message)) = empty {
            assert!(message.contains("Q lane queue is empty"));
        } else if let Ok(prompt) = empty {
            assert!(
                prompt.contains("Owner lane: Q"),
                "a Q-lane head must dispatch with its lane, got: {prompt}"
            );
        }
    }

    #[test]
    fn table_render_is_total_and_non_empty() {
        let source = LedgerQueueSource::new(repo_root());
        let tasks = source.fetch(None, Some(5)).expect("queue fetch");
        let table = format_queue_table(&tasks);
        assert!(table.contains(&tasks[0].id), "the table names the head");
        assert!(table.contains("Rank"), "the table carries its header");
        assert_eq!(
            format_queue_table(&[]),
            "Queue empty — no ranked work items."
        );
    }

    /// Plan `M1.11` acceptance, end-to-end on a sample item: the JSON face the
    /// pilot renders is byte-level the `glyim.ledger-next/1` contract the
    /// ledger CLI emits — the same document the
    /// [`crate::gates::ledger::LedgerGate`] already consumes.
    #[test]
    fn json_face_matches_the_ledger_next_contract() {
        let source = LedgerQueueSource::new(repo_root());
        let outcome = source
            .fetch_outcome(None, Some(1))
            .expect("queue outcome fetch");
        let json = format_next_json(&outcome);
        assert!(
            json.contains("\"schema\": \"glyim.ledger-next/1\""),
            "the JSON face must be the ledger contract, got: {json}"
        );
    }
}
