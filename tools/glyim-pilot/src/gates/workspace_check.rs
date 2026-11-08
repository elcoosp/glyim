use crate::error::PilotError;
use crate::gates::helpers::{run_gate_command, strip_ansi};
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// `WorkspaceCheckGate`.
pub struct WorkspaceCheckGate;
#[async_trait]
impl Gate for WorkspaceCheckGate {
    fn name(&self) -> &'static str {
        "workspace_check"
    }
    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let output = run_gate_command(
            "cargo",
            &["check", "--workspace"],
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "workspace_check",
        )
        .await?;
        if output.status.success() {
            Ok(GateResult::pass("workspace_check"))
        } else {
            Ok(GateResult::fail_with_details(
                "workspace_check",
                "workspace check failed",
                strip_ansi(&String::from_utf8_lossy(&output.stderr)),
            ))
        }
    }
}
