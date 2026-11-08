use crate::error::PilotError;
use crate::gates::helpers::{run_gate_command, strip_ansi};
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// `DeadCodeGate`.
pub struct DeadCodeGate;
#[async_trait]
impl Gate for DeadCodeGate {
    fn name(&self) -> &'static str {
        "dead_code"
    }
    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let output = run_gate_command(
            "cargo",
            &[
                "check",
                "--all-targets",
                "--",
                "-W",
                "dead_code",
                "-W",
                "unused_imports",
            ],
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "dead_code",
        )
        .await?;
        if !output.status.success() {
            return Ok(GateResult::fail(
                "dead_code",
                "cargo check failed – fix compilation first",
            ));
        }
        let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
        if stderr.contains("dead_code") || stderr.contains("unused") {
            Ok(GateResult::fail_with_details(
                "dead_code",
                "dead code or unused imports found",
                stderr,
            ))
        } else {
            Ok(GateResult::pass("dead_code"))
        }
    }
}
