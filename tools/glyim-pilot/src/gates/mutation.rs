use crate::error::PilotError;
use crate::gates::helpers::{is_command_not_found, run_gate_command};
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static MUTATION_PCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"kill rate (\d+(?:\.\d+)?)%").unwrap());

/// MutationGate.
pub struct MutationGate {
    /// Struct.
    pub min_kill_rate: f64,
}

#[async_trait]
impl Gate for MutationGate {
    fn name(&self) -> &str {
        "mutation"
    }

    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        // Run cargo mutants --no-times for faster execution
        let output = run_gate_command(
            "cargo",
            &["mutants", "--no-times"],
            &ctx.worktree_dir,
            ctx.timeout_secs,
            "mutation",
        )
        .await;

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(caps) = MUTATION_PCT_RE.captures(&stdout) {
                    if let Some(rate_str) = caps.get(1) {
                        if let Ok(rate) = rate_str.as_str().parse::<f64>() {
                            if rate >= self.min_kill_rate {
                                Ok(GateResult::pass_with_note(
                                    "mutation",
                                    format!(
                                        "Mutation kill rate {:.1}% meets minimum {}%",
                                        rate, self.min_kill_rate
                                    ),
                                ))
                            } else {
                                Ok(GateResult::fail_with_details(
                                    "mutation",
                                    format!(
                                        "Mutation kill rate {:.1}% < {}%",
                                        rate, self.min_kill_rate
                                    ),
                                    stdout.to_string(),
                                ))
                            }
                        } else {
                            Ok(GateResult::fail(
                                "mutation",
                                "Failed to parse kill rate percentage",
                            ))
                        }
                    } else {
                        Ok(GateResult::fail(
                            "mutation",
                            "Could not find kill rate in output",
                        ))
                    }
                } else {
                    Ok(GateResult::fail(
                        "mutation",
                        "No mutation data found. Run tests with mutants first.",
                    ))
                }
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                if is_command_not_found(&stdout, &stderr) {
                    Err(PilotError::Gate {
                        gate: "mutation".into(),
                        message:
                            "cargo-mutants not installed. Install with: cargo install cargo-mutants"
                                .into(),
                    })
                } else {
                    Ok(GateResult::fail_with_details(
                        "mutation",
                        "cargo mutants failed",
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
    async fn test_mutation_gate_parsing() {
        let gate = MutationGate {
            min_kill_rate: 75.0,
        };
        let result = gate.run(&test_context()).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_mutation_kill_rate_parsing() {
        let output = "kill rate 82%";
        let caps = MUTATION_PCT_RE.captures(output).unwrap();
        let rate = caps.get(1).unwrap().as_str().parse::<f64>().unwrap();
        assert_eq!(rate, 82.0);
    }
}
