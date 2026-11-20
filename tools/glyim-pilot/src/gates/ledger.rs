//! The plan M1.11 pilot–ledger integration: the `LedgerGate`.
//!
//! The readiness ledger owns the truth about repo status (plan C1: status
//! is computed, never typed), so the pilot's done-pipeline must consult it
//! before declaring work finished. This gate composes two ledger faces:
//!
//! 1. `ledger check` — registry consistency: an inconsistent registry
//!    (unknown feature ID, malformed gap block) fails the gate before any
//!    queue is consulted, because a poisoned registry poisons every queue
//!    verdict downstream.
//! 2. `ledger next --n 1` — the plan §7.1 work-loop step 1: the
//!    orchestrator consumes the *deterministic* `glyim.ledger-next/1`
//!    queue instead of picking work itself. The gate surfaces the queue
//!    head (ID + title) in the gate note so the pilot's next task is
//!    exactly what the ledger ranks first — the queue→gate→verdict seam.
//!
//! The verdict function is pure ([`verdict_from_queue`]) and unit-tested
//! against a sample queue item; the end-to-end test drives the gate over
//! this repository's real registry, so the JSON the gate parses is the
//! same document `ledger next` emits in production.

use crate::error::PilotError;
use crate::gates::helpers::run_gate_command;
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// The plan M1.11 ledger gate: registry consistency + deterministic work
/// queue consumption.
pub struct LedgerGate;

impl LedgerGate {
    /// The ledger CLI invocation for one subcommand, run against the
    /// pilot worktree (`cargo run` keeps the gate working from any
    /// checkout without requiring an installed binary).
    fn ledger_command(worktree_dir: &std::path::Path, args: &[&str]) -> Vec<String> {
        let mut argv = vec![
            "cargo".to_string(),
            "run".to_string(),
            "-q".to_string(),
            "-p".to_string(),
            "glyim-ledger".to_string(),
            "--".to_string(),
        ];
        argv.extend(args.iter().map(ToString::to_string));
        let _ = worktree_dir;
        argv
    }
}

/// Pure queue→verdict mapping: given the `ledger check` result and the
/// `glyim.ledger-next/1` document, produce the gate verdict.
///
/// The queue head is surfaced verbatim (ID + title) so the pilot's task
/// list is fed by the ledger's ranking (plan §3.9) instead of the
/// orchestrator's own preference. An unparsable queue document fails the
/// gate: a ledger that cannot answer "what is next" cannot certify that
/// nothing is silently broken.
#[must_use]
pub fn verdict_from_queue(check_passed: bool, queue_json: &str) -> GateResult {
    if !check_passed {
        return GateResult::fail(
            "ledger",
            "ledger check failed — registry or evidence store is inconsistent",
        );
    }
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(queue_json);
    let Ok(document) = parsed else {
        return GateResult::fail(
            "ledger",
            "ledger next did not emit a parsable glyim.ledger-next/1 document",
        );
    };
    let schema_ok = document
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|schema| schema == "glyim.ledger-next/1");
    if !schema_ok {
        return GateResult::fail(
            "ledger",
            "ledger next emitted an unexpected schema (expected glyim.ledger-next/1)",
        );
    }
    let head = document
        .get("items")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| items.first());
    let Some(head) = head else {
        // An empty queue is the honest terminal state: the registry has
        // no ranked work left, so the gate passes with that note.
        return GateResult::pass_with_note("ledger", "queue empty — no ranked work items");
    };
    let id = head
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing id>");
    let title = head
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing title>");
    GateResult::pass_with_note("ledger", format!("queue head (rank 1): {id} — {title}"))
}

#[async_trait]
impl Gate for LedgerGate {
    fn name(&self) -> &'static str {
        "ledger"
    }

    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        // Face 1: registry consistency. A poisoned registry invalidates
        // every downstream queue verdict, so it fails the gate outright.
        let check_argv = LedgerGate::ledger_command(&ctx.worktree_dir, &["check", "."]);
        let program = check_argv[0].clone();
        let rest: Vec<&str> = check_argv[1..].iter().map(String::as_str).collect();
        let check = run_gate_command(
            &program,
            &rest,
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "ledger",
        )
        .await?;
        if !check.status.success() {
            let stderr = crate::gates::helpers::strip_ansi(&String::from_utf8_lossy(&check.stderr));
            return Ok(GateResult::fail_with_details(
                "ledger",
                "ledger check failed — registry or evidence store is inconsistent",
                stderr,
            ));
        }

        // Face 2: the deterministic work queue (plan §7.1 step 1). The
        // gate consumes `ledger next --n 1` and surfaces the head item;
        // the orchestrator picks work from this note, not from its own
        // heuristics.
        let next_argv = LedgerGate::ledger_command(&ctx.worktree_dir, &["next", ".", "--n", "1"]);
        let next_program = next_argv[0].clone();
        let next_rest: Vec<&str> = next_argv[1..].iter().map(String::as_str).collect();
        let next = run_gate_command(
            &next_program,
            &next_rest,
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "ledger",
        )
        .await?;
        let queue_json = String::from_utf8_lossy(&next.stdout).to_string();
        Ok(verdict_from_queue(true, &queue_json))
    }
}

#[cfg(test)]
mod tests {
    use super::{verdict_from_queue, LedgerGate};
    use crate::gates::types::GateContext;
    use std::path::PathBuf;

    /// A sample `glyim.ledger-next/1` document with one ranked queue item
    /// (the shape `ledger next` actually emits; see the runner's renderer).
    const SAMPLE_QUEUE: &str = r#"{
  "schema": "glyim.ledger-next/1",
  "total": 259,
  "count": 1,
  "items": [
    {
      "rank": 1,
      "id": "GAP-0002",
      "title": "Windows SEH cleanup edges are unreachable",
      "kind": { "kind": "gap", "class": "b", "witnessed": false },
      "why": "class-b gap: silent wrong behaviour",
      "evidence_needed": "strict-xfail witness fixture",
      "suggested_files": ["./ledger/gaps/known/0001-known-gaps-seed.toml"],
      "lane": "B",
      "related": ["LANG-UNWIND-CLEANUP"]
    }
  ]
}"#;

    #[test]
    fn verdict_passes_and_surfaces_the_queue_head() {
        let verdict = verdict_from_queue(true, SAMPLE_QUEUE);
        assert!(verdict.passed, "a consistent registry with a queue passes");
        let note = verdict.note.unwrap_or_default();
        assert!(
            note.contains("GAP-0002") && note.contains("Windows SEH"),
            "the gate note must surface the queue head item, got: {note}"
        );
    }

    #[test]
    fn verdict_fails_on_inconsistent_registry_or_foreign_schema() {
        let verdict = verdict_from_queue(false, SAMPLE_QUEUE);
        assert!(!verdict.passed, "an inconsistent registry must fail");
        let verdict = verdict_from_queue(true, r#"{"schema":"other/1","items":[]}"#);
        assert!(
            !verdict.passed,
            "a foreign queue schema must fail, not pass silently"
        );
    }

    /// End-to-end: the gate runs the REAL ledger over THIS repository's
    /// registry and consumes the real queue (queue→gate→verdict). The
    /// repo's queue is non-empty (gap seed data), so the verdict passes
    /// and names the ranked head item.
    #[tokio::test]
    async fn ledger_gate_consumes_the_real_queue_end_to_end() {
        // Repo root from the pilot crate's manifest location
        // (`tools/glyim-pilot` → two parents up); never hardcoded.
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("pilot crate lives under <repo>/tools/")
            .to_path_buf();
        let ctx = GateContext::new(
            repo_root.clone(),
            repo_root,
            "main".to_string(),
            "test".to_string(),
            300,
            Vec::new(),
        );
        let result = LedgerGate.run(&ctx).await.expect("gate runs");
        assert!(
            result.passed,
            "the repo's registry is consistent and its queue is non-empty: {:?}",
            result.details
        );
        let note = result.note.unwrap_or_default();
        assert!(
            note.starts_with("queue head (rank 1): GAP-"),
            "the verdict must name the ledger-ranked head item, got: {note}"
        );
    }
}
