pub mod architecture;
/// audit.
pub mod audit;
pub mod banned_pattern;
/// check.
pub mod check;
/// clippy.
pub mod clippy;
/// commit_pipeline.
pub mod commit_pipeline;
/// contracts.
pub mod contracts;
/// coverage.
pub mod coverage;
/// dead_code.
pub mod dead_code;
/// done_pipeline.
pub mod done_pipeline;
/// fmt_check.
pub mod fmt_check;
/// fmt_fix.
pub mod fmt_fix;
/// helpers.
pub mod helpers;
/// mutation.
pub mod mutation;
/// self_review.
pub mod self_review;
/// test.
pub mod test;
/// types.
pub mod types;
/// workspace_check.
pub mod workspace_check;

use crate::error::PilotError;
use crate::gates::types::GateContext;
use async_trait::async_trait;

pub use types::{GateResult, GateSideEffect, PipelineResult};

#[async_trait]
/// Gate.
pub trait Gate: Send + Sync {
    /// name.
    fn name(&self) -> &str;
    /// run.
    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError>;
}
