use crate::config::types::ResolvedCommitGates;
use crate::domain_types::{BannedPattern, DependencyRule};
use crate::error::PilotError;
use crate::gates::commit_pipeline;
use crate::gates::fmt_fix;
use crate::gates::types::GateContext;
use crate::git_ops::{commit_all, emergency_wip_commit};
use std::path::PathBuf;

#[derive(Debug, Clone)]
/// CommitDecision.
pub enum CommitDecision {
    /// Variant.
    Committed {
        /// Struct.
        message: String,
        /// Struct.
        new_fix_round: u32,
    },
    /// Variant.
    GateFailed {
        /// Struct.
        new_fix_round: u32,
        /// Struct.
        feedback: String,
    },
    /// Variant.
    Escalated {
        /// Struct.
        new_fix_round: u32,
        /// Struct.
        feedback: String,
    },
}

impl CommitDecision {
    /// new_fix_round.
    pub fn new_fix_round(&self) -> u32 {
        match self {
            Self::Committed { new_fix_round, .. } => *new_fix_round,
            Self::GateFailed { new_fix_round, .. } => *new_fix_round,
            Self::Escalated { new_fix_round, .. } => *new_fix_round,
        }
    }
}

#[derive(Debug, Clone)]
/// CommitContext.
pub struct CommitContext {
    /// Struct.
    pub worktree_dir: PathBuf,
    /// Struct.
    pub project_root: PathBuf,
    /// Struct.
    pub stream_id: String,
    /// Struct.
    pub commit_message: String,
    /// Struct.
    pub current_fix_round: u32,
    /// Struct.
    pub timeout_secs: u64,
    /// Struct.
    pub default_branch: String,
    /// Struct.
    pub branch_version: String,
    /// Struct.
    pub changed_files: Vec<String>,
}

/// CommitEngine.
pub struct CommitEngine {
    gate_config: ResolvedCommitGates,
    max_fix_rounds: u32,
    banned_patterns: Vec<BannedPattern>,
    architecture_rules: Vec<DependencyRule>,
}

impl CommitEngine {
    /// new.
    pub fn new(
        gate_config: ResolvedCommitGates,
        max_fix_rounds: u32,
        banned_patterns: Vec<BannedPattern>,
        architecture_rules: Vec<DependencyRule>,
    ) -> Self {
        Self {
            gate_config,
            max_fix_rounds,
            banned_patterns,
            architecture_rules,
        }
    }

    /// evaluate_commit.
    pub async fn evaluate_commit(&self, ctx: &CommitContext) -> Result<CommitDecision, PilotError> {
        let gate_ctx = GateContext::new(
            ctx.worktree_dir.clone(),
            ctx.project_root.clone(),
            ctx.default_branch.clone(),
            ctx.branch_version.clone(),
            ctx.timeout_secs,
            ctx.changed_files.clone(),
        );

        let pipeline_result = commit_pipeline::run_commit_pipeline(
            &gate_ctx,
            &self.gate_config,
            self.banned_patterns.clone(),
            self.architecture_rules.clone(),
        )
        .await?;

        if pipeline_result.passed {
            commit_all(
                &ctx.worktree_dir,
                &ctx.stream_id,
                &ctx.commit_message,
                ctx.timeout_secs,
            )
            .await?;
            Ok(CommitDecision::Committed {
                message: ctx.commit_message.clone(),
                new_fix_round: 0,
            })
        } else {
            let fmt_failed = pipeline_result
                .gates
                .iter()
                .any(|g| g.gate_name == "fmt" && !g.passed);
            if fmt_failed {
                let fix_result = fmt_fix::run_fmt_fix(&gate_ctx).await?;
                if fix_result.passed {
                    let updated_changed = crate::git_ops::diff_name_only(
                        &ctx.worktree_dir,
                        &ctx.default_branch,
                        ctx.timeout_secs,
                    )
                    .await
                    .unwrap_or_default()
                    .lines()
                    .map(|l| l.to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                    let retry_ctx = GateContext::new(
                        ctx.worktree_dir.clone(),
                        ctx.project_root.clone(),
                        ctx.default_branch.clone(),
                        ctx.branch_version.clone(),
                        ctx.timeout_secs,
                        updated_changed,
                    );
                    let retry_result = commit_pipeline::run_commit_pipeline(
                        &retry_ctx,
                        &self.gate_config,
                        self.banned_patterns.clone(),
                        self.architecture_rules.clone(),
                    )
                    .await?;
                    if retry_result.passed {
                        let fix_msg = format!("{} (fmt auto-fixed)", ctx.commit_message);
                        commit_all(
                            &ctx.worktree_dir,
                            &ctx.stream_id,
                            &fix_msg,
                            ctx.timeout_secs,
                        )
                        .await?;
                        return Ok(CommitDecision::Committed {
                            message: fix_msg,
                            new_fix_round: 0,
                        });
                    }
                    let feedback = retry_result.failure_message();
                    return self.escalate_or_retry(ctx, ctx.current_fix_round + 1, &feedback);
                }
            }
            let feedback = pipeline_result.failure_message();
            self.escalate_or_retry(ctx, ctx.current_fix_round + 1, &feedback)
        }
    }

    fn escalate_or_retry(
        &self,
        _ctx: &CommitContext,
        new_fix_round: u32,
        feedback: &str,
    ) -> Result<CommitDecision, PilotError> {
        if new_fix_round > self.max_fix_rounds {
            Ok(CommitDecision::Escalated {
                new_fix_round,
                feedback: feedback.to_string(),
            })
        } else {
            Ok(CommitDecision::GateFailed {
                new_fix_round,
                feedback: feedback.to_string(),
            })
        }
    }

    /// emergency_commit.
    pub async fn emergency_commit(&self, ctx: &CommitContext) -> Result<(), PilotError> {
        emergency_wip_commit(&ctx.worktree_dir, &ctx.stream_id, ctx.timeout_secs).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::ResolvedCommitGates;
    use std::path::PathBuf;

    fn test_engine() -> CommitEngine {
        let gates = ResolvedCommitGates {
            fmt: true,
            check: true,
            clippy: true,
            test: true,
            banned_patterns: true,
            architecture: true,
            contracts: true,
            default_branch: "main".to_string(),
            branch_version: "v0.1.0".to_string(),
        };
        CommitEngine::new(gates, 3, vec![], vec![])
    }

    #[test]
    fn test_escalate_or_retry_within_limit() {
        let engine = test_engine();
        let ctx = CommitContext {
            worktree_dir: PathBuf::from("."),
            project_root: PathBuf::from("."),
            stream_id: "test".into(),
            commit_message: "msg".into(),
            current_fix_round: 1,
            timeout_secs: 30,
            default_branch: "main".into(),
            branch_version: "v0.1.0".into(),
            changed_files: vec![],
        };
        let result = engine.escalate_or_retry(&ctx, 2, "fail").unwrap();
        match result {
            CommitDecision::GateFailed { new_fix_round, .. } => assert_eq!(new_fix_round, 2),
            _ => panic!("expected GateFailed"),
        }
    }

    #[test]
    fn test_escalate_or_retry_exceeds_limit() {
        let engine = test_engine();
        let ctx = CommitContext {
            worktree_dir: PathBuf::from("."),
            project_root: PathBuf::from("."),
            stream_id: "test".into(),
            commit_message: "msg".into(),
            current_fix_round: 1,
            timeout_secs: 30,
            default_branch: "main".into(),
            branch_version: "v0.1.0".into(),
            changed_files: vec![],
        };
        let result = engine.escalate_or_retry(&ctx, 4, "fail").unwrap();
        match result {
            CommitDecision::Escalated { new_fix_round, .. } => assert_eq!(new_fix_round, 4),
            _ => panic!("expected Escalated"),
        }
    }
}
