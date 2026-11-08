use crate::config::types::ResolvedCommitGates;
use crate::domain_types::{BannedPattern, DependencyRule};
use crate::error::PilotError;
use crate::gates::types::GateContext;
use crate::gates::{
    architecture::ArchitectureGate, banned_pattern::BannedPatternGate, check::CheckGate,
    clippy::ClippyGate, contracts::ContractGate, fmt_check::FmtCheckGate, test::TestGate, Gate,
    PipelineResult,
};
use std::sync::Arc;
use std::time::Instant;

/// `run_commit_pipeline`.
pub async fn run_commit_pipeline(
    ctx: &GateContext,
    config: &ResolvedCommitGates,
    banned_patterns: Vec<BannedPattern>,
    architecture_rules: Vec<DependencyRule>,
) -> Result<PipelineResult, PilotError> {
    let mut gates: Vec<Arc<dyn Gate>> = Vec::new();
    if config.fmt {
        gates.push(Arc::new(FmtCheckGate));
    }
    if config.check {
        gates.push(Arc::new(CheckGate));
    }
    if config.clippy {
        gates.push(Arc::new(ClippyGate));
    }
    if config.test {
        gates.push(Arc::new(TestGate));
    }
    if config.banned_patterns {
        gates.push(Arc::new(BannedPatternGate::new(banned_patterns)));
    }
    if config.architecture {
        gates.push(Arc::new(ArchitectureGate::new(architecture_rules)));
    }
    if config.contracts {
        gates.push(Arc::new(ContractGate));
    }

    let mut results = Vec::new();
    for gate in &gates {
        let start = Instant::now();
        let result = gate.run(ctx).await?;
        tracing::info!(gate = gate.name(), elapsed = ?start.elapsed(), passed = result.passed, "commit gate completed");
        let passed = result.passed;
        results.push(result);
        if !passed {
            break;
        }
    }
    Ok(PipelineResult::from_gates(results))
}
