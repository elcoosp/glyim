//! Crate root.
/// Applier mod.
pub mod applier;
/// cli.
pub mod cli;
/// commit.
pub mod commit;
/// config.
pub mod config;
/// context.
pub mod context;
/// dispatch.
pub mod dispatch;
pub mod domain_types;
/// error.
pub mod error;
/// gates.
pub mod gates;
/// `git_ops`.
pub mod git_ops;
/// metrics.
pub mod metrics;
/// orchestrator.
pub mod orchestrator;
/// process.
pub mod process;
/// protocol.
pub mod protocol;
/// server.
pub mod server;
/// session.
pub mod session;

pub use applier::{
    apply_ops, apply_ops_async, preview_ops, preview_ops_async, ApplyAction, ApplyResult,
    PlannedAction, PlannedChange,
};
pub use domain_types::{ApplyLimits, BannedPattern, DependencyRule};
pub use error::PilotError;
pub use protocol::parser::{extract_ops_blocks, parse_ops_block};
pub use protocol::types::{FileOp, ParsedOps, PROTOCOL_VERSION};
