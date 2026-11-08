use crate::config::types::ResolvedDoneGates;
use crate::error::PilotError;
use crate::gates::types::GateContext;
use crate::gates::{
    audit::AuditGate, coverage::CoverageGate, dead_code::DeadCodeGate, mutation::MutationGate,
    workspace_check::WorkspaceCheckGate, Gate, PipelineResult,
};
use std::sync::Arc;

/// `run_done_pipeline`.
pub async fn run_done_pipeline(
    ctx: &GateContext,
    config: &ResolvedDoneGates,
) -> Result<PipelineResult, PilotError> {
    let mut gates: Vec<Arc<dyn Gate>> = Vec::new();
    if config.dead_code {
        gates.push(Arc::new(DeadCodeGate));
    }
    if config.coverage {
        gates.push(Arc::new(CoverageGate {
            min_coverage: config.coverage_min,
        }));
    }
    if config.mutation {
        gates.push(Arc::new(MutationGate {
            min_kill_rate: config.mutation_kill_rate,
        }));
    }
    if config.workspace_check {
        gates.push(Arc::new(WorkspaceCheckGate));
    }
    if config.audit {
        gates.push(Arc::new(AuditGate));
    }

    if gates.is_empty() {
        return Ok(PipelineResult::from_gates(vec![
            crate::gates::types::GateResult::pass("done_pipeline"),
        ]));
    }

    let mut results = Vec::new();
    for gate in &gates {
        let result = gate.run(ctx).await?;
        let passed = result.passed;
        results.push(result);
        if !passed {
            break;
        }
    }
    Ok(PipelineResult::from_gates(results))
}
