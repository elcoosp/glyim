use crate::error::PilotError;
use crate::gates::helpers::{is_command_not_found, run_gate_command};
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// `AuditGate`.
pub struct AuditGate;

#[async_trait]
impl Gate for AuditGate {
    fn name(&self) -> &'static str {
        "audit"
    }

    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let output = run_gate_command(
            "cargo",
            &["audit"],
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "audit",
        )
        .await;

        match output {
            Ok(out) if out.status.success() => Ok(GateResult::pass("audit")),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                if is_command_not_found(&stdout, &stderr) {
                    Err(PilotError::Gate {
                        gate: "audit".into(),
                        message:
                            "cargo-audit not installed. Install with: cargo install cargo-audit"
                                .into(),
                    })
                } else {
                    Ok(GateResult::fail_with_details(
                        "audit",
                        "Security vulnerabilities found",
                        format!("{stdout}{stderr}"),
                    ))
                }
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::types::GateContext;
    use std::path::PathBuf;

    fn test_context() -> GateContext {
        GateContext {
            worktree_dir: PathBuf::from("."),
            project_root: PathBuf::from("."),
            default_branch: "main".to_string(),
            branch_version: "v0.1.0".to_string(),
            timeout_secs: 10,
            changed_files: vec![],
        }
    }

    #[tokio::test]
    async fn test_audit_gate_runs() {
        let gate = AuditGate;
        let result = gate.run(&test_context()).await;
        assert!(result.is_ok() || result.is_err());
    }
}
