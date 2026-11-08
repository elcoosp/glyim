use crate::error::PilotError;
use crate::gates::helpers::{run_gate_command, strip_ansi, trim_errors_and_warnings};
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// `CheckGate`.
pub struct CheckGate;
#[async_trait]
impl Gate for CheckGate {
    fn name(&self) -> &'static str {
        "check"
    }
    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let output = run_gate_command(
            "cargo",
            &["check"],
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "check",
        )
        .await?;
        if output.status.success() {
            Ok(GateResult::pass("check"))
        } else {
            Ok(GateResult::fail_with_details(
                "check",
                "compilation failed",
                trim_errors_and_warnings(&strip_ansi(&String::from_utf8_lossy(&output.stderr))),
            ))
        }
    }
}
