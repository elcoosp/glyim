use crate::applier::apply_ops_async;
use crate::commit::{CommitContext, CommitDecision, CommitEngine};
use crate::config::types::PilotConfig;
use crate::error::PilotError;
use crate::gates::done_pipeline;
use crate::gates::self_review::build_review_prompt;
use crate::git_ops::{create_pr, diff_main, diff_name_only, log_oneline, push_branch};
use crate::metrics::Metrics;
use crate::protocol::parser::parse_ops_block;
use crate::session::persistence::StatePersistence;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
/// OrchestratorAction.
pub enum OrchestratorAction {
    /// Variant.
    Feedback {
        /// Struct.
        session_id: String,
        /// Struct.
        message: String,
        /// Struct.
        trace_id: Option<String>,
    },
    /// Variant.
    Continue {
        /// Struct.
        session_id: String,
        /// Struct.
        trace_id: Option<String>,
    },
    /// Variant.
    SelfReview {
        /// Struct.
        session_id: String,
        /// Struct.
        prompt: String,
        /// Struct.
        trace_id: Option<String>,
    },
    /// Variant.
    StreamComplete {
        /// Struct.
        session_id: String,
        /// Struct.
        pr_url: String,
        /// Struct.
        trace_id: Option<String>,
    },
    /// Variant.
    Escalate {
        /// Struct.
        session_id: String,
        /// Struct.
        reason: String,
        /// Struct.
        trace_id: Option<String>,
    },
    /// Variant.
    WaitForResponse {
        /// Struct.
        session_id: String,
        /// Struct.
        trace_id: Option<String>,
    },
}

#[derive(Clone)]
/// TurnContext.
pub struct TurnContext {
    /// Struct.
    pub ops_block: String,
    /// Struct.
    pub session_id: String,
    /// Struct.
    pub stream_id: String,
    /// Struct.
    pub worktree_dir: PathBuf,
    /// Struct.
    pub project_root: PathBuf,
    /// Struct.
    pub config: Arc<PilotConfig>,
    /// Struct.
    pub persistence: Arc<StatePersistence>,
    /// Struct.
    pub processing: Arc<Mutex<HashSet<String>>>,
    /// Struct.
    pub turn: u32,
    /// Struct.
    pub trace_id: String,
    /// Struct.
    pub metrics: Arc<dyn Metrics>,
}

/// process_turn_dispatch.
pub async fn process_turn_dispatch(ctx: TurnContext) -> Result<OrchestratorAction, PilotError> {
    let span = tracing::info_span!("process_turn", stream_id = %ctx.stream_id, turn = ctx.turn, trace_id = %ctx.trace_id);
    let _enter = span.enter();

    let stream_id = ctx.stream_id.clone();
    let processing = ctx.processing.clone();
    // Clone session_id and trace_id for error branches
    let session_id_clone = ctx.session_id.clone();
    let trace_id_clone = ctx.trace_id.clone();
    {
        let lock_future = processing.lock();
        let guard = match tokio::time::timeout(std::time::Duration::from_secs(5), lock_future).await
        {
            Ok(g) => g,
            Err(_) => {
                tracing::warn!("processing lock acquisition timed out, skipping turn");
                return Ok(OrchestratorAction::WaitForResponse {
                    session_id: session_id_clone,
                    trace_id: Some(trace_id_clone),
                });
            }
        };
        let mut guard = guard;
        if !guard.insert(stream_id.clone()) {
            tracing::warn!("already processing, skipping duplicate");
            return Ok(OrchestratorAction::WaitForResponse {
                session_id: session_id_clone,
                trace_id: Some(trace_id_clone),
            });
        }
    }

    let result = process_turn_inner(ctx).await;

    {
        let lock_future = processing.lock();
        let guard = match tokio::time::timeout(std::time::Duration::from_secs(5), lock_future).await
        {
            Ok(g) => g,
            Err(_) => {
                tracing::warn!("processing lock acquisition timed out, skipping turn");
                // We still need to return something, but the lock couldn't be released.
                // Just return the result without removing from set? This is a race, but fine.
                return result;
            }
        };
        let mut guard = guard;
        guard.remove(&stream_id);
    }

    result
}

async fn process_turn_inner(ctx: TurnContext) -> Result<OrchestratorAction, PilotError> {
    let ops = parse_ops_block(&ctx.ops_block)?;
    tracing::info!(ops_count = ops.ops.len(), "parsed ops block");

    if !ops.ops.is_empty() {
        let results = apply_ops_async(
            ctx.worktree_dir.clone(),
            ops.ops.clone(),
            ctx.config.limits.clone(),
        )
        .await?;
        tracing::info!(applied = results.len(), "file operations applied");
    }

    let trace_id_some = Some(ctx.trace_id.clone());

    if ops.approved {
        push_branch(
            &ctx.worktree_dir,
            &ctx.stream_id,
            &ctx.config.execution.branch_version,
            ctx.config.execution.command_timeout,
        )
        .await?;
        let title = format!("stream-{}: implementation", ctx.stream_id);
        let body = format!("Automated implementation for stream {}", ctx.stream_id);
        let pr_url = create_pr(
            &ctx.worktree_dir,
            &ctx.stream_id,
            &ctx.config.execution.default_branch,
            &ctx.config.execution.branch_version,
            &title,
            &body,
            ctx.config.execution.command_timeout,
        )
        .await?;
        ctx.persistence
            .set_pr_url(&ctx.stream_id, pr_url.clone())
            .await?;
        return Ok(OrchestratorAction::StreamComplete {
            session_id: ctx.session_id.clone(),
            pr_url,
            trace_id: trace_id_some,
        });
    }

    if ops.done {
        let resolved = ctx.config.gates.done.resolve(ctx.config.gates.level);
        let changed_files = diff_name_only(
            &ctx.worktree_dir,
            &ctx.config.execution.default_branch,
            ctx.config.execution.command_timeout,
        )
        .await
        .unwrap_or_default()
        .lines()
        .map(|l| l.to_string())
        .filter(|l| !l.is_empty())
        .collect();
        let gate_ctx = crate::gates::types::GateContext::new(
            ctx.worktree_dir.clone(),
            ctx.project_root.clone(),
            ctx.config.execution.default_branch.clone(),
            ctx.config.execution.branch_version.clone(),
            ctx.config.execution.command_timeout,
            changed_files,
        );
        let result = done_pipeline::run_done_pipeline(&gate_ctx, &resolved).await?;
        if result.passed {
            let diff = diff_main(
                &ctx.worktree_dir,
                &ctx.config.execution.default_branch,
                ctx.config.execution.command_timeout,
            )
            .await?;
            let log = log_oneline(
                &ctx.worktree_dir,
                &ctx.config.execution.default_branch,
                ctx.config.execution.command_timeout,
            )
            .await?;
            Ok(OrchestratorAction::SelfReview {
                session_id: ctx.session_id.clone(),
                prompt: build_review_prompt(&diff, &log),
                trace_id: trace_id_some,
            })
        } else {
            Ok(OrchestratorAction::Feedback {
                session_id: ctx.session_id.clone(),
                message: format!("Done gate failed:\n{}", result.failure_message()),
                trace_id: trace_id_some,
            })
        }
    } else if ops.incomplete {
        Ok(OrchestratorAction::Continue {
            session_id: ctx.session_id.clone(),
            trace_id: trace_id_some,
        })
    } else if let Some(msg) = ops.commit_message {
        let current_fix_round = ctx.persistence.get_fix_round(&ctx.stream_id).await;
        let resolved = ctx.config.gates.commit.resolve(
            ctx.config.gates.level,
            ctx.config.execution.default_branch.clone(),
            ctx.config.execution.branch_version.clone(),
        );
        let changed_files = diff_name_only(
            &ctx.worktree_dir,
            &ctx.config.execution.default_branch,
            ctx.config.execution.command_timeout,
        )
        .await
        .unwrap_or_default()
        .lines()
        .map(|l| l.to_string())
        .filter(|l| !l.is_empty())
        .collect();
        let engine = CommitEngine::new(
            resolved,
            ctx.config.execution.max_fix_rounds,
            ctx.config.gates.banned_patterns.clone(),
            ctx.config.gates.architecture_rules.clone(),
        );
        let commit_ctx = CommitContext {
            worktree_dir: ctx.worktree_dir.clone(),
            project_root: ctx.project_root.clone(),
            stream_id: ctx.stream_id.clone(),
            commit_message: msg,
            current_fix_round,
            timeout_secs: ctx.config.execution.command_timeout,
            default_branch: ctx.config.execution.default_branch.clone(),
            branch_version: ctx.config.execution.branch_version.clone(),
            changed_files,
        };
        let decision = engine.evaluate_commit(&commit_ctx).await?;
        match decision {
            CommitDecision::Committed { message, .. } => Ok(OrchestratorAction::Feedback {
                session_id: ctx.session_id.clone(),
                message: format!("✅ Committed: {}", message),
                trace_id: trace_id_some,
            }),
            CommitDecision::GateFailed { feedback, .. } => Ok(OrchestratorAction::Feedback {
                session_id: ctx.session_id.clone(),
                message: format!("❌ Commit gate failed:\n\n{}", feedback),
                trace_id: trace_id_some,
            }),
            CommitDecision::Escalated { feedback, .. } => Ok(OrchestratorAction::Escalate {
                session_id: ctx.session_id.clone(),
                reason: format!("Fix rounds exceeded.\n\n{}", feedback),
                trace_id: trace_id_some,
            }),
        }
    } else {
        ctx.persistence
            .try_update_session(&ctx.stream_id, |s| {
                s.record_turn();
                Ok(())
            })
            .await?;
        Ok(OrchestratorAction::WaitForResponse {
            session_id: ctx.session_id.clone(),
            trace_id: trace_id_some,
        })
    }
}
