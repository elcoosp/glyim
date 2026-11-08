use crate::error::PilotError;
use crate::gates::helpers::{run_gate_command, strip_ansi, trim_errors_and_warnings};
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// `FmtCheckGate`.
pub struct FmtCheckGate;
#[async_trait]
impl Gate for FmtCheckGate {
    fn name(&self) -> &'static str {
        "fmt"
    }
    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let output = run_gate_command(
            "cargo",
            &["fmt", "--", "--check"],
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "fmt",
        )
        .await?;
        if output.status.success() {
            Ok(GateResult::pass("fmt"))
        } else {
            Ok(GateResult::fail_with_details(
                "fmt",
                "formatting check failed",
                trim_errors_and_warnings(&strip_ansi(&String::from_utf8_lossy(&output.stdout))),
            ))
        }
    }
}
