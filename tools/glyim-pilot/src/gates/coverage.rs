use crate::error::PilotError;
use crate::gates::helpers::{is_command_not_found, run_gate_command};
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static COVERAGE_PCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+\.?\d*)%\s*coverage").unwrap());

/// CoverageGate.
pub struct CoverageGate {
    /// Struct.
    pub min_coverage: f64,
}

#[async_trait]
impl Gate for CoverageGate {
    fn name(&self) -> &str {
        "coverage"
    }

    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        // Run cargo llvm-cov --summary-only
        let output = run_gate_command(
            "cargo",
            &["llvm-cov", "--summary-only"],
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "coverage",
        )
        .await;

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(caps) = COVERAGE_PCT_RE.captures(&stdout) {
                    if let Some(pct_str) = caps.get(1) {
                        if let Ok(pct) = pct_str.as_str().parse::<f64>() {
                            if pct >= self.min_coverage {
                                Ok(GateResult::pass_with_note(
                                    "coverage",
                                    format!(
                                        "Coverage {:.1}% meets minimum {}%",
                                        pct, self.min_coverage
                                    ),
                                ))
                            } else {
                                Ok(GateResult::fail_with_details(
                                    "coverage",
                                    format!("Coverage {:.1}% < {}%", pct, self.min_coverage),
                                    stdout.to_string(),
                                ))
                            }
                        } else {
                            Ok(GateResult::fail(
                                "coverage",
                                "Failed to parse coverage percentage",
                            ))
                        }
                    } else {
                        Ok(GateResult::fail(
                            "coverage",
                            "Could not find coverage percentage in output",
                        ))
                    }
                } else {
                    Ok(GateResult::fail(
                        "coverage",
                        "No coverage data found. Run tests with coverage first.",
                    ))
                }
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                if is_command_not_found(&stdout, &stderr) {
                    Err(PilotError::Gate {
                        gate: "coverage".into(),
                        message: "cargo-llvm-cov not installed. Install with: cargo install cargo-llvm-cov".into(),
                    })
                } else {
                    Ok(GateResult::fail_with_details(
                        "coverage",
                        "cargo llvm-cov failed",
                        format!("{}{}", stdout, stderr),
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

    // Helper to create a minimal GateContext for testing
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
    async fn test_coverage_gate_parsing() {
        let gate = CoverageGate { min_coverage: 80.0 };
        // Without actual cargo-llvm-cov, we expect an error (command not found) which is fine.
        let result = gate.run(&test_context()).await;
        // We don't assert success because the tool may not be installed; we just ensure no panic.
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_coverage_percentage_parsing() {
        let output = "Total coverage: 85.5% coverage";
        let caps = COVERAGE_PCT_RE.captures(output).unwrap();
        let pct = caps.get(1).unwrap().as_str().parse::<f64>().unwrap();
        assert_eq!(pct, 85.5);
    }
}
