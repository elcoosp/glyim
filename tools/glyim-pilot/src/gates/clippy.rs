use crate::error::PilotError;
use crate::gates::helpers::{run_gate_command, strip_ansi, trim_errors_and_warnings};
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// `ClippyGate`.
pub struct ClippyGate;
#[async_trait]
impl Gate for ClippyGate {
    fn name(&self) -> &'static str {
        "clippy"
    }
    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let output = run_gate_command(
            "cargo",
            &["clippy", "--", "-D", "warnings"],
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "clippy",
        )
        .await?;
        if output.status.success() {
            Ok(GateResult::pass("clippy"))
        } else {
            Ok(GateResult::fail_with_details(
                "clippy",
                "clippy warnings found",
                trim_errors_and_warnings(&strip_ansi(&String::from_utf8_lossy(&output.stderr))),
            ))
        }
    }
}
