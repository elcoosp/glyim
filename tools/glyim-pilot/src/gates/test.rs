use crate::error::PilotError;
use crate::gates::helpers::{run_gate_command, strip_ansi, trim_test_failures};
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// `TestGate`.
pub struct TestGate;
#[async_trait]
impl Gate for TestGate {
    fn name(&self) -> &'static str {
        "test"
    }
    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let output = run_gate_command(
            "cargo",
            &["test"],
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "test",
        )
        .await?;
        if output.status.success() {
            Ok(GateResult::pass("test"))
        } else {
            let combined = format!(
                "{}\n{}",
                strip_ansi(&String::from_utf8_lossy(&output.stdout)),
                strip_ansi(&String::from_utf8_lossy(&output.stderr))
            );
            Ok(GateResult::fail_with_details(
                "test",
                "test failures detected",
                trim_test_failures(&combined),
            ))
        }
    }
}
